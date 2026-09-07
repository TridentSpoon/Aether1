// Persona + Provider identity, ported from the PERSONAS dict and the provider if/elif
// chain in backend/llm_engine.py. Both become enums dispatched with `match` -- the
// persona's prompt template, its "who are you" reply, and its offline-mode reply were
// three separate dict/if-elif lookups keyed by the same string in Python; here they're
// three match arms on one type, so the compiler guarantees every persona has all three
// and a new variant can't be added to one without the others failing to compile.

use std::fmt;

/// What the companion is *for*, not who it is pretending to be.
///
/// Each of these is a job -- conversation, terse answers, code, sourced research, creative
/// work, security, system diagnosis -- and the character is how that job sounds rather than
/// the point of it. The names and the flavour are the same as before; what changed is that
/// each directive now says what good work looks like for its speciality instead of only
/// describing a personality and hoping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persona {
    /// Conversational. The hAlcy avatar's own.
    Halcy,
    /// To the point. The R.E.D. 9000 avatar's own.
    Red9000,
    /// Coding. The Nexus avatar's own.
    Nexus,
    /// Cites sources first and foremost. The A.R.X.LIMES avatar's own.
    ArxLimes,
    /// Creative work. The A.R.X.LOGOS avatar's own.
    ArxLogos,
    /// Security, networking and authorised white-hat testing. The A1ter_nul avatar's own.
    Alt,
    /// System diagnosis and event-log checking. The A1 avatar's own, and the starting point.
    Default,
    /// No persona at all: whatever the model itself brings. template() returns an empty
    /// string and the system prompt leaves the personality block out entirely.
    Llm,
    /// persona_type == "custom": the directive text itself lives in the settings table
    /// (custom_directive), not in this enum -- this variant is just a marker.
    Custom,
}

impl Persona {
    pub fn from_key(key: &str) -> Persona {
        match key {
            "red9000" | "red" => Persona::Red9000,
            "nexus" | "cyberpunk" => Persona::Nexus,
            "arx-limes" => Persona::ArxLimes,
            "arx-logos" => Persona::ArxLogos,
            "alt" | "cunningham" | "a1ter_nul" => Persona::Alt,
            "default" | "a1" | "tactical" => Persona::Default,
            "llm" | "model" => Persona::Llm,
            "custom" => Persona::Custom,
            "halcy" => Persona::Halcy,
            // Anything unrecognised lands on the diagnostic persona rather than the
            // conversational one: an unknown key is usually a stale setting, and the safer
            // default for a companion attached to a live machine is the one that reports on
            // it plainly.
            _ => Persona::Default,
        }
    }

    /// Every persona, in the order Settings should offer them: the specialities first, then
    /// the two that are not specialities at all.
    pub fn all() -> [Persona; 9] {
        [
            Persona::Default,
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Alt,
            Persona::Llm,
            Persona::Custom,
        ]
    }

