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
/// Turns a provider failure into something the operator can act on.
///
/// Every one of these used to reach the HUD as "Neural link dropped" with the reason
/// printed to stderr -- which, for an app launched from a Start Menu shortcut, is nowhere.
/// A rejected key, a mistyped model name and an unplugged network all looked identical,
/// and the honest reading of that screen was "the API does nothing". The cause is what
/// makes the difference between a mystery and a two-second fix, so it goes on screen.
///
/// The raw text is kept for anything unrecognised: a wrong guess dressed up as a
/// diagnosis is worse than the error itself.
pub fn explain_failure(provider: Provider, endpoint: &str, raw: &str) -> String {
    let lowered = raw.to_lowercase();
    let local = matches!(provider, Provider::Ollama | Provider::LmStudio);

    // Where the operator would go to fix it, named as it appears in Settings.
    let key_field = "API KEY in Settings";

    if lowered.contains("401") || lowered.contains("unauthorized") {
        return format!("{provider} rejected the API key -- check the {key_field}.");
    }
    if lowered.contains("403") || lowered.contains("forbidden") {
        if lowered.contains("proxy") {
            return format!(
                "a proxy on this network refused the connection to {provider}, so the request never arrived."
            );
        }
        return format!(
            "{provider} refused the API key -- it may lack access to this model, or be for the wrong account."
        );
    }
    if lowered.contains("404") || lowered.contains("not found") {
        return format!(
            "{provider} has no model by that name -- check MODEL NAME / ID in Settings."
        );
    }
    if lowered.contains("429") || lowered.contains("too many requests") {
        return format!(
            "{provider} is rate-limiting this key -- wait a moment, or check its quota."
        );
    }
    if lowered.contains("500")
        || lowered.contains("502")
        || lowered.contains("503")
        || lowered.contains("504")
    {
        return format!("{provider} itself returned an error -- nothing here is misconfigured.");
    }
    if lowered.contains("connection refused") || lowered.contains("connectionrefused") {
        if local {
            return format!(
                "nothing is listening at {endpoint} -- is {provider} running, and is that the right port?"
            );
        }
        return format!("the connection to {provider} was refused.");
    }
    if lowered.contains("dns")
        || lowered.contains("resolve")
        || lowered.contains("no such host")
        || lowered.contains("name or service not known")
    {
        return format!(
            "{provider}'s address could not be resolved -- this machine looks offline."
        );
    }
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return if local {
            format!("{provider} did not answer in time -- a large model on modest hardware can take longer than the timeout.")
        } else {
            format!("{provider} did not answer in time.")
        };
    }
    if lowered.contains("certificate") || lowered.contains("tls") || lowered.contains("ssl") {
        return format!(
            "the secure connection to {provider} could not be verified -- often a proxy or antivirus intercepting HTTPS."
        );
    }
    if lowered.contains("api key") || lowered.contains("api_key") {
        return format!(
            "{provider} would not accept the request -- check the {key_field}. ({raw})"
        );
    }

    format!("{provider} could not be reached: {raw}")
}

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

/// What a provider said about the tokens it actually used.
///
/// Every provider here returns real counts, and until now every one of them was thrown
/// away and replaced with a four-characters-per-token guess. The guess is still the
/// fallback -- a server that reports nothing has to produce *some* number -- but it is
/// never presented as if it were measured. See `Metered`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Nanoseconds the model spent generating, when the provider measures it -- Ollama
    /// does. Wall-clock time includes loading the model off disk and waiting in a queue,
    /// so tokens-per-second computed from it understates a fast model on its first reply
    /// and says nothing useful about the hardware.
    pub eval_nanos: Option<u64>,
}

/// A finished call: the text, and whatever the provider was willing to say about its own
/// token use. `usage` is None when it said nothing.
#[derive(Debug, Clone, Default)]
pub struct Completion {
    pub text: String,
    pub usage: Option<TokenUsage>,
}

