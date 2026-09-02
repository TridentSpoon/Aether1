// One HTTP call per provider, ported from llm_engine.py's _call_ollama /
// _call_openai_compatible / _call_gemini / _call_anthropic. Each provider's wire format
// gets its own small request/response structs (derive Serialize/Deserialize) instead of
// building/indexing a loose JSON dict by hand -- a typo in a field name is a compile
// error here instead of a silent KeyError at runtime.
//
// Every provider comes in two flavors: call_* returns the whole reply at once, stream_*
// feeds it to a Sink token by token as it arrives. The two share their payload builders
// (*_payload below), so the streaming path can't drift from the blocking one -- only the
// `stream` flag, the URL, and how the response body is read differ. llm/mod.rs always
// tries stream_* first and falls back to call_* if the stream fails before any output,
// which is what keeps providers/models that can't stream working.

use std::io::{BufRead, BufReader};

use serde::{Deserialize, Serialize};

use super::db::Message;
use super::persona::Provider;

/// Receives reply text as it arrives. Called with each delta, never with the accumulated
/// text -- the caller is responsible for joining them if it wants the whole thing.
pub type Sink<'a> = &'a mut dyn FnMut(&str);

/// Blocking calls wait for the whole reply, so this bounds the entire request.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
/// A stream is alive as long as tokens keep arriving, and a long answer from a local model
/// on modest hardware legitimately outlasts the blocking timeout -- applying 45s to the
/// whole stream would cut off exactly the slow replies streaming exists to make bearable.
const STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

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

// ------------------------------------------------------------- stream plumbing

/// Reads a response body line by line, handing each to `on_line`. Returning Ok(false) from
/// `on_line` stops early (an explicit end-of-stream marker) without treating it as an error.
fn for_each_line(
    response: ureq::http::Response<ureq::Body>,
    mut on_line: impl FnMut(&str) -> Result<bool, String>,
) -> Result<(), String> {
    let reader = BufReader::new(response.into_body().into_reader());
    for line in reader.lines() {
        let line = line.map_err(|e| format!("stream read failed: {e}"))?;
        if !on_line(&line)? {
            break;
        }
    }
    Ok(())
}

/// The JSON payload of one Server-Sent Event line, or None for the blank lines, comments,
/// and `event:` lines that carry no data. `[DONE]` (the OpenAI-style terminator) is
/// reported as an explicit end rather than as a payload.
enum SseLine<'a> {
    Data(&'a str),
    End,
    Ignore,
}

fn sse_line(line: &str) -> SseLine<'_> {
    match line.strip_prefix("data:") {
        Some(data) => {
            let data = data.trim();
            if data.is_empty() {
                SseLine::Ignore
            } else if data == "[DONE]" {
                SseLine::End
            } else {
                SseLine::Data(data)
            }
        }
        None => SseLine::Ignore,
    }
}

/// A stream that produced no text at all is a failure even when the transport succeeded --
/// an empty reply would otherwise reach the operator as silence.
fn finish(accumulated: String) -> Result<String, String> {
    let trimmed = accumulated.trim().to_string();
    if trimmed.is_empty() {
        Err("stream ended without any content".to_string())
    } else {
        Ok(trimmed)
    }
}

// ---------------------------------------------------------------- Ollama

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Deserialize, Default)]
struct OllamaResponse {
    #[serde(default)]
    response: String,
}

fn ollama_payload(model: &str, ctx: &ChatContext, stream: bool) -> OllamaRequest {
    let mut prompt_body = format!("{}\n\n", ctx.system_prompt);
    for msg in ctx.history {
        prompt_body.push_str(&format!("{}: {}\n", msg.sender.to_uppercase(), msg.text));
    }
    prompt_body.push_str(&format!("USER: {}\n{}:", ctx.prompt, ctx.agent_name));

    OllamaRequest {
        model: if model.is_empty() { "llama3" } else { model }.to_string(),
        prompt: prompt_body,
        stream,
        options: OllamaOptions {
            temperature: 0.7,
            top_p: 0.9,
        },
    }
}

fn ollama_url(endpoint: &str) -> String {
    format!("{}/api/generate", endpoint.trim_end_matches('/'))
}

pub fn call_ollama(endpoint: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let response: OllamaResponse = ureq::post(&ollama_url(endpoint))
        .config()
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .send_json(ollama_payload(model, ctx, false))
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    Ok(response.response.trim().to_string())
}

