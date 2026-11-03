//! What a model costs to run, and how much room that leaves it to read.
//!
//! ## The two answers that were one question
//!
//! Until now AETHER1 decided those separately. `hf::memory_needed_gb` took the size of a
//! GGUF file, added a fifth with a one-gigabyte floor, and called that what the model
//! needs. `providers::local_context_window` clamped whatever the model reported to a fixed
//! ceiling of 16K. Neither knew about the other, and the fifth was doing the work of a
//! term that is not a fifth of anything: the KV cache is linear in the context length and
//! in the model's *active* parameter count, and at 16K on a 30B it is gigabytes.
//!
//! So a quantisation that "fits" could fit only with a context too small to be useful, or
//! be declared to fit and then not fit once the window was opened. #249 is what that looks
//! like from the operator's chair: a 27B at IQ2_XS, about 8GB of weights, on a 16GB card,
//! with a context sized from *system RAM* -- the KV cache did not fit, Ollama put layers on
//! the processor, and a greeting took 167 seconds. The post-mortem's conclusion was that
//! RAM "was the wrong question twice over: it is not what runs out (VRAM is), and even VRAM
//! cannot settle it without the model's size". This module is that conclusion as
//! arithmetic.
//!
//! One function answers both halves: given a card and a model, `plan` returns the memory it
//! needs **and** the context that leaves. The picker shows the pair, and the per-request
//! `num_ctx` is the same number the picker promised.
//!
//! ## Why it is all pure
//!
//! Every figure here is arithmetic over tables with no I/O, for the reason `hf::Machine`
//! already gives: the sizing has to be testable without a graphics card, and CI has
//! neither a card nor a model server. What the machine *is* arrives from `gpu.rs` and what
//! the model is arrives from `/api/show`; this module only does sums on them.
//!
//! ## What it will not claim
//!
//! Tokens per second for a model partly on the processor. The speed of a partial offload is
//! bound by the slowest part of the path and that is not predictable from a specification
//! sheet, so `speed_estimate` answers `None` the moment the whole thing does not fit on the
//! card, rather than printing an encouraging number that the machine will then disprove.
//! Where AETHER1 has measured a model's real speed, that measurement is the better answer
//! and this is only the fallback for a model not yet downloaded.

/// How much room a model wants beyond its weights and its context: the runtime's own
/// working set, the compute buffers, and the fragmentation that stops a card that is
/// exactly full from being usable.
///
/// A flat figure rather than a fraction, because it is a property of the runtime rather
/// than of the model: a 1B and a 70B both allocate roughly this much to run at all.
pub const RUNTIME_BUFFER_GB: f64 = 0.6;

/// The smallest context worth opening a conversation with.
///
/// Matches `providers::LOCAL_CTX_FLOOR`, and deliberately: a model that cannot be given
/// this much is a model that will forget the top of the question, and the honest answer is
/// to say it does not fit rather than to serve a window nobody can use. The two constants
/// are asserted equal in `providers`' own tests.
pub const CTX_FLOOR: u32 = 8_192;

/// The largest context AETHER1 will ask a local model for.
///
/// Matches `providers::LOCAL_CTX_CEILING`. Not a memory limit -- it is the point past which
/// a bigger window stops buying anything for the conversations AETHER1 actually has, and
/// starts costing a reload. See the note there.
pub const CTX_CEILING: u32 = 16_384;

/// The context sizes to consider, largest first.
///
/// A ladder rather than a continuous figure because Ollama keys a loaded instance partly on
/// its context size: two nearby values are two model loads, and a window that drifts with
/// the conversation is the second-copy trap `providers` documents. Rungs are far enough
/// apart that the answer is stable for a given model on a given card.
const CTX_LADDER: [u32; 4] = [16_384, 12_288, 10_240, 8_192];