/// A stream that produced no text at all is a failure even when the transport succeeded --
/// an empty reply would otherwise reach the operator as silence.
fn finish(accumulated: String, usage: Option<TokenUsage>) -> Result<Completion, String> {
    let trimmed = accumulated.trim().to_string();
    if trimmed.is_empty() {
        Err("stream ended without any content".to_string())
    } else {
        Ok(Completion {
            text: trimmed,
            usage,
        })
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
    /// Only on the final object of a stream (and on a non-streamed reply), which is why
    /// each is an Option rather than a count that happens to be zero most of the time.
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
    #[serde(default)]
    eval_duration: Option<u64>,
}

impl OllamaResponse {
    fn usage(&self) -> Option<TokenUsage> {
        self.eval_count.map(|completion_tokens| TokenUsage {
            prompt_tokens: self.prompt_eval_count.unwrap_or(0),
            completion_tokens,
            eval_nanos: self.eval_duration,
        })
    }
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

pub fn call_ollama(endpoint: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
    let response: OllamaResponse = ureq::post(&ollama_url(endpoint))
        .config()
        .timeout_global(Some(CALL_TIMEOUT))
        .build()
        .send_json(ollama_payload(model, ctx, false))
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    Ok(Completion {
        text: response.response.trim().to_string(),
        usage: response.usage(),
    })
}

/// Ollama streams newline-delimited JSON rather than SSE: one object per token, each with
/// the incremental text in `response`.
pub fn stream_ollama(
    endpoint: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
    let response = ureq::post(&ollama_url(endpoint))
        .config()
        .timeout_global(Some(STREAM_TIMEOUT))
        .build()
        .send_json(ollama_payload(model, ctx, true))
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    let mut usage = None;
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
        // The counts ride on the last object of the stream, so this overwrites nothing.
        if let Some(reported) = chunk.usage() {
            usage = Some(reported);
        }
        Ok(true)
    })?;

    finish(full, usage)
}

// -------------------------------------------------- OpenAI-compatible (OpenAI, Groq, LM Studio)

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Asks an OpenAI-compatible server to append a usage object to the stream. Sent only to
/// the two providers that document supporting it (see `openai_payload`): an older or
/// homegrown local server can reject a request outright for carrying a field it does not
/// know, and breaking a working local setup to gain a token count would be a poor trade --
/// especially since for a local model the number that matters is throughput, which is
/// measured here regardless.
#[derive(Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAiStreamOptions>,
}

#[derive(Deserialize, Default, Clone, Copy)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

impl From<OpenAiUsage> for TokenUsage {
    fn from(u: OpenAiUsage) -> TokenUsage {
        TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            eval_nanos: None,
        }
    }
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
    #[serde(default)]
    usage: Option<OpenAiUsage>,
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
    /// Arrives on a final chunk of its own, with an empty `choices`, and only when
    /// `stream_options.include_usage` was sent.
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

/// What a local model can actually do, as the server itself describes it.
///
/// Every field is optional because every field is something a particular server may not
/// report, and the point of this whole change is to stop presenting a guess as a fact. A
/// missing number is shown as missing rather than filled in with a plausible default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LocalCapability {
    /// How many tokens of context the model was built with.
    pub context_tokens: Option<u64>,
    /// "7B", "13B" -- the parameter count as the server spells it.
    pub parameter_size: Option<String>,
    /// "Q4_K_M" and friends. Together with the parameter size this is most of what decides
    /// whether a reply will be fast and shallow or slow and careful.
    pub quantization: Option<String>,
}

impl LocalCapability {
    fn is_empty(&self) -> bool {
        self == &LocalCapability::default()
    }
}

#[derive(Serialize)]
struct OllamaShowRequest<'a> {
    model: &'a str,
}