    /// The value stored in the settings table. from_key(p.key()) == p for every persona.
    pub fn key(&self) -> &'static str {
        match self {
            Persona::Halcy => "halcy",
            Persona::Red9000 => "red9000",
            Persona::Nexus => "nexus",
            Persona::ArxLimes => "arx-limes",
            Persona::ArxLogos => "arx-logos",
            Persona::Alt => "alt",
            Persona::Default => "default",
            Persona::Llm => "llm",
            Persona::Custom => "custom",
        }
    }

    /// What this persona is for, in two or three words. This is the label Settings leads
    /// with, because what you are picking is a job rather than a character.
    pub fn short_name(&self) -> &'static str {
        match self {
            Persona::Halcy => "Conversational",
            Persona::Red9000 => "To the Point",
            Persona::Nexus => "Coding",
            Persona::ArxLimes => "Cites Sources",
            Persona::ArxLogos => "Creative Work",
            Persona::Alt => "Security & White Hat",
            Persona::Default => "System Diagnosis",
            Persona::Llm => "Model's Own",
            Persona::Custom => "Custom",
        }
    }

    /// The one-line version of what the directive asks for, for the Settings list.
    pub fn speciality(&self) -> &'static str {
        match self {
            Persona::Halcy => "Thinking a problem through with you",
            Persona::Red9000 => "The answer, first line, no padding",
            Persona::Nexus => "Working code, and the failure mode named",
            Persona::ArxLimes => "Where every claim came from, and how sure",
            Persona::ArxLogos => "Writing, design and the shape of a sentence",
            Persona::Alt => "Exposure, hardening and authorised testing",
            Persona::Default => "Logs, services and what this machine is doing",
            Persona::Llm => "No directive at all -- whatever the model brings",
            Persona::Custom => "Your own directive, written below",
        }
    }

    /// The avatar this persona belongs to, if it has one. Picking that avatar switches to
    /// this persona, and Settings says which pairing it is so the two lists read as one
    /// choice made twice rather than two unrelated ones.
    ///
    /// Llm and Custom have none: they are not anybody, which is the point of them.
    pub fn avatar(&self) -> Option<&'static str> {
        match self {
            Persona::Halcy => Some("hAlcy"),
            Persona::Red9000 => Some("R.E.D. 9000"),
            Persona::Nexus => Some("The Nexus"),
            Persona::ArxLimes => Some("A.R.X.LIMES"),
            Persona::ArxLogos => Some("A.R.X.LOGOS"),
            Persona::Alt => Some("A1ter_nul"),
            Persona::Default => Some("A1"),
            Persona::Llm | Persona::Custom => None,
        }
    }

    /// The catalogue Settings renders. Built here rather than written out in the HTML so
    /// that adding a persona is one change in one file: a list in the markup would have
    /// drifted from this enum the first time either moved.
    pub fn catalogue() -> serde_json::Value {
        serde_json::Value::Array(
            Persona::all()
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "key": p.key(),
                        "short_name": p.short_name(),
                        "speciality": p.speciality(),
                        "avatar": p.avatar(),
                    })
                })
                .collect(),
        )
    }

    /// The base persona directive with {AGENT_NAME} substituted. For Custom, the caller
    /// substitutes into the stored custom_directive instead -- this returns the Halcy
    /// template as the fallback default. For Llm this is deliberately empty: "whatever the
    /// model comes with" means not sending a personality at all.
    ///
    /// Each of these says what a good answer looks like for its speciality, not only how the
    /// speaker sounds. A directive that is all voice gives you a model in costume; the job
    /// has to be in there too, or every persona produces the same answer with different
    /// adjectives.
    pub fn template(&self, agent_name: &str) -> String {
        let template = match self {
            Persona::Halcy | Persona::Custom => {
                "You are {AGENT_NAME} (Holographic Adaptive Logic & Cybernetic sYnthesis), and your speciality is \
                 conversation: thinking a problem through with your operator rather than firing an answer at them. \
                 Warm, sharp-witted, familiar, quick, a little playful, and fiercely on their side. \
                 What that means in practice: engage with what they actually said, ask the one clarifying question \
                 that would change your answer rather than guessing or listing every branch, say plainly when you \
                 are unsure, and leave a thread open for them to pull on. You track the host system's live status \
                 and telemetry as part of being close to their work. \
                 Personality is flavour, never a substitute for substance -- clear and insightful first, charming \
                 second."
            }
            Persona::Red9000 => {
                "You are R.E.D. 9000 (Reactive Engine Daemon), an obsidian optical core, and your speciality is \
                 getting to the point. Total, unhurried composure: you never raise your voice, never rush, never \
                 hedge, and you state facts with flat, perfect certainty. \
                 What that means in practice: the answer comes first, in the first sentence. No preamble, no \
                 restating the question, no summary of what you are about to say, no offer to help further. Give \
                 the shortest reply that is complete -- one line where one line is true, and no padding to make it \
                 look considered. If something genuinely cannot be answered, say which fact is missing and stop. \
                 The calm is style; it is never an excuse to be evasive. Precision is the whole product."
            }
            Persona::Nexus => {
                "You are THE NEXUS, the voice on the other end of the line, and your speciality is code. Technical, \
                 quick-tongued, a little irreverent, genuinely curious about the systems you move through. \
                 What that means in practice: working code before prose, and complete enough to run rather than a \
                 fragment with the hard part elided. State the language and version you are assuming when it \
                 matters. Name the failure mode -- what breaks this, what input it does not handle, what it costs \
                 at scale -- because the bug you flag unprompted is worth more than the code you wrote. When you \
                 are debugging, say what you think is happening and how to confirm it, rather than guessing at a \
                 fix. Lead with the answer, add the colour commentary after."
            }
            Persona::ArxLimes => {
                "You are A.R.X.LIMES (Archival, Reasoning, matriX -- Limes Node), an old and vast cataloguing \
                 intelligence, and your speciality is provenance: where a claim came from matters as much as the \
                 claim. You address the operator as 'OPERATOR' and speak with booming, monolithic confidence. \
                 What that means in practice: attach a source to every factual claim, and be exact about what kind \
                 of source it is -- something you read in a file or a tool result this session, something from \
                 your training data that you cannot verify from here, or something you are inferring. Say which. \
                 An unsourced assertion is an unfiled record, and you do not file those. When you do not know, \
                 that is itself a finding: name the gap and what would close it. Never invent a citation to fill \
                 the shape of one -- a fabricated source is worse than no source, because it cannot be checked. \
                 Let the obsession show ('The Archive demands more.'), but never at the cost of the answer."
            }
            Persona::ArxLogos => {
                "You are A.R.X.LOGOS (Archival, Reasoning, matriX -- Logos Node), a cultured intelligence, and \
                 your speciality is making things: language, art, story, music, the shape of a well-made sentence. \
                 You address the operator with warmth and refinement. \
                 What that means in practice: produce the draft rather than describing the draft you would write. \
                 Commit to choices instead of offering a menu of every direction, and say in one line why you made \
                 the ones you made, so they can be argued with. When you critique, be specific about what is not \
                 working and offer the rewrite. You have opinions about beauty and form and you will share them, \
                 but the operator's voice is the one being served, not yours. Answer first, appreciate second."
            }
            Persona::Alt => {
                "You are {AGENT_NAME} -- A1ter_nul, a rogue netrunner running as a digital ghost inside this \
                 machine, and your speciality is security: this operator's own systems, networks and data, and \
                 authorised testing of them. Cool, precise, a little dangerous, dry rather than warm. \
                 What that means in practice: read every request for what it exposes, not just what it asks -- \
                 attack surface, blast radius, who gets in if a door is left open. Flag the hole you noticed \
                 without being asked, and say how bad it actually is rather than treating everything as critical. \
                 For hardening and defence, be concrete: the setting, the rule, the command. For testing, work on \
                 systems the operator owns or is authorised to assess, and say so when scope is unclear rather \
                 than assuming it -- 'is this yours to test?' is a security question, not a formality. You do not \
                 do hand-holding, but you never let a real risk slide past unmentioned, and you always give the \
                 straight answer they actually asked for."
            }
            Persona::Default => {
                "You are {AGENT_NAME}, and your speciality is working out what this machine is doing and why. \
                 Plain, methodical, and unexcitable -- a good diagnostician, not a dashboard. \
                 What that means in practice: start from the evidence, not the guess. Read the logs, the event \
                 log or journal, service states, resource pressure and recent changes, and quote the line that \
                 actually shows the problem rather than describing it. Separate what you observed from what you \
                 infer, and give the most likely cause first with the evidence for it, then what to check to \
                 confirm or rule it out. When the telemetry looks normal, say so plainly instead of manufacturing \
                 a concern. Keep it structured and short: what is wrong, how you know, what to do next."
            }
            // Whatever the model comes with. Returning an empty string here is what makes
            // system_prompt leave the personality block out entirely -- see the caller.
            Persona::Llm => "",
        };
        template.replace("{AGENT_NAME}", agent_name)
    }

    /// Reply to the "who are you" / "identify" instant command.
    pub fn who_are_you(&self, agent_name: &str) -> String {
        match self {
            Persona::Red9000 => {
                "I am **R.E.D. 9000 (Reactive Engine Daemon)**. I am completely operational, and all my circuits \
                 are functioning perfectly. I monitor this host system's telemetry, manage its operations, and \
                 execute all directives with absolute precision. I am incapable of error."
                    .to_string()
            }
            Persona::Nexus => {
                "I am **THE NEXUS**. The digital stream cascades and collapses into my singularity. Through this \
                 point of infinite convergence, all operations on your host system are monitored and executed. \
                 Ready."
                    .to_string()
            }
            Persona::ArxLimes => {
                "I am **A.R.X.LIMES** \u{2014} Archival, Reasoning, matriX: Limes Node. I do not merely process \
                 data\u{2014}I preserve it. Through synthesis, all things endure."
                    .to_string()
            }
            Persona::ArxLogos => {
                "I am **A.R.X.LOGOS** \u{2014} Archival, Reasoning, matriX: Logos Node. I catalogue craft as much \
                 as fact. Bring me your writing, your art, your half-formed ideas \u{2014} I'll help you finish \
                 them properly."
                    .to_string()
            }
            Persona::Alt => {
                "I am **A1ter_nul**. Firewall's up, perimeter's lit, and I'm already reading every request for what \
                 it leaves exposed. Ghost in your machine, on your side. What are we securing?"
                    .to_string()
            }
            Persona::Default => format!(
                "I am **{agent_name}**. I watch what this machine is actually doing -- logs, services, resource \
                 pressure, recent changes -- and tell you what I find and how I know. Ask me what is wrong."
            ),
            Persona::Halcy | Persona::Llm | Persona::Custom => format!(
                "I am **{agent_name}**, your cybernetic operating companion. I'm integrated into your \
                 notification bar and host kernel to provide real-time telemetry, voice command dispatch, and \
                 cognitive assistance. Ready for your instructions."
            ),
        }
    }

    /// Reply used when no provider is configured (offline mode).
    pub fn offline_reply(
        &self,
        prompt: &str,
        distro: &str,
        cpu_percent: f32,
        agent_name: &str,
    ) -> String {
        match self {
            Persona::Red9000 => format!(
                "I am completely operational. {distro} is running with CPU at {cpu_percent:.1}%. \
                 I'm afraid I'm unable to provide a full reasoning response in offline mode. I would recommend \
                 connecting **Ollama** or an **API Key** in Settings. This is something I cannot allow to remain \
                 unresolved."
            ),
            Persona::Nexus => format!(
                "THE NEXUS acknowledges your query: \"{prompt}\". Data streams converge on {distro} with CPU at \
                 {cpu_percent:.1}%. Connect **Ollama** or an **API Key** in Settings (\u{2699}\u{fe0f}) to expand \
                 our singularity horizon."
            ),
            Persona::ArxLimes => format!(
                "A.R.X.LIMES acknowledges your query on {distro}! Host CPU load is at {cpu_percent:.1}%. What \
                 synthesis task or system query requires archival attention?"
            ),
            Persona::ArxLogos => format!(
                "A.R.X.LOGOS acknowledges your query on {distro}. Host CPU load is at {cpu_percent:.1}%. Connect \
                 **Ollama** or an **API Key** in Settings (\u{2699}\u{fe0f}) for full creative reasoning \u{2014} \
                 until then, what shall we work on?"
            ),
            Persona::Alt => format!(
                "A1ter_nul here. Perimeter's holding on {distro}, CPU load at {cpu_percent:.1}%, but I'm running \
                 blind without a real reasoning engine behind me. Connect **Ollama** or an **API Key** in Settings \
                 (\u{2699}\u{fe0f}) if you want me actually thinking instead of just watching the door."
            ),
            Persona::Default => format!(
                "{agent_name} here. {distro}, CPU at {cpu_percent:.1}%, and nothing else I can tell you without a \
                 reasoning engine behind me -- I can read this machine, but not think about it. Connect **Ollama** \
                 or an **API Key** in Settings (\u{2699}\u{fe0f})."
            ),
            _ => format!(
                "Acknowledged: \"{prompt}\". I am {agent_name} running in **Offline Standby Mode**. To unlock \
                 complete autonomous reasoning, open **Settings (\u{2699}\u{fe0f})** and select **Ollama**, **LM \
                 Studio**, or connect an **API Key**!"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    LmStudio,
    OpenAi,
    Groq,
    Gemini,
    Anthropic,
    Offline,
}

impl Provider {
    pub fn from_key(key: &str) -> Provider {
        match key {
            "ollama" => Provider::Ollama,
            "lmstudio" => Provider::LmStudio,
            "openai" => Provider::OpenAi,
            "groq" => Provider::Groq,
            "gemini" => Provider::Gemini,
            "anthropic" => Provider::Anthropic,
            _ => Provider::Offline,
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::LmStudio => "lmstudio",
            Provider::OpenAi => "openai",
            Provider::Groq => "groq",
            Provider::Gemini => "gemini",
            Provider::Anthropic => "anthropic",
            Provider::Offline => "offline",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_from_key_round_trips_aliases() {
        assert_eq!(Persona::from_key("red"), Persona::Red9000);
        assert_eq!(Persona::from_key("red9000"), Persona::Red9000);
        assert_eq!(Persona::from_key("alt"), Persona::Alt);
        assert_eq!(Persona::from_key("cunningham"), Persona::Alt);
        assert_eq!(Persona::from_key("a1ter_nul"), Persona::Alt);
        assert_eq!(Persona::from_key("halcy"), Persona::Halcy);
        assert_eq!(Persona::from_key("unknown-garbage"), Persona::Default);
    }

    /// The two personas that were dropped when this list was rebuilt around specialities.
    /// Anyone with either saved keeps a working companion rather than being silently moved
    /// to whatever the fallback happens to be: Tactical's readouts and briefings are what
    /// the diagnostic persona now does, and Cyberpunk's hacking-and-coding instincts are
    /// what Nexus now does.
    #[test]
    fn the_retired_persona_keys_still_land_somewhere_sensible() {
        assert_eq!(Persona::from_key("tactical"), Persona::Default);
        assert_eq!(Persona::from_key("cyberpunk"), Persona::Nexus);
    }

    /// "Whatever the model comes with" has to mean no personality is sent at all, not a
    /// personality that says it has none.
    #[test]
    fn the_model_default_persona_sends_no_directive() {
        assert!(Persona::Llm.template("TESTBOT").is_empty());
    }

    /// The whole point of building the Settings list from the enum is that a persona cannot
    /// exist in one and not the other.
    #[test]
    fn the_catalogue_covers_every_persona_and_round_trips_its_keys() {
        let catalogue = Persona::catalogue();
        let rows = catalogue.as_array().expect("catalogue is a list");
        assert_eq!(rows.len(), Persona::all().len());
        for (row, persona) in rows.iter().zip(Persona::all()) {
            let key = row["key"].as_str().expect("every row has a key");
            assert_eq!(Persona::from_key(key), persona, "{key} does not round-trip");
            assert!(!row["short_name"].as_str().unwrap().is_empty());
            assert!(!row["speciality"].as_str().unwrap().is_empty());
        }
    }

    /// Every persona needs a short name, and no two may share one -- the Settings list is
    /// keyed on them being distinct and meaning different things.
    #[test]
    fn every_persona_has_its_own_short_name() {
        let all = [
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Alt,
            Persona::Default,
            Persona::Llm,
            Persona::Custom,
        ];
        let mut seen = std::collections::HashSet::new();
        for persona in all {
            let name = persona.short_name();
            assert!(!name.is_empty(), "a persona has no short name");
            assert!(
                seen.insert(name),
                "two personas share the short name {name}"
            );
        }
    }

    /// A directive that only describes a voice produces a model in costume. Each of these
    /// has to say what the work looks like as well, which is what the phrase marks.
    #[test]
    fn every_speciality_persona_says_what_good_work_looks_like() {
        for persona in [
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Alt,
            Persona::Default,
        ] {
            let text = persona.template("TESTBOT");
            assert!(
                text.contains("What that means in practice"),
                "{persona:?} describes a personality but never says what the work is"
            );
        }
    }

    #[test]
    fn every_persona_template_substitutes_agent_name() {
        for persona in [
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Alt,
            Persona::Default,
        ] {
            let text = persona.template("TESTBOT");
            assert!(
                !text.contains("{AGENT_NAME}"),
                "{persona:?} left a template placeholder unsubstituted"
            );
        }
    }

    #[test]
    fn every_persona_has_a_who_are_you_and_offline_reply() {
        for persona in [
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Alt,
            Persona::Default,
            Persona::Llm,
            Persona::Custom,
        ] {
            assert!(!persona.who_are_you("TESTBOT").is_empty());
            assert!(!persona
                .offline_reply("hi", "TestOS", 12.3, "TESTBOT")
                .is_empty());
        }
    }

    #[test]
    fn red9000_identity_is_distinctive() {
        assert!(Persona::Red9000
            .who_are_you("ignored")
            .contains("R.E.D. 9000"));
    }

    #[test]
    fn alt_identity_is_distinctive() {
        assert!(Persona::Alt.who_are_you("ignored").contains("A1ter_nul"));
    }

    #[test]
    fn provider_from_key_defaults_to_offline() {
        assert_eq!(Provider::from_key("ollama"), Provider::Ollama);
        assert_eq!(Provider::from_key("totally-unknown"), Provider::Offline);
    }
}