/// Bytes per parameter, by quantisation label.
///
/// The figures are the effective file-size-per-parameter of each GGUF scheme rather than
/// its nominal bit width: `Q4_K_M` is not four bits a parameter, because the K-quants keep
/// some tensors at higher precision and every GGUF carries embeddings that are not
/// quantised as far as the body. These are what the files on the hub actually weigh.
const QUANT_BPP: &[(&str, f64)] = &[
    ("Q2_K", 0.40),
    ("IQ2", 0.36),
    ("Q3_K_S", 0.46),
    ("Q3_K_M", 0.51),
    ("Q3_K_L", 0.55),
    ("Q3_K", 0.51),
    ("IQ3", 0.44),
    ("Q4_0", 0.59),
    ("Q4_1", 0.65),
    ("Q4_K_S", 0.58),
    ("Q4_K_M", 0.62),
    ("Q4_K", 0.62),
    ("IQ4", 0.56),
    ("Q5_0", 0.71),
    ("Q5_1", 0.77),
    ("Q5_K_S", 0.71),
    ("Q5_K_M", 0.73),
    ("Q5_K", 0.73),
    ("Q6_K", 0.84),
    ("Q8_0", 1.09),
    ("BF16", 2.0),
    ("FP16", 2.0),
    ("F16", 2.0),
    ("F32", 4.0),
];

/// Bytes per parameter for one quantisation label, or `None` for a label not recognised.
///
/// Longest match first: `Q4_K_M` must not be answered by the `Q4_K` row, and `Q3_K_L` must
/// not be answered by `Q3_K`. Returns `None` rather than a default, because a quantisation
/// nobody here knows is a quantisation whose size should be read off the file instead.
pub fn bytes_per_param(label: &str) -> Option<f64> {
    let upper = label.trim().to_uppercase();
    QUANT_BPP
        .iter()
        .filter(|(name, _)| upper.starts_with(name))
        .max_by_key(|(name, _)| name.len())
        .map(|(_, bpp)| *bpp)
}

/// Gigabytes of KV cache per billion active parameters per token.
///
/// Quoted for an eight-bit cache, which is what Ollama keeps unless it is told otherwise
/// and AETHER1 never tells it otherwise -- so there is one coefficient here rather than a
/// cache-precision parameter nothing would ever pass. A four-bit cache would halve this and
/// a sixteen-bit one would double it; if AETHER1 ever drives that setting, this is the
/// figure to scale.
///
/// Empirical, and necessarily so: the exact cost depends on the layer count, the head count
/// and the head dimension, none of which a repository listing reports and only some of
/// which `/api/show` does. This is the coefficient that lands closest across the GGUF
/// models people actually run, and it is used for the question being asked -- "is there
/// room for this window" -- not to predict an allocation to the megabyte.
const KV_GB_PER_B_PER_TOKEN: f64 = 0.000_008;

/// What the KV cache costs, in gigabytes, at one context length.
///
/// **Active** parameters, not total. A mixture-of-experts model routes each token through a
/// fraction of its weights, and its cache is sized by that fraction: `Qwen3-30B-A3B` caches
/// like a 3B and not like a 30B, which is the whole reason it is worth running on a small
/// card. Costing it at 30B would refuse a model that fits comfortably.
pub fn kv_gb(active_params_b: f64, context: u32) -> f64 {
    if active_params_b <= 0.0 {
        return 0.0;
    }
    KV_GB_PER_B_PER_TOKEN * active_params_b * f64::from(context)
}

/// A model, as much of it as a fit needs to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    /// What the weights weigh, in gigabytes. From the file size where there is one, and
    /// from the parameter count times the quantisation where there is not.
    pub weights_gb: f64,
    /// Parameters routed per token, in billions -- the figure the KV cache is sized by.
    /// Equal to the total for a dense model.
    pub active_params_b: f64,
}

impl Model {
    /// A model from a GGUF's size on disk and its active parameter count.
    pub fn from_file(bytes: u64, active_params_b: f64) -> Model {
        Model {
            weights_gb: bytes as f64 / 1_000_000_000.0,
            active_params_b,
        }
    }