/// Ollama streams newline-delimited JSON rather than SSE: one object per token, each with
/// the incremental text in `response`.
pub fn stream_ollama(
    endpoint: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<String, String> {
    let response = ureq::post(&ollama_url(endpoint))
        .config()
        .timeout_global(Some(STREAM_TIMEOUT))
        .build()
        .send_json(ollama_payload(model, ctx, true))
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    for_each_line(response, |line| {
        let line = line.trim();
        if line.is_empty() {
            return Ok(true);
        }
        let chunk: OllamaResponse = serde_json::from_str(line)
            .map_err(|e| format!("unparseable chunk from Ollama: {e}"))?;
        if !chunk.response.is_empty() {
            full.push_str(&chunk.response);
            sink(&chunk.response);
        }
        Ok(true)
    })?;

    finish(full)
}

// -------------------------------------------------- OpenAI-compatible (OpenAI, Groq, LM Studio)

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    stream: bool,
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

#[derive(Deserialize)]
struct OpenAiStreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    #[serde(default)]
    delta: Option<OpenAiStreamDelta>,
}

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
}

/// The per-provider base URL and the model to use when none is configured.
fn openai_defaults(provider: Provider, endpoint: &str) -> (&str, &str) {
    match provider {
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
    }
}

fn openai_payload(
    provider: Provider,
    endpoint: &str,
    model: &str,
    ctx: &ChatContext,
    stream: bool,
) -> (String, OpenAiRequest) {
    let (base_endpoint, default_model) = openai_defaults(provider, endpoint);
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

    (
        url,
        OpenAiRequest {
            model: model.to_string(),
            messages,
            temperature: 0.7,
            stream,
        },
    )
}

fn openai_request(
    url: &str,
    api_key: &str,
    timeout: std::time::Duration,
) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
    let mut request = ureq::post(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .header("Content-Type", "application/json");
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {api_key}"));
    }
    request
}

pub fn call_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
) -> Result<String, String> {
    let (url, payload) = openai_payload(provider, endpoint, model, ctx, false);

    let response: OpenAiResponse = openai_request(&url, api_key, CALL_TIMEOUT)
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

pub fn stream_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<String, String> {
    let (url, payload) = openai_payload(provider, endpoint, model, ctx, true);

    let response = openai_request(&url, api_key, STREAM_TIMEOUT)
        .send_json(&payload)
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let chunk: OpenAiStreamChunk =
            serde_json::from_str(data).map_err(|e| format!("unparseable chunk: {e}"))?;
        for choice in chunk.choices {
            if let Some(text) = choice.delta.and_then(|d| d.content) {
                if !text.is_empty() {
                    full.push_str(&text);
                    sink(&text);
                }
            }
        }
        Ok(true)
    })?;

    finish(full)
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

fn gemini_model(model: &str) -> &str {
    if model.is_empty() {
        "gemini-2.0-flash"
    } else {
        model
    }
}

fn gemini_payload(ctx: &ChatContext) -> GeminiRequest {
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

    GeminiRequest { contents }
}

pub fn call_gemini(api_key: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={api_key}",
        gemini_model(model)
    );

    let response: GeminiResponse = ureq::post(&url)
        .config()
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .send_json(gemini_payload(ctx))
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

/// `alt=sse` asks for Server-Sent Events; without it streamGenerateContent returns a
/// single JSON array, which can't be consumed incrementally.
pub fn stream_gemini(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={api_key}",
        gemini_model(model)
    );

    let response = ureq::post(&url)
        .config()
        .timeout_global(Some(STREAM_TIMEOUT))
        .build()
        .send_json(gemini_payload(ctx))
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let chunk: GeminiResponse =
            serde_json::from_str(data).map_err(|e| format!("unparseable chunk: {e}"))?;
        for candidate in chunk.candidates {
            for part in candidate.content.parts {
                if !part.text.is_empty() {
                    full.push_str(&part.text);
                    sink(&part.text);
                }
            }
        }
        Ok(true)
    })?;

    finish(full)
}

// ---------------------------------------------------------------- Anthropic

/// Deliberately generous: 1024 truncated long answers mid-sentence, and the streaming
/// path removes the request-timeout pressure that made a small cap tempting.
const ANTHROPIC_MAX_TOKENS: u32 = 8192;

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    system: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

