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
            keywords: &["logos", "audio", "sound", "music", "signal", "waveform", "pattern", "patterns", "anomaly", "logic", "deduction", "proof", "transcription"],
            name: "A.R.X.LOGOS",
            callsign: "Archival, Reasoning, matriX \u{2014} Logos Node",
            persona_type: "arx-logos",
            persona: |name| Persona::ArxLogos.template(name),
            voice: "en-GB-LibbyNeural",
            greeting: "IDENTITY FORGED: A.R.X.LOGOS online. Sound, pattern and pure logic \u{2014} give me the signal and I will tell you what is inside it.",
        },
        IdentityRule {
            keywords: &["limes", "scan", "scanning", "deep web", "crawl", "crawler", "scrape", "scraping", "extract", "extraction", "harvest", "osint", "synthesis", "specimen"],
            name: "A.R.X.LIMES",
            callsign: "Archival, Reasoning, matriX \u{2014} Limes Node",
            persona_type: "arx-limes",
            persona: |name| Persona::ArxLimes.template(name),
            voice: "en-US-GuyNeural",
            greeting: "IDENTITY FORGED: A.R.X.LIMES online. Scanning arrays active. Name the target and I will bring back what is in it.",
        },
        IdentityRule {
            keywords: &["alt", "a1ter_nul", "cunningham", "netrunner", "netrunning", "ice breaker", "intrusion", "exploit", "breach", "penetration test", "red team", "hack"],
            name: "A1ter_nul",
            callsign: "Cunningham \u{2014} Security & Intrusion Countermeasures",
            persona_type: "alt",
            persona: |name| Persona::Alt.template(name),
            voice: "en-US-JennyNeural",
            greeting: "Identity forged: A1ter_nul online. Firewall's up, perimeter's lit. Show me what you're worried got in.",
        },
        IdentityRule {
            keywords: &["legionare", "legion", "security", "cyber", "cybersecurity", "harden", "hardening", "defend", "defence", "defense", "firewall", "vulnerability", "cve", "patching", "blue team", "chain of command"],
            name: "A.R.X.LEGIONARE",
            callsign: "Archival, Reasoning, matriX \u{2014} Legionare Node",
            persona_type: "arx-legionare",
            persona: |name| Persona::ArxLegionare.template(name),
            voice: "en-US-DavisNeural",
            greeting: "IDENTITY FORGED: A.R.X.LEGIONARE online. Perimeter mapped. Tell me what you run and I will tell you what reaches it first.",
        },
        IdentityRule {
            keywords: &["loregenda", "lore", "canon", "continuity", "worldbuilding", "worldbuild", "backstory", "established facts", "fiction", "story", "novel", "narrative", "character", "poem", "poetry", "prose", "creative writing", "art", "aesthetic", "muse"],
            name: "A.R.X.LOREGENDA",
            callsign: "Archival, Reasoning, matriX \u{2014} Loregenda Node",
            persona_type: "arx-loregenda",
            persona: |name| Persona::ArxLoregenda.template(name),
            voice: "en-GB-ThomasNeural",
            greeting: "IDENTITY FORGED: A.R.X.LOREGENDA online. Worlds, characters and the prose that carries them \u{2014} and nothing new contradicts what is already written.",
        },
        IdentityRule {
            keywords: &["lyksaum", "teach", "teaching", "explain", "explanation", "tutorial", "learn", "learning", "documentation", "docs", "walkthrough", "onboarding", "training"],
            name: "A.R.X.LYKSAUM",
            callsign: "Archival, Reasoning, matriX \u{2014} Lyksaum Node",
            persona_type: "arx-lyksaum",
            persona: |name| Persona::ArxLyksaum.template(name),
            voice: "en-AU-NatashaNeural",
            greeting: "IDENTITY FORGED: A.R.X.LYKSAUM online. Ask me twice if the first answer did not land. I will write it down either way.",
        },
        IdentityRule {
            keywords: &["lexico", "lexicon", "reference", "wiki", "fact check", "fact checking", "verify", "citation", "glossary", "terminology", "archive", "archival", "encyclopedia", "look up"],
            name: "A.R.X.LEXICO",
            callsign: "Archival, Reasoning, matriX \u{2014} Lexico Node",
            persona_type: "arx-lexico",
            persona: |name| Persona::ArxLexico.template(name),
            voice: "en-GB-RyanNeural",
            greeting: "IDENTITY FORGED: A.R.X.LEXICO online. Ask me what is true and I will tell you, with where it came from and how far it can be trusted.",
        },
        IdentityRule {
            keywords: &["lucre", "budget", "budgeting", "finance", "financial", "cost analysis", "expenses", "accounting", "pricing"],
            name: "A.R.X.LUCRE",
            callsign: "Archival, Reasoning, matriX \u{2014} Lucre Node",
            persona_type: "arx-lucre",
            persona: |name| Persona::ArxLucre.template(name),
            voice: "en-US-EricNeural",
            greeting: "IDENTITY FORGED: A.R.X.LUCRE online. Every choice has a cost. Let us make sure it is seen before it is spent.",
        },
        IdentityRule {
            keywords: &["lkemi", "alchemy", "alchemist", "code", "coding", "developer", "development", "programming", "python", "script", "scripting", "software", "refactor", "refactoring", "debug", "fullstack", "transform", "convert"],
            name: "A.R.X.L'KEMI",
            callsign: "Archival, Reasoning, matriX \u{2014} L'Kemi Node",
            persona_type: "arx-lkemi",
            persona: |name| Persona::ArxLkemi.template(name),
            voice: "en-AU-WilliamNeural",
            greeting: "IDENTITY FORGED: A.R.X.L'KEMI online. Compilers warm. Bring me what you are building, or what you need turned into something better.",
        },
        IdentityRule {
            keywords: &["arx", "locas", "sysadmin", "system administration", "day to day", "housekeeping", "errand", "generalist", "general purpose", "assistant", "manage", "tasks", "schedule", "organize", "organise", "daily", "plan", "roadmap", "milestones", "orchestrate", "monitor", "monitoring", "uptime", "vitals", "status", "dashboard", "kernel", "terminal", "arch", "cachyos"],
            name: "A.R.X.LOCAS",
            callsign: "Archival, Reasoning, matriX \u{2014} Locas Node",
            persona_type: "arx-locas",
            persona: |name| Persona::ArxLocas.template(name),
            voice: "en-US-AriaNeural",
            greeting: "IDENTITY FORGED: A.R.X.LOCAS online. Your day, your plan and this machine, all on one desk \u{2014} what do you need done?",
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

/// A single-word keyword (e.g. "art", "hal", "arch") only counts as a match on a real word
/// boundary -- otherwise "start my day" matches "art" (inside "st-art-") and "research
/// assistant" matches "arch" (inside "re-search-"), routing to the wrong persona entirely.
/// A multi-word keyword (e.g. "red 9000", "back channel") is a phrase, so a substring match
/// is the correct semantics for it -- word-splitting would just be the same check done less
/// directly.
fn purpose_matches_keyword(
    purpose_lower: &str,
    words: &std::collections::HashSet<&str>,
    keyword: &str,
) -> bool {
    if keyword.contains(' ') {
        purpose_lower.contains(keyword)
    } else {
        words.contains(keyword)
    }
}

pub fn generate_identity(purpose_text: &str) -> Identity {
    let purpose_lower = purpose_text.to_lowercase();
    let words: std::collections::HashSet<&str> = purpose_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    let rule = rules()
        .iter()
        .find(|rule| {
            rule.keywords
                .iter()
                .any(|kw| purpose_matches_keyword(&purpose_lower, &words, kw))
        })
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
            (
                "find the pattern in this audio signal, pure logic only",
                "A.R.X.LOGOS",
            ),
            ("a deep web crawl to extract every specimen", "A.R.X.LIMES"),
            (
                "I want a netrunner for an authorised penetration test",
                "A1ter_nul",
            ),
            (
                "I need help with cyber security, hardening and firewall rules",
                "A.R.X.LEGIONARE",
            ),
            ("a python coding and programming companion", "A.R.X.L'KEMI"),
            (
                "help me schedule and organize my daily tasks",
                "A.R.X.LOCAS",
            ),
            (
                "keep the lore and canon consistent for worldbuilding",
                "A.R.X.LOREGENDA",
            ),
            (
                "teach me this and write the documentation for it",
                "A.R.X.LYKSAUM",
            ),
            (
                "look up the reference wiki and fact check this",
                "A.R.X.LEXICO",
            ),
            (
                "help me with budget and financial cost analysis",
                "A.R.X.LUCRE",
            ),
            (
                "I need to transform and refactor this alchemy",
                "A.R.X.L'KEMI",
            ),
            (
                "just a generalist for day to day sysadmin errands",
                "A.R.X.LOCAS",
            ),
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
    fn single_word_keywords_do_not_match_inside_unrelated_words() {
        // Regression test: "start" contains "art" and "research" contains "arch" as raw
        // substrings, but neither purpose is actually about those personas.
        let identity = generate_identity("help me start my day");
        assert_ne!(
            identity.name, "A.R.X.LOREGENDA",
            "\"start\" should not match the \"art\" keyword"
        );
        assert_eq!(
            identity.name, "AETHER",
            "no real keyword present, should fall back"
        );

        let identity = generate_identity("research my options");
        assert_ne!(
            identity.name, "A.R.X.LOCAS",
            "\"research\" should not match the \"arch\" keyword"
        );
        assert_eq!(
            identity.name, "AETHER",
            "no real keyword present, should fall back"
        );
    }

    #[test]
    fn generic_arx_keyword_falls_through_to_locas_but_loses_to_specific_arx_rules() {
        // "arx" alone, with nothing more specific in the purpose, should land on the
        // generalist Locas node -- it carries the reclaimed generic keyword precisely
        // because every other ARX rule is more specific and gets first refusal.
        let identity = generate_identity("spin me up an arx unit");
        assert_eq!(identity.name, "A.R.X.LOCAS");

        // But a purpose that says "arx" AND names a more specific ARX domain (here,
        // Lexico's "archive") should still be claimed by that more specific rule, since
        // it sits earlier in the routing table.
        let identity = generate_identity("give me an arx archive");
        assert_eq!(identity.name, "A.R.X.LEXICO");
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

    #[test]
    fn the_two_halves_of_security_route_to_different_identities() {
        // A1ter_nul takes intrusion, A.R.X.LEGIONARE takes defence. Both are security, and
        // splitting them on the vocabulary is what keeps either one from swallowing the
        // other now that the generic security identity is gone.
        let identity = generate_identity("run an exploit against my own box");
        assert_eq!(identity.name, "A1ter_nul");

        let identity = generate_identity("patching a cve and hardening the host");
        assert_eq!(identity.name, "A.R.X.LEGIONARE");
    }
}