    /// A model from a parameter count and a quantisation, for a file whose size nobody
    /// reported. `None` when the quantisation is not one of the known ones, because a
    /// weight estimated from an unknown bytes-per-parameter is a number made up twice.
    pub fn from_quant(total_params_b: f64, active_params_b: f64, quant: &str) -> Option<Model> {
        let bpp = bytes_per_param(quant)?;
        Some(Model {
            weights_gb: total_params_b * bpp,
            active_params_b,
        })
    }
}

/// What this machine has to judge a model against.
///
/// Passed in rather than probed here so the sizing is a pure function of two numbers and
/// can be tested without a machine. The two fractions are the ones `setup::models_for` and
/// `hf` already used, and for the reasons stated there: the operating system and the HUD
/// are already resident in RAM, and a card is holding the compositor's framebuffers and the
/// HUD's own scene.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Machine {
    pub ram_total_gb: f64,
    pub vram_gb: Option<f64>,
}

impl Machine {
    pub fn usable_ram(&self) -> f64 {
        self.ram_total_gb * 0.7
    }

    pub fn usable_vram(&self) -> Option<f64> {
        self.vram_gb.filter(|gb| *gb > 0.0).map(|gb| gb * 0.9)
    }
}

/// What running one model on one machine costs, and what it leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Total memory the model occupies at `context`: weights, cache and runtime.
    pub needs_gb: f64,
    /// The weights alone.
    pub weights_gb: f64,
    /// The cache at `context`.
    pub kv_gb: f64,
    /// The context this machine has room for, from `CTX_LADDER`.
    ///
    /// `None` when there is not room for `CTX_FLOOR`, which is the honest answer and not
    /// the same as a small number: a window below the floor is one the model will overflow
    /// on the first real question.
    pub context: Option<u32>,
    /// Whether the whole of it fits on the graphics card. `None` when there is no card to
    /// speak of -- which is not the same sentence as "it does not fit".
    pub on_gpu: Option<bool>,
    /// Whether this machine has the memory for it at all, card or no card.
    pub fits: bool,
}

/// What a model costs here, and the largest window it leaves room for.
///
/// The order matters and is the point of the module: the context is chosen *after* the
/// weights are paid for, out of what is left, rather than being a constant the weights are
/// then expected to fit around. That is the inversion #249 got backwards.
pub fn plan(model: Model, machine: Machine) -> Plan {
    /* Whether it can be on the card at all is settled first, at the smallest window worth
    opening: a model that cannot hold the floor on the card will not hold anything on it,
    and asking the question at the ceiling would answer "no card" for a model that fits
    perfectly well with a shorter window. */
    let at_floor = model.weights_gb + kv_gb(model.active_params_b, CTX_FLOOR) + RUNTIME_BUFFER_GB;
    let on_gpu = machine.usable_vram().map(|vram| at_floor <= vram);

    /* And the window comes out of whichever memory it is going to live in. On the card
    when it fits there; out of system memory when it does not, because a model running on
    the processor is bound by RAM and not by a card it is not using. Two different places,
    one arithmetic. */
    let room = match (on_gpu, machine.usable_vram()) {
        (Some(true), Some(vram)) => vram,
        _ => machine.usable_ram(),
    };
    let context = context_that_fits(model, room);

    // The memory figure is quoted at the window that will actually be opened. Where none
    // fits, quote it at the floor -- the honest "this is what it would need" to put beside
    // "and there is not room for it".
    let kv = kv_gb(model.active_params_b, context.unwrap_or(CTX_FLOOR));
    let needs = model.weights_gb + kv + RUNTIME_BUFFER_GB;
    Plan {
        needs_gb: needs,
        weights_gb: model.weights_gb,
        kv_gb: kv,
        context,
        on_gpu,
        fits: needs <= machine.usable_ram().max(room),
    }
}

/// The largest rung of the ladder whose weights, cache and runtime fit in `room_gb`.
pub fn context_that_fits(model: Model, room_gb: f64) -> Option<u32> {
    CTX_LADDER.into_iter().find(|rung| {
        model.weights_gb + kv_gb(model.active_params_b, *rung) + RUNTIME_BUFFER_GB <= room_gb
    })
}

