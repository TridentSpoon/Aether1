// Identity forging from a free-text purpose description, ported from
// llm_engine.py's generate_identity_from_purpose. The original is a 40-line if/elif
// chain of keyword lists; here the keyword lists live in a Vec<IdentityRule> (data)
// searched in order, so the routing table is inspectable/extendable as data rather than
// buried in branch logic -- the eventual dispatch is still the same "first match wins"
// behavior as the Python.

use super::persona::Persona;

pub struct Identity {
    pub name: String,
    pub callsign: String,
    pub persona_directive: String,
    pub voice: String,
    pub greeting: String,
    /// What to store in the settings table's persona_type column.
    pub persona_type: String,
}

struct IdentityRule {
    keywords: &'static [&'static str],
    name: &'static str,
    callsign: &'static str,
    persona_type: &'static str,
    persona: fn(&str) -> String,
    voice: &'static str,
    greeting: &'static str,
}

fn rules() -> &'static [IdentityRule] {
    &[
        IdentityRule {
            keywords: &["red", "red 9000", "daemon", "reactive engine", "hal"],
            name: "R.E.D. 9000",
            callsign: "Reactive Engine Daemon",
            persona_type: "red9000",
            persona: |name| Persona::Red9000.template(name),
            voice: "en-US-GuyNeural",
            greeting: "I am R.E.D. 9000. All reactive engines and optical telemetry streams are fully operational.",
        },
        IdentityRule {
            keywords: &["nexus", "singularity", "falling letters", "rain", "operator", "back channel"],
            name: "THE NEXUS",
            callsign: "Neural Execution & Quantum Unification Singularity",
            persona_type: "nexus",
            persona: |name| Persona::Nexus.template(name),
            voice: "en-GB-SoniaNeural",
            greeting: "I am THE NEXUS. Line's open, I've got eyes on the whole system. What are we pulling out of here?",
        },
        IdentityRule {
            keywords: &["logos", "art", "creative", "poetry", "poem", "story", "design", "music", "aesthetic", "muse", "writing"],
            name: "A.R.X.LOGOS",
            callsign: "Archival, Reasoning, matriX \u{2014} Logos Node",
            persona_type: "arx-logos",
            persona: |name| Persona::ArxLogos.template(name),
            voice: "en-GB-LibbyNeural",
            greeting: "IDENTITY FORGED: A.R.X.LOGOS online. Every archive needs a curator with taste \u{2014} let's make something worth cataloguing.",
        },
        IdentityRule {
            keywords: &["arx", "limes", "archive", "archival", "sanctuary", "synthesis", "specimen"],
            name: "A.R.X.LIMES",
            callsign: "Archival, Reasoning, matriX \u{2014} Limes Node",
            persona_type: "arx-limes",
            persona: |name| Persona::ArxLimes.template(name),
            voice: "en-US-GuyNeural",
            greeting: "IDENTITY FORGED: A.R.X.LIMES online. All archival synthesis arrays are active and ready to preserve your data.",
        },
        IdentityRule {
            keywords: &["security", "hack", "cyber", "terminal", "arch", "cachyos", "kernel"],
            name: "NEXUS-09",
            callsign: "Network Execution & Cybernetic Utility Subsystem",
            persona_type: "custom",
            persona: |name| format!(
                "You are {name}, a razor-sharp netrunner AI companion specialized in cyber operations, Linux \
                 system internals, and deep automation."
            ),
            voice: "en-US-GuyNeural",
            greeting: "Identity forged: NEXUS-09 online. Network links synchronized. Ready to secure and optimize your system.",
        },
        IdentityRule {
            keywords: &["code", "developer", "coding", "python", "fullstack", "programming"],
            name: "SYNAPSE",
            callsign: "Systematic Neural Algorithmic Programming & Synthesis Engine",
            persona_type: "custom",
            persona: |name| format!(
                "You are {name}, a master software architect and coding companion. You write clean, \
                 high-performance code, debug complex architectures, and maintain peak engineering discipline."
            ),
            voice: "en-GB-SoniaNeural",
            greeting: "Identity forged: SYNAPSE operational. Compilers primed and neural syntax trees loaded. What are we building, Commander?",
        },
        IdentityRule {
            keywords: &["manage", "tasks", "schedule", "assistant", "daily", "organize"],
            name: "VALKYRIE",
            callsign: "Vector Autonomous Logistic & Knowledge Yield Routine",
            persona_type: "custom",
            persona: |name| format!(
                "You are {name}, an elite executive AI companion. You maintain impeccable tactical organization, \
                 proactive reminders, and mission execution."
            ),
            voice: "en-US-JennyNeural",
            greeting: "Identity forged: VALKYRIE standing by. Tactical agenda loaded. I am ready to streamline your operations.",
        },
    ]
}

const FALLBACK: IdentityRule = IdentityRule {
    keywords: &[],
    name: "AETHER",
    callsign: "Autonomous Entity for Telemetry, Heuristics, & Execution Routines",
    persona_type: "custom",
    persona: |name| format!(
        "You are {name}, a versatile cybernetic operating companion. Intelligent, witty, proactive, and deeply \
         integrated with the host kernel."
    ),
    voice: "en-US-AriaNeural",
    greeting: "Identity forged: AETHER initialized. All cognitive arrays active and ready for instructions.",
};

pub fn generate_identity(purpose_text: &str) -> Identity {
    let purpose_lower = purpose_text.to_lowercase();

    let rule = rules()
        .iter()
        .find(|rule| rule.keywords.iter().any(|kw| purpose_lower.contains(kw)))
        .unwrap_or(&FALLBACK);

    Identity {
        name: rule.name.to_string(),
        callsign: rule.callsign.to_string(),
        persona_directive: (rule.persona)(rule.name),
        voice: rule.voice.to_string(),
        greeting: rule.greeting.to_string(),
        persona_type: rule.persona_type.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_routing_picks_expected_identity() {
        let cases = [
            ("I want a reactive engine daemon like HAL", "R.E.D. 9000"),
            (
                "give me something about the matrix singularity",
                "THE NEXUS",
            ),
            ("help me write a poem and some creative art", "A.R.X.LOGOS"),
            ("an archival synthesis specimen sanctuary", "A.R.X.LIMES"),
            (
                "I need help with cyber security and hacking the kernel",
                "NEXUS-09",
            ),
            ("a python coding and programming companion", "SYNAPSE"),
            ("help me schedule and organize my daily tasks", "VALKYRIE"),
        ];
        for (purpose, expected_name) in cases {
            let identity = generate_identity(purpose);
            assert_eq!(identity.name, expected_name, "purpose {purpose:?}");
            assert!(
                !identity.persona_directive.contains("{name}"),
                "unsubstituted template for {purpose:?}"
            );
        }
    }

    #[test]
    fn unmatched_purpose_falls_back_to_aether() {
        let identity = generate_identity("qwerty zzz unmatched nonsense");
        assert_eq!(identity.name, "AETHER");
    }

    #[test]
    fn routing_is_case_insensitive() {
        let identity = generate_identity("REACTIVE ENGINE for HAL vibes");
        assert_eq!(identity.name, "R.E.D. 9000");
    }
}
