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
use serde_json::{json, Value};

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

    // Where the operator would go to fix it, named as it appears in Settings. A gateway
    // has its own key field, and sending someone to the provider one -- which they may
    // have filled in correctly for something else -- wastes the only useful thing a
    // refusal carries.
    let key_field = if provider == Provider::Gateway {
        "GATEWAY KEY in Settings"
    } else {
        "API KEY in Settings"
    };

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
    // 400 is the provider saying the request was wrong rather than the key or the network,
    // and its body says which part. That body is worth more than anything that could be
    // written here, so this passes it through instead of paraphrasing it away.
    if lowered.contains("400") || lowered.contains("invalid_request") {
        return format!("{provider} rejected the request itself: {raw}");
    }
    if lowered.contains("404") || lowered.contains("not found") {
        if provider == Provider::Gateway {
            // The id is also how a gateway decides which provider answers, so an id it
            // does not recognise is not just a typo -- it is a model the operator may not
            // have been granted. Both fixes start at the list.
            return "the gateway does not route a model by that name -- pick one from its \
                    model list in Settings, or ask whoever administers it whether you have \
                    access to that model."
                .to_string();
        }
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
    /// Image attachments for this request only. They are never written into conversation
    /// history or the Memory vault.
    pub images: &'a [MediaAttachment],
    pub agent_name: &'a str,
    /// The tools to offer natively. Empty means send none -- either there are no tools, or
    /// this provider is driven by the text protocol instead and the catalogue is already in
    /// the system prompt.
    pub tools: &'a [crate::tools::ToolSchema],
    /// The tool rounds already taken *this turn*. Providers replay these in their own
    /// shape so the model sees what it asked for and what came back.
    pub exchanges: &'a [Exchange],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MediaAttachment {
    pub mime_type: String,
    pub data_base64: String,
}

/// One tool call as a provider asked for it.
///
/// `id` is the provider's own handle for the call, and the whole reason native tool calling
/// needs a type of its own rather than reusing the text protocol's ToolCall: every provider
/// requires the result to be sent back quoting that id, and a conversation with two calls in
/// flight has no other way to say which answer belongs to which question.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCall {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
}

/// What one tool produced, paired back to the call that asked for it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    pub id: String,
    pub tool: String,
    pub output: String,
}

/// A round of the within-turn tool conversation.
///
/// These live only for the length of a turn. The stored history stays plain text, because
/// a tool round is scaffolding for one answer rather than part of the conversation someone
/// would want to read back later -- and because giving the database a structured message
/// format to support this would be a migration in service of something nobody reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Exchange {
    /// What the model said, and what it asked to run.
    Called {
        text: String,
        calls: Vec<NativeCall>,
    },
    /// What those calls returned, in the same order.
    Returned(Vec<CallResult>),
}

fn role_for(sender: &str, model_role: &str) -> String {
    if sender == "user" {
        "user".to_string()
    } else {
        model_role.to_string()
    }
}

/// How much of a provider's error body to keep. Enough for the sentence that says what is
/// wrong, short of pasting a wall of JSON into the chat.
const MAX_ERROR_BODY: usize = 600;

/// Turns a non-2xx response into an error that carries what the provider actually said.
///
/// Every request here is built with `http_status_as_error(false)` so a 400 arrives as a
/// response with a readable body rather than as a bare status code. That body is the whole
/// point: "http status: 400" tells an operator nothing, while the same failure with the
/// body attached says `messages: roles must alternate between "user" and "assistant"`, or
/// names the model that does not exist, or the max_tokens that is too high for it. The
/// status stays in the string too, because `explain_failure` matches on it.
fn checked(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut response = response.map_err(|e| e.to_string())?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|_| String::new());
    let body = body.trim();
    if body.is_empty() {
        return Err(format!("http status: {}", status.as_u16()));
    }
    let mut shown = body.chars().take(MAX_ERROR_BODY).collect::<String>();
    if body.chars().count() > MAX_ERROR_BODY {
        shown.push_str("...");
    }
    Err(format!("http status: {} -- {shown}", status.as_u16()))
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
    /// Tools the model asked for through the provider's own tool-calling shape. Empty when
    /// it asked for none, and always empty for providers driven by the text protocol --
    /// those announce their calls inside `text`, which the caller parses instead.
    pub calls: Vec<NativeCall>,
}

/// A stream that produced no text at all is a failure even when the transport succeeded --
/// an empty reply would otherwise reach the operator as silence.
///
/// A turn that asked for a tool and said nothing else is not an empty reply, though -- it
/// is the most ordinary shape there is -- so the emptiness check applies only when there
/// are no calls either.
fn finish_with(
    accumulated: String,
    usage: Option<TokenUsage>,
    calls: Vec<NativeCall>,
) -> Result<Completion, String> {
    let trimmed = accumulated.trim().to_string();
    if trimmed.is_empty() && calls.is_empty() {
        return Err("stream ended without any content".to_string());
    }
    Ok(Completion {
        text: trimmed,
        usage,
        calls,
    })
}

// ---------------------------------------------------------------- Ollama
//
// Driven through `/api/chat`, not `/api/generate`.
//
// `/api/generate` takes one `prompt` string, so the only way to give it a conversation was
// to flatten the whole thing into a fake transcript -- "USER: ...\nA1: ..." -- and hand it
// over as a single user turn. Two things followed from that, and both were reported as the
// product not working:
//
//   * The model's own chat template was applied to the transcript rather than to the turns
//     inside it. A model trained to answer the last user message instead saw a block of
//     dialogue, and answered the way a model does when it is asked to continue a script:
//     echoing the question back, inventing the next few turns, losing the system prompt.
//   * There is no `tools` field on `/api/generate` at all. Every local model was therefore
//     on the fenced-text protocol whatever it was capable of, and a 7B asked to emit JSON
//     in a fence mostly does not -- so the companion could not search the web, and said so
//     in whatever words it liked.
//
// `/api/chat` takes `messages` with roles, `tools` in the same shape OpenAI uses, and
// `tool_calls` back on the assistant message. So a model Ollama reports as tool-capable
// gets real tool calling, and every model gets its own template applied to real turns.
//
// `num_ctx` is set explicitly for the same class of reason. Ollama's default context is
// 4096 tokens regardless of what the model can do, and AETHER1's system prompt (persona,
// telemetry, the code graph, the tool rules) plus eight messages of history goes past that
// on a long conversation. What falls off the front when it does is the system prompt --
// so the model stopped knowing it had tools, or who it was, for reasons nothing reported.

/// How much context to ask a local model for when its own limit is larger.
///
/// Context is memory: a 128K window on a 7B is several gigabytes of KV cache, and asking
/// for it on a 16GB card to hold a chat is how a model ends up swapping to CPU and taking
/// a minute a reply. 16K is comfortably more than a system prompt plus a tool round plus
/// the history AETHER1 sends, which is the whole requirement.
const LOCAL_CTX_CEILING: u64 = 16_384;

/// The floor, for a model that reports something tiny or reports nothing at all. Ollama's
/// own default is 4096 and that is the number that was overflowing.
const LOCAL_CTX_FLOOR: u64 = 8_192;

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    num_ctx: u64,
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDef>>,
}

/// One message in Ollama's chat shape. Images ride on the message they belong to as bare
/// base64, and a tool result is role `tool` with the name of the tool that produced it --
/// Ollama has no call ids, so the name is what pairs a result to its question.
#[derive(Serialize, Clone, Debug)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

impl OllamaMessage {
    fn plain(role: &str, content: String) -> OllamaMessage {
        OllamaMessage {
            role: role.to_string(),
            content,
            images: Vec::new(),
            tool_calls: None,
            tool_name: None,
        }
    }
}

/// A tool call as Ollama reports it. No id and no streamed fragments: the whole call
/// arrives in one object, and `arguments` is a JSON *object* rather than the string
/// OpenAI sends.
#[derive(Deserialize, Default, Clone)]
struct OllamaToolCall {
    #[serde(default)]
    function: OllamaCallFunction,
}

#[derive(Deserialize, Default, Clone)]
struct OllamaCallFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize, Default)]
struct OllamaChatMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Deserialize, Default)]
struct OllamaResponse {
    /// `/api/chat` answers under `message`; the field is optional because the final object
    /// of a stream carries only the counts.
    #[serde(default)]
    message: Option<OllamaChatMessage>,
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

    fn text(&self) -> &str {
        self.message
            .as_ref()
            .map(|m| m.content.as_str())
            .unwrap_or("")
    }