/// One event from the Messages API stream. Only content_block_delta carries text; the
/// other event types (message_start, content_block_start/stop, message_delta,
/// message_stop) are structure and are ignored here.
#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    error: Option<AnthropicError>,
}

#[derive(Deserialize)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicError {
    #[serde(default)]
    message: String,
}

fn anthropic_payload(model: &str, ctx: &ChatContext, stream: bool) -> AnthropicRequest {
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

    AnthropicRequest {
        model: if model.is_empty() {
            "claude-opus-5"
        } else {
            model
        }
        .to_string(),
        system: ctx.system_prompt.to_string(),
        messages,
        max_tokens: ANTHROPIC_MAX_TOKENS,
        stream,
    }
}

fn anthropic_request(
    api_key: &str,
    timeout: std::time::Duration,
) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
    ureq::post("https://api.anthropic.com/v1/messages")
        .config()
        .timeout_global(Some(timeout))
        .build()
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
}

pub fn call_anthropic(api_key: &str, model: &str, ctx: &ChatContext) -> Result<String, String> {
    let response: AnthropicResponse = anthropic_request(api_key, CALL_TIMEOUT)
        .send_json(anthropic_payload(model, ctx, false))
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

pub fn stream_anthropic(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<String, String> {
    let response = anthropic_request(api_key, STREAM_TIMEOUT)
        .send_json(anthropic_payload(model, ctx, true))
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let event: AnthropicStreamEvent =
            serde_json::from_str(data).map_err(|e| format!("unparseable event: {e}"))?;

        match event.event_type.as_str() {
            // An error arrives as a stream event with HTTP 200 already sent, so it has to
            // be surfaced from in here rather than from the status code.
            "error" => {
                let message = event
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "unspecified error".to_string());
                Err(format!("Anthropic stream error: {message}"))
            }
            "content_block_delta" => {
                if let Some(text) = event.delta.and_then(|d| d.text) {
                    if !text.is_empty() {
                        full.push_str(&text);
                        sink(&text);
                    }
                }
                Ok(true)
            }
            "message_stop" => Ok(false),
            _ => Ok(true),
        }
    })?;

    finish(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_lines_are_classified() {
        assert!(matches!(
            sse_line("data: {\"a\":1}"),
            SseLine::Data("{\"a\":1}")
        ));
        assert!(matches!(sse_line("data: [DONE]"), SseLine::End));
        assert!(matches!(sse_line("event: message_stop"), SseLine::Ignore));
        assert!(matches!(sse_line(""), SseLine::Ignore));
        assert!(matches!(sse_line(": keep-alive"), SseLine::Ignore));
    }

    #[test]
    fn an_empty_stream_is_an_error_not_an_empty_reply() {
        assert!(finish("   \n ".to_string()).is_err());
        assert_eq!(finish(" hello ".to_string()).unwrap(), "hello");
    }

    #[test]
    fn payloads_match_between_blocking_and_streaming() {
        // The whole point of sharing the builders: only the stream flag differs.
        let ctx = ChatContext {
            system_prompt: "be helpful",
            history: &[],
            prompt: "hello",
            agent_name: "HALCY",
        };
        let blocking = serde_json::to_value(anthropic_payload("m", &ctx, false)).unwrap();
        let streaming = serde_json::to_value(anthropic_payload("m", &ctx, true)).unwrap();
        assert_eq!(blocking["messages"], streaming["messages"]);
        assert_eq!(blocking["system"], streaming["system"]);
        assert_eq!(blocking["stream"], serde_json::json!(false));
        assert_eq!(streaming["stream"], serde_json::json!(true));
    }

    #[test]
    fn an_unset_model_falls_back_per_provider() {
        let ctx = ChatContext {
            system_prompt: "s",
            history: &[],
            prompt: "p",
            agent_name: "a",
        };
        assert_eq!(ollama_payload("", &ctx, true).model, "llama3");
        let (url, payload) = openai_payload(Provider::Groq, "", "", &ctx, true);
        assert_eq!(payload.model, "llama-3.3-70b-versatile");
        assert!(url.starts_with("https://api.groq.com/"));
        assert_eq!(gemini_model(""), "gemini-2.0-flash");
    }
}