/// Asks Ollama what a model is: its context length, parameter count and quantisation.
///
/// The context length lives under an architecture-prefixed key -- `llama.context_length`,
/// `qwen2.context_length`, `gemma3.context_length` -- so rather than keeping a list of
/// architectures that would be out of date by the next release, any key ending in
/// `.context_length` counts.
///
/// Returns None when the server is not Ollama, is not running, or says nothing useful.
/// Deliberately short-timeout: this is decoration on a telemetry panel, and it must never
/// be the reason a reply feels slow.
pub fn ollama_capability(endpoint: &str, model: &str) -> Option<LocalCapability> {
    let url = format!("{}/api/show", endpoint.trim_end_matches('/'));
    let body: serde_json::Value = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_millis(2500)))
        .build()
        .send_json(OllamaShowRequest { model })
        .ok()?
        .into_body()
        .read_json()
        .ok()?;

    let details = body.get("details");
    let capability = LocalCapability {
        context_tokens: body
            .get("model_info")
            .and_then(|info| info.as_object())
            .and_then(|info| {
                info.iter()
                    .find(|(key, _)| key.ends_with(".context_length"))
                    .and_then(|(_, value)| value.as_u64())
            }),
        parameter_size: details
            .and_then(|d| d.get("parameter_size"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        quantization: details
            .and_then(|d| d.get("quantization_level"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };

    // Nothing known is not the same as a model with no properties; say so with None.
    (!capability.is_empty()).then_some(capability)
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
            stream_options: if stream && reports_stream_usage(provider) {
                Some(OpenAiStreamOptions {
                    include_usage: true,
                })
            } else {
                None
            },
        },
    )
}

/// Whether this provider documents `stream_options.include_usage`. LM Studio is left out
/// deliberately: it is the local one, it is the one most likely to be an older build or a
/// look-alike server, and a rejected request there costs a working setup.
fn reports_stream_usage(provider: Provider) -> bool {
    matches!(provider, Provider::OpenAi | Provider::Groq)
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
) -> Result<Completion, String> {
    let (url, payload) = openai_payload(provider, endpoint, model, ctx, false);

    let response: OpenAiResponse = openai_request(&url, api_key, CALL_TIMEOUT)
        .send_json(&payload)
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    let usage = response.usage.map(TokenUsage::from);
    response
        .choices
        .into_iter()
        .next()
        .map(|c| Completion {
            text: c.message.content.trim().to_string(),
            usage,
        })
        .ok_or_else(|| "empty choices in response".to_string())
}

pub fn stream_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
    let (url, payload) = openai_payload(provider, endpoint, model, ctx, true);

    let response = openai_request(&url, api_key, STREAM_TIMEOUT)
        .send_json(&payload)
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    let mut usage = None;
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let chunk: OpenAiStreamChunk =
            serde_json::from_str(data).map_err(|e| format!("unparseable chunk: {e}"))?;
        if let Some(reported) = chunk.usage {
            usage = Some(TokenUsage::from(reported));
        }
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

    finish(full, usage)
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

#[derive(Deserialize, Default, Clone, Copy)]
struct GeminiUsage {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: u64,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_token_count: u64,
}

impl From<GeminiUsage> for TokenUsage {
    fn from(u: GeminiUsage) -> TokenUsage {
        TokenUsage {
            prompt_tokens: u.prompt_token_count,
            completion_tokens: u.candidates_token_count,
            eval_nanos: None,
        }
    }
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    /// Gemini repeats this on every streamed chunk with a running total, so the last one
    /// seen is the final count.
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsage>,
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

pub fn call_gemini(api_key: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
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

    let usage = response.usage_metadata.map(TokenUsage::from);
    response
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| Completion {
            text: p.text.trim().to_string(),
            usage,
        })
        .ok_or_else(|| "empty candidates in response".to_string())
}

/// `alt=sse` asks for Server-Sent Events; without it streamGenerateContent returns a
/// single JSON array, which can't be consumed incrementally.
pub fn stream_gemini(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
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
    let mut usage = None;
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let chunk: GeminiResponse =
            serde_json::from_str(data).map_err(|e| format!("unparseable chunk: {e}"))?;
        if let Some(reported) = chunk.usage_metadata {
            usage = Some(TokenUsage::from(reported));
        }
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

    finish(full, usage)
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
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize, Default, Clone, Copy)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// The envelope on a `message_start` event, which is the only place the input count
/// appears -- the running output count arrives separately on `message_delta`.
#[derive(Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
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
    /// message_start only.
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
    /// message_delta only, carrying the output count so far.
    #[serde(default)]
    usage: Option<AnthropicUsage>,
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

pub fn call_anthropic(api_key: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
    let response: AnthropicResponse = anthropic_request(api_key, CALL_TIMEOUT)
        .send_json(anthropic_payload(model, ctx, false))
        .map_err(|e| e.to_string())?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;

    let usage = response.usage.map(|u| TokenUsage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        eval_nanos: None,
    });
    response
        .content
        .into_iter()
        .next()
        .map(|c| Completion {
            text: c.text.trim().to_string(),
            usage,
        })
        .ok_or_else(|| "empty content in response".to_string())
}