    /// The calls on this object, given positional ids. Ollama issues none, and the loop
    /// that sends results back needs something to pair them with.
    fn calls(&self, from: usize) -> Vec<NativeCall> {
        self.message
            .as_ref()
            .and_then(|m| m.tool_calls.as_ref())
            .map(|calls| {
                calls
                    .iter()
                    .enumerate()
                    .map(|(index, call)| NativeCall {
                        id: format!("call_{}", from + index),
                        tool: call.function.name.clone(),
                        arguments: if call.function.arguments.is_null() {
                            Value::Object(Default::default())
                        } else {
                            call.function.arguments.clone()
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn ollama_payload(
    model: &str,
    ctx: &ChatContext,
    stream: bool,
    facts: LocalFacts,
) -> OllamaChatRequest {
    let model = if model.is_empty() { "llama3.1" } else { model };

    let mut messages = vec![OllamaMessage::plain(
        "system",
        ctx.system_prompt.to_string(),
    )];
    for msg in ctx.history {
        messages.push(OllamaMessage::plain(
            &role_for(&msg.sender, "assistant"),
            msg.text.clone(),
        ));
    }
    messages.push(OllamaMessage {
        role: "user".to_string(),
        content: ctx.prompt.to_string(),
        images: ctx
            .images
            .iter()
            .map(|image| image.data_base64.clone())
            .collect(),
        tool_calls: None,
        tool_name: None,
    });

    // This turn's tool rounds, replayed in Ollama's shape so the model sees what it asked
    // for and what came back.
    for exchange in ctx.exchanges {
        match exchange {
            Exchange::Called { text, calls } => messages.push(OllamaMessage {
                role: "assistant".to_string(),
                content: text.clone(),
                images: Vec::new(),
                tool_calls: Some(
                    calls
                        .iter()
                        .map(|call| {
                            json!({"function": {"name": call.tool, "arguments": call.arguments}})
                        })
                        .collect(),
                ),
                tool_name: None,
            }),
            Exchange::Returned(results) => {
                for result in results {
                    messages.push(OllamaMessage {
                        role: "tool".to_string(),
                        content: result.output.clone(),
                        images: Vec::new(),
                        tool_calls: None,
                        tool_name: Some(result.tool.clone()),
                    });
                }
            }
        }
    }

    OllamaChatRequest {
        model: model.to_string(),
        messages,
        stream,
        options: OllamaOptions {
            temperature: 0.7,
            top_p: 0.9,
            num_ctx: local_context_window(facts.context_tokens),
        },
        tools: (!ctx.tools.is_empty()).then(|| {
            ctx.tools
                .iter()
                .map(|t| OpenAiToolDef {
                    kind: "function",
                    function: OpenAiFunctionDef {
                        name: t.name,
                        description: t.description,
                        parameters: t.input_schema.clone(),
                    },
                })
                .collect()
        }),
    }
}

fn ollama_url(endpoint: &str) -> String {
    format!("{}/api/chat", endpoint.trim_end_matches('/'))
}

/// The context size to ask for, from what the model says it was built with.
///
/// Clamped at both ends: above, because a long window costs memory that a reply would
/// rather have (see `LOCAL_CTX_CEILING`); below, because a model reporting a 2048-token
/// window cannot be given more than it has, and asking for more is how Ollama ends up
/// refusing or silently truncating.
pub fn local_context_window(reported: Option<u64>) -> u64 {
    match reported {
        Some(limit) if limit > 0 => limit.min(LOCAL_CTX_CEILING).max(limit.min(LOCAL_CTX_FLOOR)),
        _ => LOCAL_CTX_FLOOR,
    }
}

pub fn call_ollama(endpoint: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
    // Ollama on loopback or on the LAN is what local-only mode exists to keep working,
    // and the gate judges that by the endpoint rather than by the provider's name: an
    // `endpoint` pointing at a rented box is outbound traffic whatever it is running.
    let response: OllamaResponse = checked(
        crate::net::post(&ollama_url(endpoint), "the model was not asked")?
            .config()
            .timeout_global(Some(CALL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .send_json(ollama_payload(
                model,
                ctx,
                false,
                ollama_facts(endpoint, model),
            )),
    )?
    .into_body()
    .read_json()
    .map_err(|e| e.to_string())?;

    let calls = response.calls(0);
    finish_with(response.text().to_string(), response.usage(), calls)
}

/// Ollama streams newline-delimited JSON rather than SSE: one object per token, each with
/// the incremental text under `message.content`.
pub fn stream_ollama(
    endpoint: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
    let response = checked(
        crate::net::post(&ollama_url(endpoint), "the model was not asked")?
            .config()
            .timeout_global(Some(STREAM_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .send_json(ollama_payload(
                model,
                ctx,
                true,
                ollama_facts(endpoint, model),
            )),
    )?;

    let mut full = String::new();
    let mut usage = None;
    let mut calls: Vec<NativeCall> = Vec::new();
    for_each_line(response, |line| {
        let line = line.trim();
        if line.is_empty() {
            return Ok(true);
        }
        let chunk: OllamaResponse = serde_json::from_str(line)
            .map_err(|e| format!("unparseable chunk from Ollama: {e}"))?;
        let delta = chunk.text();
        if !delta.is_empty() {
            full.push_str(delta);
            sink(delta);
        }
        // Unlike OpenAI, a call is not fragmented across chunks -- it arrives whole, on
        // whichever object the model finished it on, so each one is appended as it lands.
        calls.extend(chunk.calls(calls.len()));
        // The counts ride on the last object of the stream, so this overwrites nothing.
        if let Some(reported) = chunk.usage() {
            usage = Some(reported);
        }
        Ok(true)
    })?;

    finish_with(full, usage, calls)
}

// -------------------------------------------------- OpenAI-compatible (OpenAI, Groq, LM Studio)

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
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
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAiStreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDef>>,
}

#[derive(Serialize)]
struct OpenAiToolDef {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiFunctionDef,
}

#[derive(Serialize)]
struct OpenAiFunctionDef {
    name: &'static str,
    description: &'static str,
    parameters: Value,
}

/// One message in the OpenAI shape. An ordinary turn is role + content; an assistant turn
/// that asked for tools also carries `tool_calls`; a result is role `tool` with the
/// `tool_call_id` it answers. Every field but the role is skipped when absent, because
/// sending `"tool_calls": null` is not the same as sending nothing.
#[derive(Serialize, Clone, Debug)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl OpenAiMessage {
    fn plain(role: &str, content: String) -> OpenAiMessage {
        OpenAiMessage {
            role: role.to_string(),
            content: Some(json!(content)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn with_images(role: &str, content: &str, images: &[MediaAttachment]) -> OpenAiMessage {
        let mut parts = vec![json!({"type": "text", "text": content})];
        parts.extend(images.iter().map(|image| json!({
            "type": "image_url",
            "image_url": {"url": format!("data:{};base64,{}", image.mime_type, image.data_base64)},
        })));
        OpenAiMessage {
            role: role.to_string(),
            content: Some(Value::Array(parts)),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// A tool call as the API returns it, streamed or not.
///
/// `index` is what stitches a streamed call together: the arguments arrive as fragments
/// across many chunks and `id` is only sent on the first of them, so the index is the only
/// field present on every fragment.
#[derive(Deserialize, Default, Clone)]
struct OpenAiToolCallChunk {
    #[serde(default)]
    index: u64,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiFunctionChunk>,
}

#[derive(Deserialize, Default, Clone)]
struct OpenAiFunctionChunk {
    #[serde(default)]
    name: Option<String>,
    /// A JSON *string*, not an object -- and only a fragment of one while streaming.
    #[serde(default)]
    arguments: Option<String>,
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
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallChunk>>,
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
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallChunk>>,
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
    let body = ollama_show(endpoint, model)?;
    let details = body.get("details");
    let capability = LocalCapability {
        context_tokens: context_length_of(&body),
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

/// What a local model can be asked to do, as its own server describes it.
///
/// Two questions, both of which AETHER1 was previously guessing the answer to:
///
///   * **Can it call tools?** Ollama answers this directly -- `/api/show` returns a
///     `capabilities` array, and `"tools"` is in it exactly when the model ships a template
///     that can emit a tool call. A model without one cannot be made to by any amount of
///     prompting, and the companion needs to know which of the two protocols it is on
///     *before* it writes its system prompt.
///   * **How much context has it got?** For `num_ctx`, which Ollama otherwise defaults to
///     4096 whatever the model was built with.
///
/// `None` for either means the server did not say -- an older Ollama, or something else
/// answering on the port. A missing answer is never filled in with a plausible one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalFacts {
    pub tools: Option<bool>,
    pub context_tokens: Option<u64>,
}

/// Probes cost a round trip, and the answer is a property of the model rather than of the
/// turn, so each (endpoint, model) pair is asked once per process. The negative answer is
/// cached too: a model that cannot call tools would otherwise be re-probed on every
/// message of a conversation that is already slower for being on the text protocol.
static LOCAL_FACTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, LocalFacts>>,
> = std::sync::LazyLock::new(Default::default);

/// Asks Ollama what one model can do, once per process.
///
/// Nothing here can fail loudly: this runs in front of a reply, and a probe that could
/// refuse a question would be a worse bug than the one it exists to fix. A server that is
/// not Ollama, is not running, or is too old to report capabilities produces
/// `LocalFacts::default()`, which puts the model on the fenced-text protocol with the
/// floor context -- exactly what every local model got before this existed.
pub fn ollama_facts(endpoint: &str, model: &str) -> LocalFacts {
    if model.trim().is_empty() {
        return LocalFacts::default();
    }
    let key = format!("{endpoint}\u{1f}{model}");
    if let Some(known) = LOCAL_FACTS.lock().ok().and_then(|c| c.get(&key).copied()) {
        return known;
    }

    let facts = probe_ollama_facts(endpoint, model).unwrap_or_default();
    if let Ok(mut cache) = LOCAL_FACTS.lock() {
        cache.insert(key, facts);
    }
    facts
}

fn probe_ollama_facts(endpoint: &str, model: &str) -> Option<LocalFacts> {
    let body = ollama_show(endpoint, model)?;
    Some(LocalFacts {
        // Present and not containing "tools" is a real answer -- this model cannot. Absent
        // entirely is no answer, and stays None rather than becoming a false.
        tools: tool_capability_of(&body),
        context_tokens: context_length_of(&body),
    })
}

/// Whether an `/api/show` body says this model can call tools.
///
/// Three answers, not two. `capabilities` present and containing `"tools"` is yes; present
/// and not containing it is a real no, and no amount of prompting will change it; absent
/// altogether is an older Ollama that does not report capabilities, which is not the same
/// as a no and must not be recorded as one.
fn tool_capability_of(body: &serde_json::Value) -> Option<bool> {
    body.get("capabilities")
        .and_then(|c| c.as_array())
        .map(|list| list.iter().any(|c| c.as_str() == Some("tools")))
}

/// Clears the cache, for a settings change that could have changed the answer -- pulling a
/// newer tag of the same name, or restarting Ollama after an upgrade.
pub fn forget_local_facts() {
    if let Ok(mut cache) = LOCAL_FACTS.lock() {
        cache.clear();
    }
}

/// The context length out of an `/api/show` body.
///
/// The key is architecture-prefixed -- `llama.context_length`, `qwen2.context_length`,
/// `gemma3.context_length` -- so rather than keeping a list of architectures that would be
/// out of date by the next release, any key ending in `.context_length` counts.
fn context_length_of(body: &serde_json::Value) -> Option<u64> {
    body.get("model_info")
        .and_then(|info| info.as_object())
        .and_then(|info| {
            info.iter()
                .find(|(key, _)| key.ends_with(".context_length"))
                .and_then(|(_, value)| value.as_u64())
        })
}

/// One `/api/show` call. Deliberately short-timeout: every caller is decoration or a
/// decision with a safe default, and none of them may be the reason a reply feels slow.
fn ollama_show(endpoint: &str, model: &str) -> Option<serde_json::Value> {
    let url = format!("{}/api/show", endpoint.trim_end_matches('/'));
    crate::net::post(&url, "the model was not asked")
        .ok()?
        .config()
        .timeout_global(Some(std::time::Duration::from_millis(2500)))
        .build()
        .send_json(OllamaShowRequest { model })
        .ok()?
        .into_body()
        .read_json()
        .ok()
}

/// The model a provider answers with when the operator has named none.
///
/// The same defaults the payload builders already apply, named in one place so a picker can
/// *show* what it is about to use rather than leaving the box empty and letting the
/// builder decide out of sight. Empty for the two local providers and for a gateway, all
/// three of which are a specific machine's business: a local server's models are whatever
/// was pulled onto it, and a gateway's ids are whatever an administrator connected, so an
/// invented one is a 404 with a confusing message on it.
pub fn default_model_for(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "gpt-4o-mini",
        Provider::Groq => "llama-3.3-70b-versatile",
        Provider::Gemini => "gemini-2.0-flash",
        Provider::Anthropic => "claude-opus-5",
        Provider::Ollama | Provider::LmStudio | Provider::Gateway | Provider::Offline => "",
    }
}

/// The per-provider base URL and the model to use when none is configured.
fn openai_defaults(provider: Provider, endpoint: &str) -> (&str, &str) {
    match provider {
        Provider::OpenAi => (
            "https://api.openai.com/v1",
            default_model_for(Provider::OpenAi),
        ),
        Provider::Groq => (
            "https://api.groq.com/openai/v1",
            default_model_for(Provider::Groq),
        ),
        Provider::LmStudio => {
            let ep = if endpoint.is_empty() {
                "http://localhost:1234/v1"
            } else {
                endpoint
            };
            (ep, "local-model")
        }
        // A gateway gets no default model on purpose. Every other provider here has one
        // model it is overwhelmingly likely to be asked for, so a default is a helpful
        // guess; a gateway's model list is whatever an administrator connected, the model
        // id is what tells it which provider to route to, and an invented id is a 404
        // with a confusing message on it. `gateway_model` turns the empty case into a
        // refusal that names the setting instead.
        Provider::Gateway => (endpoint, ""),
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

    let mut messages = vec![OpenAiMessage::plain(
        "system",
        ctx.system_prompt.to_string(),
    )];
    for msg in ctx.history {
        messages.push(OpenAiMessage::plain(
            &role_for(&msg.sender, "assistant"),
            msg.text.clone(),
        ));
    }
    messages.push(if ctx.images.is_empty() {
        OpenAiMessage::plain("user", ctx.prompt.to_string())
    } else {
        OpenAiMessage::with_images("user", ctx.prompt, ctx.images)
    });

    // This turn's tool rounds. Unlike Anthropic, results are separate messages rather than
    // blocks inside one -- one `tool` message per call, each quoting the id it answers.
    for exchange in ctx.exchanges {
        match exchange {
            Exchange::Called { text, calls } => messages.push(OpenAiMessage {
                role: "assistant".to_string(),
                content: (!text.trim().is_empty()).then(|| json!(text)),
                tool_calls: Some(
                    calls
                        .iter()
                        .map(|call| {
                            json!({
                                "id": call.id,
                                "type": "function",
                                "function": {
                                    "name": call.tool,
                                    // Arguments go back as the JSON *string* they arrived as.
                                    "arguments": call.arguments.to_string(),
                                },
                            })
                        })
                        .collect(),
                ),
                tool_call_id: None,
            }),
            Exchange::Returned(results) => {
                for result in results {
                    messages.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(json!(result.output)),
                        tool_calls: None,
                        tool_call_id: Some(result.id.clone()),
                    });
                }
            }
        }
    }

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
            tools: (!ctx.tools.is_empty()).then(|| {
                ctx.tools
                    .iter()
                    .map(|t| OpenAiToolDef {
                        kind: "function",
                        function: OpenAiFunctionDef {
                            name: t.name,
                            description: t.description,
                            parameters: t.input_schema.clone(),
                        },
                    })
                    .collect()
            }),
        },
    )
}

/// Rebuilds calls from the fragments a stream delivers them in.
///
/// Everything is keyed by `index` because that is the only field present on every fragment:
/// `id` and `name` arrive once, at the start, and the arguments arrive as a run of partial
/// JSON strings that are not parseable until the last one has landed.
#[derive(Default)]
struct OpenAiCallBuilder {
    parts: std::collections::BTreeMap<u64, (String, String, String)>,
}

impl OpenAiCallBuilder {
    fn absorb(&mut self, chunks: &[OpenAiToolCallChunk]) {
        for chunk in chunks {
            let slot = self.parts.entry(chunk.index).or_default();
            if let Some(id) = &chunk.id {
                slot.0 = id.clone();
            }
            if let Some(function) = &chunk.function {
                if let Some(name) = &function.name {
                    slot.1 = name.clone();
                }
                if let Some(arguments) = &function.arguments {
                    slot.2.push_str(arguments);
                }
            }
        }
    }

    fn finish(self) -> Result<Vec<NativeCall>, String> {
        self.parts
            .into_values()
            .map(|(id, tool, arguments)| {
                Ok(NativeCall {
                    id,
                    tool,
                    arguments: parse_arguments(&arguments)?,
                })
            })
            .collect()
    }
}

/// Whether this provider reports the time it spent generating, as distinct from the time
/// the request took.
///
/// Only Ollama does, via `eval_duration` on the final object of its stream. It is the one
/// clock that measures the model rather than the machine's disk and queue, so it is the
/// only one the speed scoreboard will accept a sample from -- and the panel needs to be
/// able to say "this provider cannot be timed" rather than leaving an empty board
/// unexplained.
pub fn reports_generation_time(provider: Provider) -> bool {
    matches!(provider, Provider::Ollama)
}

/// Whether this provider documents `stream_options.include_usage`. LM Studio is left out
/// deliberately: it is the local one, it is the one most likely to be an older build or a
/// look-alike server, and a rejected request there costs a working setup.
fn reports_stream_usage(provider: Provider) -> bool {
    matches!(provider, Provider::OpenAi | Provider::Groq)
}

/// The one place an OpenAI-shaped request is built, for every provider that speaks that
/// shape -- which is why the choice of gate belongs here rather than at the two call
/// sites. `docs/HOW_THIS_IS_BUILT.md` asks for the check at the chokepoint every call
/// passes through, and this is it.
///
/// A gateway forwards, so its gate is `net::post_forwarding`, which does not care that
/// the address might be loopback. Everything else is judged on its address as usual. See
/// `net::require_forwarding` for why the distinction exists.
fn openai_request(
    provider: Provider,
    url: &str,
    api_key: &str,
    timeout: std::time::Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    let gated = if provider == Provider::Gateway {
        crate::net::post_forwarding(url, "the model was not asked")
    } else {
        crate::net::post(url, "the model was not asked")
    };
    let mut request = gated?
        .config()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json");
    if !api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {api_key}"));
    }
    Ok(request)
}

pub fn call_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
) -> Result<Completion, String> {
    let (url, payload) = openai_payload(provider, endpoint, model, ctx, false);

    let response: OpenAiResponse =
        checked(openai_request(provider, &url, api_key, CALL_TIMEOUT)?.send_json(&payload))?
            .into_body()
            .read_json()
            .map_err(|e| e.to_string())?;

    let usage = response.usage.map(TokenUsage::from);
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "empty choices in response".to_string())?;

    // Not streamed, so each call arrives whole -- but the same builder assembles it, so
    // there is one place that turns this shape into a NativeCall rather than two.
    let mut builder = OpenAiCallBuilder::default();
    if let Some(chunks) = &choice.message.tool_calls {
        builder.absorb(chunks);
    }
    let calls = builder.finish()?;
    let text = choice.message.content.unwrap_or_default();
    if text.trim().is_empty() && calls.is_empty() {
        return Err("empty choices in response".to_string());
    }
    Ok(Completion {
        text: text.trim().to_string(),
        usage,
        calls,
    })
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

    let response =
        checked(openai_request(provider, &url, api_key, STREAM_TIMEOUT)?.send_json(&payload))?;

    let mut full = String::new();
    let mut usage = None;
    let mut builder = OpenAiCallBuilder::default();
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
            let Some(delta) = choice.delta else { continue };
            if let Some(chunks) = &delta.tool_calls {
                builder.absorb(chunks);
            }
            if let Some(text) = delta.content {
                if !text.is_empty() {
                    full.push_str(&text);
                    sink(&text);
                }
            }
        }
        Ok(true)
    })?;

    finish_with(full, usage, builder.finish()?)
}

// ---------------------------------------------------------------- Gemini

#[derive(Serialize)]
struct GeminiPart {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    function_call: Option<Value>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    function_response: Option<Value>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    inline_data: Option<Value>,
}

impl GeminiPart {
    fn text(text: impl Into<String>) -> GeminiPart {
        GeminiPart {
            text: Some(text.into()),
            function_call: None,
            function_response: None,
            inline_data: None,
        }
    }
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    /// One entry holding every declaration, which is the shape the API documents -- not one
    /// entry per tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiToolBlock>>,
}

#[derive(Serialize)]
struct GeminiToolBlock {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDecl>,
}

#[derive(Serialize)]
struct GeminiFunctionDecl {
    name: &'static str,
    description: &'static str,
    parameters: Value,
}

#[derive(Deserialize)]
struct GeminiRespPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

/// Gemini names a call rather than giving it an opaque id, and `id` is optional. The name
/// is what the answer is matched on, so it stands in when no id came back.
#[derive(Deserialize)]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    id: Option<String>,
}

impl From<GeminiFunctionCall> for NativeCall {
    fn from(call: GeminiFunctionCall) -> NativeCall {
        NativeCall {
            id: call.id.unwrap_or_else(|| call.name.clone()),
            tool: call.name,
            arguments: call.args.unwrap_or_else(|| json!({})),
        }
    }
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
        default_model_for(Provider::Gemini)
    } else {
        model
    }
}

fn gemini_payload(ctx: &ChatContext) -> GeminiRequest {
    let mut contents = vec![
        GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::text(format!(
                "System Directive: {}",
                ctx.system_prompt
            ))],
        },
        GeminiContent {
            role: "model".to_string(),
            parts: vec![GeminiPart::text(format!(
                "Directive acknowledged. {} systems operational. Ready.",
                ctx.agent_name
            ))],
        },
    ];
    for msg in ctx.history {
        contents.push(GeminiContent {
            role: role_for(&msg.sender, "model"),
            parts: vec![GeminiPart::text(msg.text.clone())],
        });
    }
    let mut current_parts = vec![GeminiPart::text(ctx.prompt.to_string())];
    current_parts.extend(ctx.images.iter().map(|image| GeminiPart {
        text: None,
        function_call: None,
        function_response: None,
        inline_data: Some(json!({"mimeType": image.mime_type, "data": image.data_base64})),
    }));
    contents.push(GeminiContent {
        role: "user".to_string(),
        parts: current_parts,
    });

    // This turn's tool rounds. Gemini keeps everything in `parts`, so a round is one
    // model turn whose parts are the calls, then one user turn whose parts are the answers.
    for exchange in ctx.exchanges {
        match exchange {
            Exchange::Called { text, calls } => {
                let mut parts = Vec::new();
                if !text.trim().is_empty() {
                    parts.push(GeminiPart::text(text.clone()));
                }
                for call in calls {
                    parts.push(GeminiPart {
                        text: None,
                        function_call: Some(json!({
                            "name": call.tool,
                            "args": call.arguments,
                        })),
                        function_response: None,
                        inline_data: None,
                    });
                }
                contents.push(GeminiContent {
                    role: "model".to_string(),
                    parts,
                });
            }
            Exchange::Returned(results) => contents.push(GeminiContent {
                role: "user".to_string(),
                parts: results
                    .iter()
                    .map(|r| GeminiPart {
                        text: None,
                        // `response` is an object rather than a string, so the output is
                        // wrapped rather than sent bare.
                        function_response: Some(json!({
                            "name": r.tool,
                            "response": {"result": r.output},
                        })),
                        function_call: None,
                        inline_data: None,
                    })
                    .collect(),
            }),
        }
    }

    GeminiRequest {
        contents,
        tools: (!ctx.tools.is_empty()).then(|| {
            vec![GeminiToolBlock {
                function_declarations: ctx
                    .tools
                    .iter()
                    .map(|t| GeminiFunctionDecl {
                        name: t.name,
                        description: t.description,
                        parameters: t.input_schema.clone(),
                    })
                    .collect(),
            }]
        }),
    }
}

pub fn call_gemini(api_key: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={api_key}",
        gemini_model(model)
    );

    let response: GeminiResponse = checked(
        crate::net::post(&url, "the model was not asked")?
            .config()
            .timeout_global(Some(CALL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .send_json(gemini_payload(ctx)),
    )?
    .into_body()
    .read_json()
    .map_err(|e| e.to_string())?;

    let usage = response.usage_metadata.map(TokenUsage::from);
    let candidate = response
        .candidates
        .into_iter()
        .next()
        .ok_or_else(|| "empty candidates in response".to_string())?;

    let mut text = String::new();
    let mut calls = Vec::new();
    for part in candidate.content.parts {
        if let Some(said) = part.text {
            text.push_str(&said);
        }
        if let Some(call) = part.function_call {
            calls.push(NativeCall::from(call));
        }
    }

    finish_with(text, usage, calls)
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

    let response = checked(
        crate::net::post(&url, "the model was not asked")?
            .config()
            .timeout_global(Some(STREAM_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .send_json(gemini_payload(ctx)),
    )?;

    let mut full = String::new();
    let mut usage = None;
    let mut calls: Vec<NativeCall> = Vec::new();
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
                if let Some(said) = part.text.filter(|t| !t.is_empty()) {
                    full.push_str(&said);
                    sink(&said);
                }
                if let Some(call) = part.function_call {
                    calls.push(NativeCall::from(call));
                }
            }
        }
        Ok(true)
    })?;

    finish_with(full, usage, calls)
}

// ---------------------------------------------------------------- Anthropic

/// Deliberately generous: 1024 truncated long answers mid-sentence, and the streaming
/// path removes the request-timeout pressure that made a small cap tempting.
const ANTHROPIC_MAX_TOKENS: u32 = 8192;

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    system: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    stream: bool,
    /// Omitted entirely when there are none: an empty array is a different thing from no
    /// tools, and some models behave differently when told they have a toolbox with nothing
    /// in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Serialize)]
struct AnthropicTool {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

/// Content is a Value because it is a plain string on an ordinary turn and an array of
/// blocks on a tool round, and the API accepts both in the same field.
#[derive(Serialize, Clone, Debug)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    // tool_use blocks only.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

/// Turns an accumulated argument buffer into a value.
///
/// A tool that takes no arguments produces no fragments at all, so an empty buffer means
/// `{}` rather than a malformed call. Both streaming providers spell arguments out the
/// same way and both need this same exception.
fn parse_arguments(buffer: &str) -> Result<Value, String> {
    if buffer.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(buffer).map_err(|e| format!("unparseable tool arguments: {e}"))
}

/// Rebuilds Anthropic's tool calls from the events they are spread across.
///
/// `content_block_start` names a call, a run of `content_block_delta` events spells its
/// arguments out a fragment at a time, and `content_block_stop` ends it. Keyed by block
/// index rather than held as a single "current" call, so the reassembly does not depend on
/// blocks never interleaving.
#[derive(Default)]
struct AnthropicCallBuilder {
    building: std::collections::BTreeMap<u64, (String, String, String)>,
    calls: Vec<NativeCall>,
}

impl AnthropicCallBuilder {
    fn start(&mut self, index: u64, block: &AnthropicContentBlock) {
        if block.block_type != "tool_use" {
            return;
        }
        self.building.insert(
            index,
            (
                block.id.clone().unwrap_or_default(),
                block.name.clone().unwrap_or_default(),
                String::new(),
            ),
        );
    }

