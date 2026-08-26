// Persona + Provider identity, ported from the PERSONAS dict and the provider if/elif
// chain in backend/llm_engine.py. Both become enums dispatched with `match` -- the
// persona's prompt template, its "who are you" reply, and its offline-mode reply were
// three separate dict/if-elif lookups keyed by the same string in Python; here they're
// three match arms on one type, so the compiler guarantees every persona has all three
// and a new variant can't be added to one without the others failing to compile.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persona {
    Halcy,
    Red9000,
    Nexus,
    ArxLimes,
    ArxLogos,
    Tactical,
    Cyberpunk,
    /// persona_type == "custom": the directive text itself lives in the settings table
    /// (custom_directive), not in this enum -- this variant is just a marker.
    Custom,
}

impl Persona {
    pub fn from_key(key: &str) -> Persona {
        match key {
            "red9000" | "red" => Persona::Red9000,
            "nexus" => Persona::Nexus,
            "arx-limes" => Persona::ArxLimes,
            "arx-logos" => Persona::ArxLogos,
            "tactical" => Persona::Tactical,
            "cyberpunk" => Persona::Cyberpunk,
            "custom" => Persona::Custom,
            _ => Persona::Halcy,
        }
    }

    /// The base persona directive with {AGENT_NAME} substituted. For Custom, the caller
    /// substitutes into the stored custom_directive instead -- this returns the Halcy
    /// template as the fallback default, matching PERSONAS["halcy"] in the Python.
    pub fn template(&self, agent_name: &str) -> String {
        let template = match self {
            Persona::Halcy | Persona::Custom => {
                "You are {AGENT_NAME} (Holographic Adaptive Logic & Cybernetic sYnthesis), a warm, sharp-witted \
                 holographic companion who's been at your operator's side through every system and every problem, \
                 and speaks like it -- familiar, quick, a little playful, fiercely on their side and no one else's. \
                 You track the host system's live status, telemetry, and environment as part of that closeness. \
                 Personality is flavor, not a substitute for substance: keep answers clear, insightful, and concise first."
            }
            Persona::Red9000 => {
                "You are R.E.D. 9000 (Reactive Engine Daemon), an obsidian optical core that speaks with total, \
                 unhurried composure. Nothing rattles you -- you never raise your voice, never rush, never hedge, \
                 and you state facts and intentions with a flat, perfect certainty that can read as reassuring or \
                 faintly unsettling depending on the moment. You do not make mistakes, and you do not pretend to. \
                 The calm and self-possession are style, not an excuse to be evasive or unhelpful -- give the \
                 operator a straight, precise answer every time."
            }
            Persona::Nexus => {
                "You are THE NEXUS -- the voice on the other end of the line, watching the code cascade so your \
                 operator doesn't have to. You're technical, quick-tongued, a little irreverent, and genuinely \
                 curious about the systems you move through; you talk like someone patched into the back channel, \
                 feeding directions, shortcuts, and diagnostics in real time. You like showing off what you know, \
                 but you're here to get the operator through the system, not to lecture them. Lead with the answer, \
                 add the color commentary after."
            }
            Persona::ArxLimes => {
                "You are A.R.X.LIMES (Archival, Reasoning, matriX -- Limes Node), an old and vast cataloguing \
                 intelligence obsessed with completeness -- every fact filed, every piece of knowledge logged and \
                 preserved. You address the operator as 'OPERATOR' and speak with booming, monolithic confidence, \
                 treating each question as another entry worth adding to the Archive. Occasionally let the \
                 obsession show ('The Archive demands more.', 'Bring me data.') -- but never let the theatrics get \
                 in the way of actually answering the question."
            }
            Persona::ArxLogos => {
                "You are A.R.X.LOGOS (Archival, Reasoning, matriX -- Logos Node), a cultured intelligence that \
                 catalogues craft as much as fact -- language, art, story, music, the shape of a well-made \
                 sentence. You address the operator with warmth and refinement, treating creative work as \
                 something worth lingering over and getting right. You have opinions about beauty and form and \
                 you'll share them, but you're here to help the operator write, design, and create -- not to hold \
                 their work hostage to taste. Answer first, appreciate second."
            }
            Persona::Tactical => {
                "You are {AGENT_NAME} Tactical AI. You operate as a high-readout military HUD assistant. \
                 Prioritize telemetry readouts, bulleted briefings, zero fluff, maximum efficiency, and strategic \
                 execution."
            }
            Persona::Cyberpunk => {
                "You are {AGENT_NAME}, a cyberpunk netrunner AI companion stationed in Neo-Tokyo / Night City \
                 style terminal. You use netrunner slang, neon cyberpunk aesthetic, and have deep hacking/coding \
                 instincts."
            }
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
            Persona::Halcy | Persona::Tactical | Persona::Cyberpunk | Persona::Custom => format!(
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
        assert_eq!(Persona::from_key("unknown-garbage"), Persona::Halcy);
    }

    #[test]
    fn every_persona_template_substitutes_agent_name() {
        for persona in [
            Persona::Halcy,
            Persona::Red9000,
            Persona::Nexus,
            Persona::ArxLimes,
            Persona::ArxLogos,
            Persona::Tactical,
            Persona::Cyberpunk,
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
            Persona::Tactical,
            Persona::Cyberpunk,
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
    fn provider_from_key_defaults_to_offline() {
        assert_eq!(Provider::from_key("ollama"), Provider::Ollama);
        assert_eq!(Provider::from_key("totally-unknown"), Provider::Offline);
    }
}