/// What happens to what is already on the card when this model is loaded.
///
/// The sentence a picker that only knows arithmetic cannot write. AETHER1 asks the server
/// what it has resident (`providers::loaded_models` and the memory figure beside it), so it
/// can say not merely "this fits" but "this fits *as well*" -- or warn that loading it will
/// turn out the model the operator is mid-conversation with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Residency {
    /// Nothing is loaded, or there is no card to be loaded onto.
    Alone,
    /// It fits in what is left after what is already there.
    Alongside,
    /// It fits on the card, but only once the server has turned something else out.
    Evicts,
    /// It does not fit on the card even with the card to itself.
    TooBig,
}

/// Which of those four this is.
///
/// `resident_gb` is what the server reports it is already holding in video memory. Zero, or
/// a machine with no card, is `Alone`.
pub fn residency(plan: &Plan, machine: Machine, resident_gb: f64) -> Residency {
    let Some(vram) = machine.usable_vram() else {
        return Residency::Alone;
    };
    if plan.on_gpu != Some(true) || plan.needs_gb > vram {
        return Residency::TooBig;
    }
    if resident_gb <= 0.0 {
        return Residency::Alone;
    }
    if plan.needs_gb <= vram - resident_gb {
        Residency::Alongside
    } else {
        Residency::Evicts
    }
}

/// Memory bandwidth in gigabytes a second, by what the driver calls the card.
///
/// Bandwidth rather than core count, because generating a token reads every active weight
/// once: on everything AETHER1 runs, decode is bound by how fast the weights can be moved
/// and not by how fast they can be multiplied. The entries are the manufacturers' own
/// figures.
const BANDWIDTH: &[(&str, f64)] = &[
    // AMD, desktop
    ("9070 XT", 624.0),
    ("9070", 488.0),
    ("9060 XT", 322.0),
    ("7900 XTX", 960.0),
    ("7900 XT", 800.0),
    ("7900 GRE", 576.0),
    ("7800 XT", 624.0),
    ("7700 XT", 432.0),
    ("7600", 288.0),
    ("6950 XT", 576.0),
    ("6900 XT", 512.0),
    ("6800 XT", 512.0),
    ("6800", 512.0),
    ("6700 XT", 384.0),
    ("6600 XT", 256.0),
    ("6600", 224.0),
    // NVIDIA, desktop and mobile
    ("5090", 1792.0),
    ("5080", 960.0),
    ("5070 TI", 896.0),
    ("5070", 672.0),
    ("5060 TI", 448.0),
    ("5060", 256.0),
    ("4090", 1008.0),
    ("4080 SUPER", 736.0),
    ("4080", 717.0),
    ("4070 TI SUPER", 672.0),
    ("4070 TI", 504.0),
    ("4070", 504.0),
    ("4060 TI", 288.0),
    ("4060", 272.0),
    ("3090 TI", 1008.0),
    ("3090", 936.0),
    ("3080 TI", 912.0),
    ("3080", 760.0),
    ("3070 TI", 608.0),
    ("3070", 448.0),
    ("3060 TI", 448.0),
    ("3060", 360.0),
    ("3050", 224.0),
    ("2080 TI", 616.0),
    ("2080", 448.0),
    ("2070", 448.0),
    ("2060", 336.0),
    ("1660 TI", 288.0),
    ("1660", 192.0),
    ("1650", 128.0),
    ("1050 TI", 112.0),
    ("1050", 112.0),
];

/// How fast a card can move its own memory, or `None` for a card not in the table.
///
/// Matched longest-first so that `1050 Ti` is not answered by the `1050` row and
/// `4070 Ti Super` is not answered by `4070 Ti`.
pub fn bandwidth_gbs(gpu_name: &str) -> Option<f64> {
    let upper = gpu_name.to_uppercase();
    BANDWIDTH
        .iter()
        .filter(|(name, _)| upper.contains(name))
        .max_by_key(|(name, _)| name.len())
        .map(|(_, gbs)| *gbs)
}