    fn fragment(&mut self, index: u64, partial: &str) {
        if let Some(slot) = self.building.get_mut(&index) {
            slot.2.push_str(partial);
        }
    }

    fn stop(&mut self, index: u64) -> Result<(), String> {
        let Some((id, tool, arguments)) = self.building.remove(&index) else {
            return Ok(()); // a text block closing, which carries nothing to rebuild
        };
        self.calls.push(NativeCall {
            id,
            tool,
            arguments: parse_arguments(&arguments)?,
        });
        Ok(())
    }

    fn finish(self) -> Vec<NativeCall> {
        self.calls
    }
}

impl AnthropicContentBlock {
    fn as_call(&self) -> Option<NativeCall> {
        if self.block_type != "tool_use" {
            return None;
        }
        Some(NativeCall {
            id: self.id.clone()?,
            tool: self.name.clone()?,
            arguments: self.input.clone().unwrap_or_else(|| json!({})),
        })
    }
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
    /// content_block_start / content_block_delta / content_block_stop all carry the index
    /// of the block they belong to. Tool arguments arrive as fragments across many deltas,
    /// so the index is what reassembles them into the right call.
    #[serde(default)]
    index: Option<u64>,
    /// content_block_start only.
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
}

#[derive(Deserialize)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
    /// input_json_delta only: one fragment of the tool's arguments, as a JSON string that
    /// is not valid JSON on its own.
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicError {
    #[serde(default)]
    message: String,
}

/// Bends a conversation into the shape the Messages API insists on: the first message is
/// from the user, and roles strictly alternate after that.
///
/// Nothing else here needs this. OpenAI and Gemini accept a transcript as it happened, so
/// the history was handed over untouched -- and a real transcript is full of sequences that
/// are not alternating. Two replies in a row whenever the companion answers and then posts
/// a status line; an assistant message first whenever the eight-message window happens to
/// open on one. Anthropic answers both with a 400 that never reaches the operator as
/// anything but a number.
///
/// Consecutive turns from the same side are joined rather than dropped, because they are
/// what was actually said and losing them would change the conversation to make it fit.
fn alternating(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        match out.last_mut() {
            // Leading assistant turns have nothing to answer, so the window starts at the
            // first thing the operator said.
            None if message.role != "user" => continue,
            Some(previous) if previous.role == message.role => {
                previous.content.push_str("\n\n");
                previous.content.push_str(&message.content);
            }
            _ => out.push(message),
        }
    }
    out
}

fn anthropic_payload(model: &str, ctx: &ChatContext, stream: bool) -> AnthropicRequest {
    // The stored conversation first, bent into the alternating shape the API requires.
    let mut plain = Vec::new();
    for msg in ctx.history {
        plain.push(ChatMessage {
            role: role_for(&msg.sender, "assistant"),
            content: msg.text.clone(),
        });
    }
    plain.push(ChatMessage {
        role: "user".to_string(),
        content: ctx.prompt.to_string(),
    });
    let mut messages: Vec<AnthropicMessage> = alternating(plain)
        .into_iter()
        .map(|m| AnthropicMessage {
            role: m.role,
            content: Value::String(m.content),
        })
        .collect();

    if !ctx.images.is_empty() {
        if let Some(user) = messages.last_mut() {
            let mut blocks = vec![json!({"type": "text", "text": ctx.prompt})];
            blocks.extend(ctx.images.iter().map(|image| json!({
                "type": "image",
                "source": {"type": "base64", "media_type": image.mime_type, "data": image.data_base64},
            })));
            user.content = Value::Array(blocks);
        }
    }

    // Then this turn's tool rounds, which alternate by construction: the model asks, the
    // tools answer, and nothing else is interleaved.
    for exchange in ctx.exchanges {
        match exchange {
            Exchange::Called { text, calls } => {
                let mut blocks = Vec::new();
                if !text.trim().is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for call in calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.tool,
                        "input": call.arguments,
                    }));
                }
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: Value::Array(blocks),
                });
            }
            Exchange::Returned(results) => {
                // Every result for a round goes in one user message. Splitting them teaches
                // the model to stop asking for more than one tool at a time.
                let blocks: Vec<Value> = results
                    .iter()
                    .map(|r| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": r.id,
                            "content": r.output,
                        })
                    })
                    .collect();
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: Value::Array(blocks),
                });
            }
        }
    }

    AnthropicRequest {
        model: if model.is_empty() {
            default_model_for(Provider::Anthropic)
        } else {
            model
        }
        .to_string(),
        system: ctx.system_prompt.to_string(),
        messages,
        max_tokens: ANTHROPIC_MAX_TOKENS,
        stream,
        tools: (!ctx.tools.is_empty()).then(|| {
            ctx.tools
                .iter()
                .map(|t| AnthropicTool {
                    name: t.name,
                    description: t.description,
                    input_schema: t.input_schema.clone(),
                })
                .collect()
        }),
    }
}

