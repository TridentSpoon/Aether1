// One HTTP call per provider, ported from llm_engine.py's _call_ollama /
// _call_openai_compatible / _call_gemini / _call_anthropic. Each provider's wire format
// gets its own small request/response structs (derive Serialize/Deserialize) instead of
// building/indexing a loose JSON dict by hand -- a typo in a field name is a compile
// error here instead of a silent KeyError at runtime.

use serde::{Deserialize, Serialize};

use super::db::Message;
use super::persona::Provider;

pub struct ChatContext<'a> {
    pub system_prompt: &'a str,
    pub history: &'a [Message],
    pub prompt: &'a str,
    pub agent_name: &'a str,
}

fn role_for(sender: &str, model_role: &str) -> String {
    if sender == "user" {
        "user".to_string()
    } else {
        model_role.to_string()
    }
}

// ---------------------------------------------------------------- Ollama

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
}

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Deserialize, Default)]
struct OllamaResponse {
    #[serde(default)]
    response: String,
}

pub fn call_ollama(endpoint: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let url = format!("{}/api/generate", endpoint.trim_end_matches('/'));

    let mut prompt_body = format!("{}\n\n", ctx.system_prompt);
    for msg in ctx.history {
        prompt_body.push_str(&format!("{}: {}\n", msg.sender.to_uppercase(), msg.text));
    }
    prompt_body.push_str(&format!("USER: {}\n{}:", ctx.prompt, ctx.agent_name));

    let payload = OllamaRequest {
        model: if model.is_empty() { "llama3" } else { model },
        prompt: prompt_body,
        stream: false,
        options: OllamaOptions {
            temperature: 0.7,
            top_p: 0.9,
        },
    };

    let response: OllamaResponse = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .send_json(&payload)
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    Ok(response.response.trim().to_string())
}

// -------------------------------------------------- OpenAI-compatible (OpenAI, Groq, LM Studio)

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Deserialize)]
struct OpenAiChoiceMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

pub fn call_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
) -> Result<String, String> {
    let (base_endpoint, default_model) = match provider {
        Provider::OpenAi => ("https://api.openai.com/v1", "gpt-4o-mini"),
        Provider::Groq => ("https://api.groq.com/openai/v1", "llama-3.3-70b-versatile"),
        Provider::LmStudio => {
            let ep = if endpoint.is_empty() {
                "http://localhost:1234/v1"
            } else {
                endpoint
            };
            (ep, "local-model")
        }
        _ => (endpoint, "gpt-3.5-turbo"),
    };
    let model = if model.is_empty() {
        default_model
    } else {
        model
    };
    let url = format!("{}/chat/completions", base_endpoint.trim_end_matches('/'));

    let mut messages = vec![ChatMessage {
        role: "system".to_string(),
        content: ctx.system_prompt.to_string(),
    }];
    for msg in ctx.history {
        messages.push(ChatMessage {
            role: role_for(&msg.sender, "assistant"),
            content: msg.text.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: ctx.prompt.to_string(),
    });

    let payload = OpenAiRequest {
        model,
        messages,
        temperature: 0.7,
    };

    let mut request = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(45)))
        .build()
        .header("Content-Type", "application/json");
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {api_key}"));
    }

    let response: OpenAiResponse = request
        .send_json(&payload)
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| "empty choices in response".to_string())
}

// ---------------------------------------------------------------- Gemini

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiRespPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiRespContent {
    parts: Vec<GeminiRespPart>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiRespContent,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

pub fn call_gemini(api_key: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let model = if model.is_empty() {
        "gemini-2.0-flash"
    } else {
        model
    };
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}"
    );

    let mut contents = vec![
        GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart {
                text: format!("System Directive: {}", ctx.system_prompt),
            }],
        },
        GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart {
                text: format!(
                    "Directive acknowledged. {} systems operational. Ready.",
                    ctx.agent_name
                ),
            }],
        },
    ];
    for msg in ctx.history {
        contents.push(GeminiContent {
            role: role_for(&msg.sender, "model"),
            parts: vec![GeminiPart {
                text: msg.text.clone(),
            }],
        });
    }
    contents.push(GeminiContent {
        role: "user".to_string(),
        parts: vec![GeminiPart {
            text: ctx.prompt.to_string(),
        }],
    });

    let payload = GeminiRequest { contents };

    let response: GeminiResponse = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(45)))
        .build()
        .send_json(&payload)
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    response
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text.trim().to_string())
        .ok_or_else(|| "empty candidates in response".to_string())
}

// ---------------------------------------------------------------- Anthropic

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    system: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

pub fn call_anthropic(api_key: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let model = if model.is_empty() {
        "claude-3-5-sonnet-20241022"
    } else {
        model
    };
    let url = "https://api.anthropic.com/v1/messages";

    let mut messages = Vec::new();
    for msg in ctx.history {
        messages.push(ChatMessage {
            role: role_for(&msg.sender, "assistant"),
            content: msg.text.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: ctx.prompt.to_string(),
    });

    let payload = AnthropicRequest {
        model,
        system: ctx.system_prompt.to_string(),
        messages,
        max_tokens: 1024,
    };

    let response: AnthropicResponse = ureq::post(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(45)))
        .build()
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .send_json(&payload)
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    response
        .content
        .into_iter()
        .next()
        .map(|c| c.text.trim().to_string())
        .ok_or_else(|| "empty content in response".to_string())
}