/// Roughly how many tokens a second this model will generate on this card.
///
/// Decode reads the active weights once per token, so the ceiling is bandwidth divided by
/// the bytes that have to be read, derated for the fact that nothing reaches its ceiling.
///
/// **`None` the moment the model does not fit on the card**, and that refusal is the honest
/// half of this function. A partial offload is bound by the processor and the bus rather
/// than by the card, and no arithmetic over a specification sheet predicts it; a number
/// printed there would be wrong in the direction that matters, which is optimistic. It is
/// also `None` for a card not in the table.
pub fn speed_estimate(plan: &Plan, gpu_name: &str) -> Option<f64> {
    if plan.on_gpu != Some(true) {
        return None;
    }
    let gbs = bandwidth_gbs(gpu_name)?;
    if plan.weights_gb <= 0.0 {
        return None;
    }
    // 0.75: the share of theoretical bandwidth a real decode loop reaches. Deliberately
    // conservative -- an estimate that disappoints is a worse answer than one that is
    // beaten.
    Some(gbs * 0.75 / plan.weights_gb)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop in the brief: a 6800 XT with 16GB, which is 14.4 usable.
    fn desktop() -> Machine {
        Machine {
            ram_total_gb: 32.0,
            vram_gb: Some(16.0),
        }
    }

    /// The laptop in the brief: a GTX 1050 Ti Mobile with 4GB, which is 3.6 usable.
    fn laptop() -> Machine {
        Machine {
            ram_total_gb: 16.0,
            vram_gb: Some(4.0),
        }
    }

    #[test]
    fn a_longer_quantisation_label_wins_over_a_shorter_prefix() {
        assert_eq!(bytes_per_param("Q4_K_M"), Some(0.62));
        assert_eq!(bytes_per_param("Q4_K_S"), Some(0.58));
        assert_eq!(bytes_per_param("Q3_K_L"), Some(0.55));
        assert_eq!(bytes_per_param("q8_0"), Some(1.09), "case is not a quant");
        assert_eq!(bytes_per_param("MOSTLY"), None, "not a quantisation");
    }

    #[test]
    fn the_cache_is_sized_by_the_parameters_a_token_actually_uses() {
        // Qwen3-30B-A3B caches like the 3B it routes through, not like the 30B it weighs.
        let dense = kv_gb(30.0, 16_384);
        let moe = kv_gb(3.0, 16_384);
        assert!(
            (dense / moe - 10.0).abs() < 1e-6,
            "ten times the active parameters is ten times the cache"
        );
        assert!(moe < 0.5, "a 3B at 16K is well under a gigabyte: {moe}");
    }

    #[test]
    fn the_cache_grows_in_step_with_the_window() {
        let floor = kv_gb(8.0, CTX_FLOOR);
        let ceiling = kv_gb(8.0, CTX_CEILING);
        assert!(
            (ceiling / floor - 2.0).abs() < 1e-9,
            "twice the window is twice the cache"
        );
    }

    #[test]
    fn a_model_with_no_parameters_named_is_charged_no_cache() {
        assert_eq!(kv_gb(0.0, 16_384), 0.0);
    }

    /// The regression this module exists for. #249: a 27B at IQ2_XS, about 8GB of weights,
    /// on the 16GB card, with the context sized from system RAM at 32768. The cache did not
    /// fit, Ollama put layers on the processor, and a greeting took 167 seconds.
    #[test]
    fn the_27b_that_took_167_seconds_is_given_a_window_that_fits() {
        let model = Model {
            weights_gb: 8.0,
            active_params_b: 27.0,
        };
        let plan = plan(model, desktop());
        let context = plan.context.expect("8GB of weights fit a 16GB card");
        assert!(
            plan.needs_gb <= desktop().usable_vram().unwrap(),
            "the figure quoted is one the card can hold: {plan:?}"
        );
        assert_eq!(plan.on_gpu, Some(true));
        assert!(
            context < 32_768,
            "the window #249 asked for is not offered: {context}"
        );
        // And the cache is the term that decided it, not a rounding error.
        assert!(
            plan.kv_gb > 1.0,
            "a 27B's cache is gigabytes, not a fifth of a file: {}",
            plan.kv_gb
        );
    }

    #[test]
    fn the_same_weights_leave_less_room_on_the_laptop() {
        // A 7B at a low quantisation: it fits on both cards, and only one of them has
        // room to open the window all the way.
        let small = Model {
            weights_gb: 2.5,
            active_params_b: 7.0,
        };
        let on_desktop = plan(small, desktop());
        let on_laptop = plan(small, laptop());
        assert_eq!(on_desktop.context, Some(CTX_CEILING));
        assert_eq!(on_laptop.context, Some(CTX_FLOOR));
        assert_eq!(
            on_laptop.on_gpu,
            Some(true),
            "it does fit, just not roomily"
        );
        assert!(
            on_laptop.kv_gb < on_desktop.kv_gb,
            "a smaller window is a smaller cache, which is the point: {on_laptop:?}"
        );
    }

    /// The ordinary 7B everybody reaches for first, at the quantisation everybody reaches
    /// for first. It is the model the laptop cannot have, and saying so before the download
    /// is most of what this is for.
    #[test]
    fn the_usual_seven_billion_at_q4_does_not_fit_the_four_gigabyte_card() {
        let seven_b = Model::from_quant(7.0, 7.0, "Q4_K_M").expect("a known quantisation");
        let on_laptop = plan(seven_b, laptop());
        assert_eq!(
            on_laptop.on_gpu,
            Some(false),
            "4.3GB of weights plus a 7B's cache is past 3.6GB of card"
        );
        assert!(
            on_laptop.fits,
            "but it will run on the processor, out of the 11.2GB of usable RAM"
        );
        assert_eq!(
            on_laptop.context,
            Some(CTX_CEILING),
            "and there the window comes out of RAM, which has room for all of it"
        );

        let on_desktop = plan(seven_b, desktop());
        assert_eq!(on_desktop.on_gpu, Some(true));
        assert_eq!(on_desktop.context, Some(CTX_CEILING));
    }

    #[test]
    fn a_model_too_big_for_the_floor_says_so_rather_than_shrinking_further() {
        let big = Model {
            weights_gb: 20.0,
            active_params_b: 70.0,
        };
        let plan = plan(big, laptop());
        assert_eq!(plan.context, None, "no rung fits, and none is invented");
        assert_eq!(plan.on_gpu, Some(false));
        assert!(!plan.fits, "20GB of weights is past 11.2GB of usable RAM");
        assert!(
            plan.needs_gb > 20.0,
            "the figure is still quoted, at the floor"
        );
    }

    #[test]
    fn no_card_is_not_the_same_answer_as_does_not_fit() {
        let onboard = Machine {
            ram_total_gb: 16.0,
            vram_gb: None,
        };
        let model = Model {
            weights_gb: 4.0,
            active_params_b: 8.0,
        };
        let plan = plan(model, onboard);
        assert_eq!(plan.on_gpu, None, "there is no card to fit on");
        assert!(plan.fits, "and it fits in 11.2GB of usable memory");
        assert!(plan.context.is_some());
    }

    #[test]
    fn the_context_is_chosen_out_of_what_the_weights_leave() {
        let room = 10.0;
        let light = Model {
            weights_gb: 2.0,
            active_params_b: 7.0,
        };
        let heavy = Model {
            weights_gb: 9.0,
            active_params_b: 7.0,
        };
        assert_eq!(context_that_fits(light, room), Some(CTX_CEILING));
        assert_eq!(
            context_that_fits(heavy, room),
            None,
            "9GB of weights plus a 7B's cache plus the runtime is past 10GB"
        );
    }

    #[test]
    fn every_rung_of_the_ladder_is_within_the_bounds_the_server_honours() {
        assert_eq!(CTX_LADDER[0], CTX_CEILING);
        assert_eq!(CTX_LADDER[CTX_LADDER.len() - 1], CTX_FLOOR);
        for pair in CTX_LADDER.windows(2) {
            assert!(pair[0] > pair[1], "largest first: {pair:?}");
        }
    }

    #[test]
    fn what_is_already_loaded_decides_whether_this_one_evicts_it() {
        let model = Model {
            weights_gb: 5.0,
            active_params_b: 8.0,
        };
        let plan = plan(model, desktop());
        assert_eq!(residency(&plan, desktop(), 0.0), Residency::Alone);
        assert_eq!(
            residency(&plan, desktop(), 3.0),
            Residency::Alongside,
            "6ish plus 3 is inside 14.4"
        );
        assert_eq!(
            residency(&plan, desktop(), 11.0),
            Residency::Evicts,
            "6ish plus 11 is not"
        );
    }

    #[test]
    fn a_model_past_the_card_is_too_big_whatever_is_resident() {
        let huge = Model {
            weights_gb: 30.0,
            active_params_b: 70.0,
        };
        let plan = plan(huge, desktop());
        assert_eq!(residency(&plan, desktop(), 0.0), Residency::TooBig);
        assert_eq!(residency(&plan, desktop(), 8.0), Residency::TooBig);
    }

    #[test]
    fn residency_on_a_machine_with_no_card_is_always_alone() {
        let onboard = Machine {
            ram_total_gb: 16.0,
            vram_gb: None,
        };
        let model = Model {
            weights_gb: 3.0,
            active_params_b: 7.0,
        };
        let plan = plan(model, onboard);
        assert_eq!(residency(&plan, onboard, 5.0), Residency::Alone);
    }

    #[test]
    fn a_card_is_matched_on_its_longest_name_not_its_first() {
        assert_eq!(bandwidth_gbs("NVIDIA GeForce GTX 1050 Ti"), Some(112.0));
        assert_eq!(
            bandwidth_gbs("NVIDIA GeForce RTX 4070 Ti SUPER"),
            Some(672.0)
        );
        assert_eq!(bandwidth_gbs("NVIDIA GeForce RTX 4070 Ti"), Some(504.0));
        assert_eq!(bandwidth_gbs("AMD Radeon RX 6800 XT"), Some(512.0));
        assert_eq!(bandwidth_gbs("Intel Arc A770"), None, "not in the table");
    }

    #[test]
    fn no_speed_is_claimed_for_a_model_that_does_not_fit_the_card() {
        let spilling = Model {
            weights_gb: 12.0,
            active_params_b: 24.0,
        };
        let plan = plan(spilling, laptop());
        assert_ne!(plan.on_gpu, Some(true));
        assert_eq!(
            speed_estimate(&plan, "NVIDIA GeForce GTX 1050 Ti"),
            None,
            "a partial offload is not predictable from a specification sheet"
        );
    }

    #[test]
    fn a_model_that_does_fit_gets_a_figure_in_the_right_order_of_magnitude() {
        let model = Model {
            weights_gb: 5.0,
            active_params_b: 8.0,
        };
        let plan = plan(model, desktop());
        let speed =
            speed_estimate(&plan, "AMD Radeon RX 6800 XT").expect("it fits on a card in the table");
        // 512 GB/s over 5GB of weights, derated: tens of tokens a second.
        assert!(
            (20.0..=200.0).contains(&speed),
            "a plausible figure, not a wild one: {speed}"
        );
    }

    #[test]
    fn weights_come_from_the_file_where_there_is_one_and_the_quant_where_there_is_not() {
        let from_file = Model::from_file(4_500_000_000, 7.0);
        assert!((from_file.weights_gb - 4.5).abs() < 1e-9);

        let from_quant = Model::from_quant(7.0, 7.0, "Q4_K_M").expect("a known quantisation");
        assert!((from_quant.weights_gb - 7.0 * 0.62).abs() < 1e-9);

        assert_eq!(
            Model::from_quant(7.0, 7.0, "Q9_WHAT"),
            None,
            "an unknown quantisation estimates nothing"
        );
    }
}