fn anthropic_request(
    api_key: &str,
    timeout: std::time::Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    Ok(crate::net::post(
        "https://api.anthropic.com/v1/messages",
        "the model was not asked",
    )?
    .config()
    .timeout_global(Some(timeout))
    .http_status_as_error(false)
    .build()
    .header("x-api-key", api_key)
    .header("anthropic-version", "2023-06-01")
    .header("content-type", "application/json"))
}

pub fn call_anthropic(api_key: &str, model: &str, ctx: &ChatContext) -> Result<Completion, String> {
    let response: AnthropicResponse = checked(
        anthropic_request(api_key, CALL_TIMEOUT)?.send_json(anthropic_payload(model, ctx, false)),
    )?
    .into_body()
    .read_json()
    .map_err(|e| e.to_string())?;

    let usage = response.usage.map(|u| TokenUsage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        eval_nanos: None,
    });
    // A reply is a list of blocks, not one block: text and tool_use arrive side by side,
    // and taking only the first would silently drop whichever came second.
    let mut text = String::new();
    let mut calls = Vec::new();
    for block in &response.content {
        if let Some(call) = block.as_call() {
            calls.push(call);
        } else if let Some(chunk) = &block.text {
            text.push_str(chunk);
        }
    }
    if text.trim().is_empty() && calls.is_empty() {
        return Err("empty content in response".to_string());
    }
    Ok(Completion {
        text: text.trim().to_string(),
        usage,
        calls,
    })
}