pub fn stream_anthropic(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
    let response = anthropic_request(api_key, STREAM_TIMEOUT)
        .send_json(anthropic_payload(model, ctx, true))
        .map_err(|e| e.to_string())?;

    let mut full = String::new();
    // Input arrives once at the top, output as a running total near the end, so the two
    // halves are accumulated separately and combined when the stream closes.
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut reported = false;
    for_each_line(response, |line| {
        let data = match sse_line(line) {
            SseLine::Data(data) => data,
            SseLine::End => return Ok(false),
            SseLine::Ignore => return Ok(true),
        };
        let event: AnthropicStreamEvent =
            serde_json::from_str(data).map_err(|e| format!("unparseable event: {e}"))?;

        if let Some(usage) = event.message.as_ref().and_then(|m| m.usage).or(event.usage) {
            reported = true;
            if usage.input_tokens > 0 {
                prompt_tokens = usage.input_tokens;
            }
            if usage.output_tokens > 0 {
                completion_tokens = usage.output_tokens;
            }
        }

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

    let usage = reported.then_some(TokenUsage {
        prompt_tokens,
        completion_tokens,
        eval_nanos: None,
    });
    finish(full, usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each provider reports its token counts in a different shape and a different place.
    /// These parse the real ones -- the counts were being thrown away and replaced with a
    /// four-characters-per-token guess, and nothing would have noticed if a rename upstream
    /// silently turned them all back into None.
    #[test]
    fn ollama_reports_its_counts_and_its_own_generation_time() {
        let final_chunk: OllamaResponse = serde_json::from_str(
            r#"{"response":"","done":true,"prompt_eval_count":41,"eval_count":128,"eval_duration":2000000000}"#,
        )
        .unwrap();
        let usage = final_chunk
            .usage()
            .expect("the last chunk carries the counts");
        assert_eq!(usage.prompt_tokens, 41);
        assert_eq!(usage.completion_tokens, 128);
        assert_eq!(usage.eval_nanos, Some(2_000_000_000));

        // Every chunk before the last one has no counts, and must not claim any.
        let mid: OllamaResponse =
            serde_json::from_str(r#"{"response":"hi","done":false}"#).unwrap();
        assert_eq!(mid.usage(), None);
    }

    #[test]
    fn the_openai_shape_reports_usage_on_a_chunk_with_no_choices() {
        let chunk: OpenAiStreamChunk = serde_json::from_str(
            r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":34}}"#,
        )
        .unwrap();
        let usage = TokenUsage::from(chunk.usage.expect("the usage chunk"));
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 34);
        assert_eq!(usage.eval_nanos, None, "only Ollama measures its own time");

        let ordinary: OpenAiStreamChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert!(ordinary.usage.is_none());
    }

    /// include_usage goes only to the two providers that document it. LM Studio is the
    /// local one and the one most likely to be an older build or a look-alike, and a
    /// rejected request there would cost a working setup to gain a number that matters
    /// less locally than throughput does.
    #[test]
    fn only_the_providers_that_document_it_are_asked_for_stream_usage() {
        assert!(reports_stream_usage(Provider::OpenAi));
        assert!(reports_stream_usage(Provider::Groq));
        assert!(!reports_stream_usage(Provider::LmStudio));

        let ctx = ChatContext {
            system_prompt: "s",
            history: &[],
            prompt: "p",
            agent_name: "A1",
        };
        let (_, lm) = openai_payload(
            Provider::LmStudio,
            "http://localhost:1234/v1",
            "m",
            &ctx,
            true,
        );
        assert!(
            serde_json::to_string(&lm)
                .unwrap()
                .find("stream_options")
                .is_none(),
            "LM Studio must not be sent a field it may reject"
        );
        let (_, openai) = openai_payload(Provider::OpenAi, "", "gpt-4o-mini", &ctx, true);
        assert!(serde_json::to_string(&openai)
            .unwrap()
            .contains("include_usage"));
        // Never on the non-streaming path, where the field means nothing.
        let (_, blocking) = openai_payload(Provider::OpenAi, "", "gpt-4o-mini", &ctx, false);
        assert!(!serde_json::to_string(&blocking)
            .unwrap()
            .contains("stream_options"));
    }

    #[test]
    fn gemini_reports_a_running_total_on_every_chunk() {
        let chunk: GeminiResponse = serde_json::from_str(
            r#"{"candidates":[],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":19,"totalTokenCount":26}}"#,
        )
        .unwrap();
        let usage = TokenUsage::from(chunk.usage_metadata.expect("usageMetadata"));
        assert_eq!(usage.prompt_tokens, 7);
        assert_eq!(usage.completion_tokens, 19);
    }

    /// Anthropic splits it: the input count arrives once inside message_start, the output
    /// count as a running total on message_delta. Reading only one of the two events would
    /// report half the turn.
    #[test]
    fn anthropic_splits_its_counts_across_two_events() {
        let start: AnthropicStreamEvent = serde_json::from_str(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":55,"output_tokens":1}}}"#,
        )
        .unwrap();
        assert_eq!(
            start.message.and_then(|m| m.usage).map(|u| u.input_tokens),
            Some(55)
        );

        let delta: AnthropicStreamEvent =
            serde_json::from_str(r#"{"type":"message_delta","usage":{"output_tokens":203}}"#)
                .unwrap();
        assert_eq!(delta.usage.map(|u| u.output_tokens), Some(203));

        // A plain text delta carries neither, and must not be read as zero counts.
        let text: AnthropicStreamEvent =
            serde_json::from_str(r#"{"type":"content_block_delta","delta":{"text":"hello"}}"#)
                .unwrap();
        assert!(text.message.is_none() && text.usage.is_none());
    }

    /// A model with no reported properties is None, not a LocalCapability full of blanks:
    /// the panel has to be able to tell "nothing said" from "said nothing useful".
    #[test]
    fn an_empty_capability_is_no_capability() {
        assert!(LocalCapability::default().is_empty());
        assert!(!LocalCapability {
            context_tokens: Some(8192),
            ..Default::default()
        }
        .is_empty());
    }

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
        assert!(finish("   \n ".to_string(), None).is_err());
        assert_eq!(finish(" hello ".to_string(), None).unwrap().text, "hello");
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

#[cfg(test)]
mod failure_tests {
    use super::*;

    /// The four causes that actually happen, each named in terms of the thing the
    /// operator would have to change.
    #[test]
    fn a_rejected_key_says_so_and_points_at_the_setting() {
        let msg = explain_failure(Provider::OpenAi, "", "http status: 401");
        assert!(msg.contains("rejected the API key"), "{msg}");
        assert!(msg.contains("API KEY in Settings"), "{msg}");
    }

    #[test]
    fn a_wrong_model_name_is_not_reported_as_a_key_problem() {
        let msg = explain_failure(Provider::Anthropic, "", "http status: 404");
        assert!(msg.contains("no model by that name"), "{msg}");
        assert!(msg.contains("MODEL NAME"), "{msg}");
    }

    /// The local providers are the ones where "nothing is listening" is the likely
    /// story, and the endpoint is the thing to check -- so it appears in the message.
    #[test]
    fn a_dead_local_server_names_the_endpoint() {
        let msg = explain_failure(
            Provider::LmStudio,
            "http://localhost:1234/v1",
            "io error: Connection refused (os error 111)",
        );
        assert!(msg.contains("http://localhost:1234/v1"), "{msg}");
        assert!(msg.contains("lmstudio"), "{msg}");
    }

    #[test]
    fn a_proxy_refusal_is_not_blamed_on_the_key() {
        let msg = explain_failure(
            Provider::OpenAi,
            "",
            "CONNECT proxy failed: proxy server responded 403/403",
        );
        assert!(msg.contains("proxy"), "{msg}");
        assert!(!msg.contains("API key"), "{msg}");
    }

    /// Anything unrecognised keeps the original text rather than being guessed at.
    #[test]
    fn an_unknown_failure_is_passed_through_verbatim() {
        let msg = explain_failure(Provider::Gemini, "", "something nobody has seen before");
        assert!(msg.contains("something nobody has seen before"), "{msg}");
    }
}