pub fn stream_anthropic(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    sink: Sink,
) -> Result<Completion, String> {
    let response = checked(
        anthropic_request(api_key, STREAM_TIMEOUT)?.send_json(anthropic_payload(model, ctx, true)),
    )?;

    let mut full = String::new();
    // Input arrives once at the top, output as a running total near the end, so the two
    // halves are accumulated separately and combined when the stream closes.
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut reported = false;
    let mut builder = AnthropicCallBuilder::default();
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
            "content_block_start" => {
                if let (Some(index), Some(block)) = (event.index, event.content_block.as_ref()) {
                    builder.start(index, block);
                }
                Ok(true)
            }
            "content_block_delta" => {
                let Some(delta) = event.delta else {
                    return Ok(true);
                };
                if let Some(fragment) = delta.partial_json {
                    if let Some(index) = event.index {
                        builder.fragment(index, &fragment);
                    }
                } else if let Some(text) = delta.text {
                    if !text.is_empty() {
                        full.push_str(&text);
                        sink(&text);
                    }
                }
                Ok(true)
            }
            "content_block_stop" => {
                if let Some(index) = event.index {
                    builder.stop(index)?;
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
    finish_with(full, usage, builder.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    /// The exact shape that produced a 400 in the field: the companion answered, then
    /// posted a status line, so two assistant turns ran together.
    #[test]
    fn two_replies_in_a_row_become_one_turn() {
        let out = alternating(vec![
            msg("user", "Claude?"),
            msg("assistant", "[HUD Alert: ...]"),
            msg("assistant", "Cognitive Core updated."),
            msg("user", "Claude you there?"),
        ]);
        let roles: Vec<&str> = out.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
        // Joined rather than dropped -- both were really said.
        assert!(out[1].content.contains("HUD Alert"));
        assert!(out[1].content.contains("Cognitive Core updated."));
    }

    /// The other half of the same failure: the eight-message window can open on a reply,
    /// and the Messages API requires the first message to be the user's.
    #[test]
    fn a_window_opening_on_a_reply_starts_at_the_first_thing_the_operator_said() {
        let out = alternating(vec![
            msg("assistant", "...earlier reply"),
            msg("assistant", "and another"),
            msg("user", "now this"),
            msg("assistant", "answer"),
            msg("user", "and this"),
        ]);
        let roles: Vec<&str> = out.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
        assert_eq!(out[0].content, "now this");
    }

    #[test]
    fn a_conversation_that_already_alternates_is_left_alone() {
        let original = vec![
            msg("user", "one"),
            msg("assistant", "two"),
            msg("user", "three"),
        ];
        assert_eq!(alternating(original.clone()), original);
    }

    /// A fresh session is one user message and nothing else, which is already valid.
    #[test]
    fn the_first_turn_of_a_session_survives() {
        let out = alternating(vec![msg("user", "hello")]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "user");

        // And a history of nothing but replies leaves nothing to send rather than an
        // invalid request.
        assert!(alternating(vec![msg("assistant", "a"), msg("assistant", "b")]).is_empty());
    }

    /// Through the real payload builder, not just the helper: the failure this fixes came
    /// back if anyone removed the one call, and a test on `alternating` alone would not
    /// have noticed.
    #[test]
    fn the_anthropic_payload_is_always_in_a_shape_the_api_accepts() {
        let history = vec![
            Message {
                sender: "user".to_string(),
                text: "Claude?".to_string(),
                timestamp: String::new(),
            },
            Message {
                sender: "assistant".to_string(),
                text: "[HUD Alert: ...]".to_string(),
                timestamp: String::new(),
            },
            Message {
                sender: "assistant".to_string(),
                text: "Cognitive Core updated.".to_string(),
                timestamp: String::new(),
            },
        ];
        let ctx = ChatContext {
            system_prompt: "be useful",
            history: &history,
            prompt: "Claude you there?",
            images: &[],
            agent_name: "R.E.D. 9000",
            tools: &[],
            exchanges: &[],
        };
        let payload = anthropic_payload("claude-opus-5", &ctx, true);

        assert_eq!(
            payload.messages[0].role, "user",
            "must open on the operator"
        );
        for pair in payload.messages.windows(2) {
            assert_ne!(
                pair[0].role, pair[1].role,
                "roles have to alternate: {:?}",
                payload.messages
            );
        }
    }

    /// A 400 means the request was wrong, not the key or the network, and the provider's
    /// own body says which part. Paraphrasing that away is what left an operator reading
    /// "http status: 400" with nothing to act on.
    #[test]
    fn a_rejected_request_keeps_what_the_provider_said_about_it() {
        let raw = "http status: 400 -- {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"messages: roles must alternate between \\\"user\\\" and \\\"assistant\\\"\"}}";
        let explained = explain_failure(Provider::Anthropic, "", raw);
        assert!(
            explained.contains("rejected the request itself"),
            "{explained}"
        );
        assert!(explained.contains("roles must alternate"), "{explained}");

        // Still distinct from the key being wrong, which is a different fix.
        let unauthorized = explain_failure(Provider::Anthropic, "", "http status: 401");
        assert!(unauthorized.contains("API KEY"), "{unauthorized}");
    }

    /// Each provider reports its token counts in a different shape and a different place.
    /// These parse the real ones -- the counts were being thrown away and replaced with a
    /// four-characters-per-token guess, and nothing would have noticed if a rename upstream
    /// silently turned them all back into None.
    #[test]
    fn ollama_reports_its_counts_and_its_own_generation_time() {
        let final_chunk: OllamaResponse = serde_json::from_str(
            r#"{"message":{"content":""},"done":true,"prompt_eval_count":41,"eval_count":128,"eval_duration":2000000000}"#,
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
            serde_json::from_str(r#"{"message":{"content":"hi"},"done":false}"#).unwrap();
        assert_eq!(mid.text(), "hi");
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
            images: &[],
            agent_name: "A1",
            tools: &[],
            exchanges: &[],
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
        assert!(finish_with("   \n ".to_string(), None, Vec::new()).is_err());
        assert_eq!(
            finish_with(" hello ".to_string(), None, Vec::new())
                .unwrap()
                .text,
            "hello"
        );
    }

    #[test]
    fn payloads_match_between_blocking_and_streaming() {
        // The whole point of sharing the builders: only the stream flag differs.
        let ctx = ChatContext {
            system_prompt: "be helpful",
            history: &[],
            prompt: "hello",
            images: &[],
            agent_name: "HALCY",
            tools: &[],
            exchanges: &[],
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
            images: &[],
            agent_name: "a",
            tools: &[],
            exchanges: &[],
        };
        assert_eq!(
            ollama_payload("", &ctx, true, LocalFacts::default()).model,
            "llama3.1",
            "the fallback is a model that can call tools, not one that cannot"
        );
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

    /// A gateway's refusals have to send the operator to the gateway's own fields, not to
    /// the provider ones they may have filled in correctly for something else.
    #[test]
    fn a_gateway_refusal_names_the_gateways_own_settings() {
        let key = explain_failure(Provider::Gateway, "", "http status: 401");
        assert!(key.contains("GATEWAY KEY"), "{key}");
        assert!(!key.contains("API KEY"), "{key}");

        // A model id a gateway does not route may be a typo or may be a model this key was
        // never granted, and the operator cannot tell which from here -- so the message
        // must not assert the first and must point at the list, which settles both.
        let model = explain_failure(Provider::Gateway, "", "http status: 404");
        assert!(model.contains("model list"), "{model}");
        assert!(
            !model.contains("MODEL NAME / ID"),
            "a gateway has no such field: {model}"
        );
    }

    /// Guessing a model id for a gateway would pick which company answers and who gets
    /// billed, so there is deliberately no default to fall back on.
    #[test]
    fn a_gateway_gets_no_default_model() {
        let (_, default_model) = openai_defaults(Provider::Gateway, "https://gw.example/v1");
        assert!(
            default_model.is_empty(),
            "a gateway must not have a guessed default model, got {default_model:?}"
        );
        // And its address is used exactly as given, rather than being replaced by one of
        // the hardcoded provider bases.
        let (base, _) = openai_defaults(Provider::Gateway, "https://gw.example/v1");
        assert_eq!(base, "https://gw.example/v1");
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

    // ------------------------------------------------ native tool calling

    fn a_tool() -> crate::tools::ToolSchema {
        crate::tools::ToolSchema {
            name: "read_file",
            description: "Read a file",
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
            mutating: false,
            always_allowable: true,
        }
    }

    fn with_tools<'a>(
        tools: &'a [crate::tools::ToolSchema],
        exchanges: &'a [Exchange],
    ) -> ChatContext<'a> {
        ChatContext {
            system_prompt: "be useful",
            history: &[],
            prompt: "what is in /etc/hostname?",
            images: &[],
            agent_name: "R.E.D. 9000",
            tools,
            exchanges,
        }
    }

    fn one_round() -> Vec<Exchange> {
        vec![
            Exchange::Called {
                text: "Let me look.".to_string(),
                calls: vec![NativeCall {
                    id: "call_1".to_string(),
                    tool: "read_file".to_string(),
                    arguments: json!({"path": "/etc/hostname"}),
                }],
            },
            Exchange::Returned(vec![CallResult {
                id: "call_1".to_string(),
                tool: "read_file".to_string(),
                output: "aether".to_string(),
            }]),
        ]
    }

    /// A turn with no tools must look exactly like it did before native tools existed:
    /// the field is absent, not an empty array. Some models behave differently when told
    /// they have a toolbox with nothing in it.
    #[test]
    fn no_tools_means_no_tools_field_at_all() {
        let tools = Vec::new();
        let ctx = with_tools(&tools, &[]);

        let anthropic = serde_json::to_value(anthropic_payload("m", &ctx, true)).unwrap();
        assert!(anthropic.get("tools").is_none(), "{anthropic}");

        let (_, openai) = openai_payload(Provider::OpenAi, "", "", &ctx, true);
        let openai = serde_json::to_value(openai).unwrap();
        assert!(openai.get("tools").is_none(), "{openai}");

        let gemini = serde_json::to_value(gemini_payload(&ctx)).unwrap();
        assert!(gemini.get("tools").is_none(), "{gemini}");
    }

    /// Each provider spells the same tool out in its own shape. These are the three wire
    /// formats verified against the published request schemas.
    #[test]
    fn each_provider_declares_a_tool_in_its_own_shape() {
        let tools = vec![a_tool()];
        let ctx = with_tools(&tools, &[]);

        let anthropic = serde_json::to_value(anthropic_payload("m", &ctx, true)).unwrap();
        let declared = &anthropic["tools"][0];
        assert_eq!(declared["name"], "read_file");
        assert_eq!(declared["input_schema"]["required"], json!(["path"]));

        let (_, openai) = openai_payload(Provider::OpenAi, "", "", &ctx, true);
        let openai = serde_json::to_value(openai).unwrap();
        let declared = &openai["tools"][0];
        assert_eq!(declared["type"], "function");
        assert_eq!(declared["function"]["name"], "read_file");
        assert_eq!(
            declared["function"]["parameters"]["required"],
            json!(["path"])
        );

        let gemini = serde_json::to_value(gemini_payload(&ctx)).unwrap();
        let declared = &gemini["tools"][0]["functionDeclarations"][0];
        assert_eq!(declared["name"], "read_file");
        assert_eq!(declared["parameters"]["required"], json!(["path"]));
    }

    /// Anthropic wants the whole round back as content blocks, and every result for the
    /// round in a single user message -- splitting them teaches the model to ask for one
    /// tool at a time.
    #[test]
    fn anthropic_replays_a_round_as_content_blocks() {
        let tools = vec![a_tool()];
        let exchanges = one_round();
        let payload = serde_json::to_value(anthropic_payload(
            "m",
            &with_tools(&tools, &exchanges),
            true,
        ))
        .unwrap();
        let messages = payload["messages"].as_array().unwrap();

        assert_eq!(messages.len(), 3, "{payload}");
        assert_eq!(messages[0]["role"], "user");

        assert_eq!(messages[1]["role"], "assistant");
        let blocks = messages[1]["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "call_1");
        assert_eq!(blocks[1]["input"]["path"], "/etc/hostname");

        assert_eq!(messages[2]["role"], "user");
        let blocks = messages[2]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "call_1");
        assert_eq!(blocks[0]["content"], "aether");
    }

    /// OpenAI is the other arrangement: the call rides on the assistant message and each
    /// result is its own `tool` message quoting the id it answers. Arguments go back as
    /// the JSON string they arrived as, not as an object.
    #[test]
    fn openai_replays_a_round_as_tool_messages() {
        let tools = vec![a_tool()];
        let exchanges = one_round();
        let (_, payload) = openai_payload(
            Provider::OpenAi,
            "",
            "",
            &with_tools(&tools, &exchanges),
            true,
        );
        let payload = serde_json::to_value(payload).unwrap();
        let messages = payload["messages"].as_array().unwrap();

        let asked = messages
            .iter()
            .find(|m| m["tool_calls"].is_array())
            .unwrap();
        assert_eq!(asked["role"], "assistant");
        let call = &asked["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "read_file");
        assert_eq!(call["function"]["arguments"], r#"{"path":"/etc/hostname"}"#);

        let answered = messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(answered["tool_call_id"], "call_1");
        assert_eq!(answered["content"], "aether");
    }

    #[test]
    fn gemini_replays_a_round_as_function_parts() {
        let tools = vec![a_tool()];
        let exchanges = one_round();
        let payload =
            serde_json::to_value(gemini_payload(&with_tools(&tools, &exchanges))).unwrap();
        let contents = payload["contents"].as_array().unwrap();

        let asked = contents.last().unwrap();
        // The last model turn: the first is the acknowledgement the system prompt is
        // dressed up as, which carries nothing to find.
        let model_turn = contents
            .iter()
            .rev()
            .find(|c| c["role"] == "model")
            .unwrap();
        let call = model_turn["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|p| p.get("functionCall"))
            .unwrap();
        assert_eq!(call["name"], "read_file");
        assert_eq!(call["args"]["path"], "/etc/hostname");

        // The answer is the last turn, and `response` is an object rather than a bare
        // string -- Gemini rejects a string there.
        assert_eq!(asked["role"], "user");
        let answer = &asked["parts"][0]["functionResponse"];
        assert_eq!(answer["name"], "read_file");
        assert_eq!(answer["response"]["result"], "aether");
    }

    /// OpenAI streams a call in fragments whose only reliable field is `index`: the id and
    /// name arrive once and the arguments dribble in as unparseable partial JSON.
    #[test]
    fn openai_reassembles_a_call_from_its_fragments() {
        let mut builder = OpenAiCallBuilder::default();
        for data in [
            r#"{"index":0,"id":"call_7","type":"function","function":{"name":"read_file","arguments":""}}"#,
            r#"{"index":0,"function":{"arguments":"{\"pa"}}"#,
            r#"{"index":0,"function":{"arguments":"th\": \"/etc/hostname\"}"}}"#,
        ] {
            let chunk: OpenAiToolCallChunk = serde_json::from_str(data).unwrap();
            builder.absorb(&[chunk]);
        }

        let calls = builder.finish().unwrap();
        assert_eq!(
            calls,
            vec![NativeCall {
                id: "call_7".to_string(),
                tool: "read_file".to_string(),
                arguments: json!({"path": "/etc/hostname"}),
            }]
        );
    }

    /// Two calls in one round are told apart by index alone, and come back in index order.
    #[test]
    fn openai_keeps_two_calls_in_the_same_round_apart() {
        let mut builder = OpenAiCallBuilder::default();
        for data in [
            r#"{"index":1,"id":"b","function":{"name":"get_time","arguments":"{}"}}"#,
            r#"{"index":0,"id":"a","function":{"name":"read_file","arguments":"{\"path\":\"/x\"}"}}"#,
        ] {
            let chunk: OpenAiToolCallChunk = serde_json::from_str(data).unwrap();
            builder.absorb(&[chunk]);
        }

        let calls = builder.finish().unwrap();
        let names: Vec<&str> = calls.iter().map(|c| c.tool.as_str()).collect();
        assert_eq!(names, vec!["read_file", "get_time"]);
        assert_eq!(calls[1].arguments, json!({}));
    }

    /// Anthropic spreads a call across three event types. The arguments are not valid JSON
    /// until the last fragment lands, so nothing can be parsed before content_block_stop.
    #[test]
    fn anthropic_reassembles_a_call_across_its_events() {
        let mut builder = AnthropicCallBuilder::default();
        let start: AnthropicContentBlock = serde_json::from_str(
            r#"{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}"#,
        )
        .unwrap();
        builder.start(1, &start);
        builder.fragment(1, "{\"path\"");
        builder.fragment(1, ": \"/etc/hostname\"}");
        builder.stop(1).unwrap();

        assert_eq!(
            builder.finish(),
            vec![NativeCall {
                id: "toolu_1".to_string(),
                tool: "read_file".to_string(),
                arguments: json!({"path": "/etc/hostname"}),
            }]
        );
    }

    /// Block 0 of a reply is usually text, and its start and stop events must not produce
    /// a call made of nothing.
    #[test]
    fn a_text_block_closing_is_not_mistaken_for_a_call() {
        let mut builder = AnthropicCallBuilder::default();
        let text: AnthropicContentBlock =
            serde_json::from_str(r#"{"type":"text","text":""}"#).unwrap();
        builder.start(0, &text);
        builder.stop(0).unwrap();
        assert!(builder.finish().is_empty());
    }

    /// A tool that takes no arguments sends no fragments, which means `{}` rather than a
    /// broken call.
    #[test]
    fn an_argument_less_call_means_an_empty_object() {
        assert_eq!(parse_arguments("").unwrap(), json!({}));
        assert_eq!(parse_arguments("   ").unwrap(), json!({}));
        assert!(parse_arguments("{\"half\":").is_err());
    }

    /// Gemini names a call instead of giving it an id, and the id is optional -- the name
    /// stands in so there is always something to match the answer to.
    #[test]
    fn a_gemini_call_without_an_id_falls_back_to_its_name() {
        let call: GeminiFunctionCall =
            serde_json::from_str(r#"{"name":"get_time","args":{}}"#).unwrap();
        let call = NativeCall::from(call);
        assert_eq!(call.id, "get_time");
        assert_eq!(call.tool, "get_time");
        assert_eq!(call.arguments, json!({}));
    }

    /// A round that is only a tool call carries no text, and that is a complete answer
    /// rather than the "stream ended without any content" failure an empty reply is.
    #[test]
    fn a_reply_that_is_only_a_tool_call_is_not_an_empty_stream() {
        let call = NativeCall {
            id: "a".to_string(),
            tool: "get_time".to_string(),
            arguments: json!({}),
        };
        let completion = finish_with(String::new(), None, vec![call]).unwrap();
        assert!(completion.text.is_empty());
        assert_eq!(completion.calls.len(), 1);

        assert!(finish_with(String::new(), None, Vec::new()).is_err());
    }

    /// Ollama carries tools now; LM Studio does not. See `Provider::supports_native_tools`
    /// for why the two local providers are split.
    #[test]
    fn ollama_carries_tools_and_lm_studio_does_not() {
        assert!(Provider::Ollama.supports_native_tools());
        assert!(!Provider::LmStudio.supports_native_tools());
        assert!(!Provider::Offline.supports_native_tools());
        for provider in [
            Provider::OpenAi,
            Provider::Groq,
            Provider::Gemini,
            Provider::Anthropic,
        ] {
            assert!(provider.supports_native_tools(), "{provider}");
        }
    }

    /// A model that says nothing about its context gets the floor rather than Ollama's
    /// 4096 default, a model that reports less than the floor gets what it has, and a
    /// 128K model is capped -- a window that size is gigabytes of KV cache.
    #[test]
    fn the_local_context_window_is_clamped_at_both_ends() {
        assert_eq!(local_context_window(None), LOCAL_CTX_FLOOR);
        assert_eq!(local_context_window(Some(0)), LOCAL_CTX_FLOOR);
        assert_eq!(local_context_window(Some(2048)), 2048);
        assert_eq!(local_context_window(Some(4096)), 4096);
        assert_eq!(local_context_window(Some(8192)), LOCAL_CTX_FLOOR);
        assert_eq!(local_context_window(Some(32_768)), LOCAL_CTX_CEILING);
        assert_eq!(local_context_window(Some(131_072)), LOCAL_CTX_CEILING);
    }

    /// `capabilities` present and without "tools" is a real "it cannot"; the field missing
    /// altogether is no answer at all, and must not become a false.
    #[test]
    fn tool_capability_distinguishes_no_from_unknown() {
        let says_yes = json!({"capabilities": ["completion", "tools", "vision"]});
        let says_no = json!({"capabilities": ["completion"]});
        let silent = json!({"details": {"parameter_size": "7B"}});
        assert_eq!(tool_capability_of(&says_yes), Some(true));
        assert_eq!(tool_capability_of(&says_no), Some(false));
        assert_eq!(tool_capability_of(&silent), None);
    }

    /// The chat payload sends real turns with real roles. The old `/api/generate` payload
    /// flattened them into one "USER: ...\nA1: ..." string handed over as a single user
    /// message, which is why a model answered by continuing the script instead of replying.
    #[test]
    fn the_chat_payload_sends_turns_not_a_transcript() {
        let history = vec![
            Message {
                sender: "user".to_string(),
                text: "first question".to_string(),
                timestamp: String::new(),
            },
            Message {
                sender: "a1".to_string(),
                text: "first answer".to_string(),
                timestamp: String::new(),
            },
        ];
        let ctx = ChatContext {
            system_prompt: "you are A1",
            history: &history,
            prompt: "second question",
            images: &[],
            agent_name: "A1",
            tools: &[],
            exchanges: &[],
        };
        let payload = ollama_payload("llama3.1", &ctx, true, LocalFacts::default());
        let roles: Vec<&str> = payload.messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(payload.messages[3].content, "second question");
        assert!(payload.tools.is_none(), "no tools offered, so none sent");
    }

    /// A tool round goes back in Ollama's shape: the assistant message carries
    /// `tool_calls`, and each result is its own `tool` message naming the tool it answers.
    #[test]
    fn a_tool_round_is_replayed_in_ollamas_shape() {
        let exchanges = vec![
            Exchange::Called {
                text: String::new(),
                calls: vec![NativeCall {
                    id: "call_0".to_string(),
                    tool: "search_web".to_string(),
                    arguments: json!({"query": "latest f1 result"}),
                }],
            },
            Exchange::Returned(vec![CallResult {
                id: "call_0".to_string(),
                tool: "search_web".to_string(),
                output: "eight results".to_string(),
            }]),
        ];
        let schema = crate::tools::ToolSchema {
            name: "search_web",
            description: "search the web",
            input_schema: json!({"type": "object"}),
            mutating: false,
            always_allowable: true,
        };
        let schemas = [schema];
        let ctx = ChatContext {
            system_prompt: "you are A1",
            history: &[],
            prompt: "who won",
            images: &[],
            agent_name: "A1",
            tools: &schemas,
            exchanges: &exchanges,
        };
        let payload = ollama_payload("llama3.1", &ctx, true, LocalFacts::default());
        let roles: Vec<&str> = payload.messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool"]);
        assert!(payload.messages[2].tool_calls.is_some());
        assert_eq!(
            payload.messages[3].tool_name.as_deref(),
            Some("search_web"),
            "Ollama has no call ids, so the tool name is what pairs a result to its call"
        );
        let tools = payload.tools.expect("the schema was offered");
        assert_eq!(tools.len(), 1);
    }

    /// A streamed call arrives whole, on one object, with no id -- unlike OpenAI, where it
    /// is reassembled from fragments keyed by index.
    #[test]
    fn a_streamed_ollama_call_is_read_off_one_object() {
        let chunk: OllamaResponse = serde_json::from_str(
            r#"{"message":{"content":"","tool_calls":[{"function":{"name":"search_web","arguments":{"query":"f1"}}}]},"done":false}"#,
        )
        .expect("a chat chunk parses");
        let calls = chunk.calls(0);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, "search_web");
        assert_eq!(calls[0].arguments, json!({"query": "f1"}));
        assert_eq!(
            calls[0].id, "call_0",
            "positional, because Ollama issues none"
        );
    }
}
