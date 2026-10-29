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
    /* What to call it in front of the operator. `Display` is the settings key, which is
    right for a log line and wrong here: it put "lmstudio rejected the request itself" in
    front of somebody who does not have LM Studio installed, because that key drives every
    OpenAI-compatible local server. `label` no longer guesses at a product -- and for a
    local server the endpoint is the only thing that actually identifies which program
    refused, so it is said. */
    let named = provider.label();
    let at = if local {
        format!(" at {endpoint}")
    } else {
        String::new()
    };

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
        return format!("{named}{at} rejected the API key -- check the {key_field}.");
    }
    if lowered.contains("403") || lowered.contains("forbidden") {
        if lowered.contains("proxy") {
            return format!(
                "a proxy on this network refused the connection to {named}, so the request never arrived."
            );
        }
        return format!(
            "{named} refused the API key -- it may lack access to this model, or be for the wrong account."
        );
    }
    // 400 is the provider saying the request was wrong rather than the key or the network,
    // and its body says which part. That body is worth more than anything that could be
    // written here, so this passes it through instead of paraphrasing it away.
    /* One 400 is worth paraphrasing, because its body names the problem and says nothing
    about the fix. "request (5242 tokens) exceeds the available context size (4096
    tokens)" on a *first* question is not a long conversation -- it is the system prompt
    not fitting, and it will happen again on every question until the model is reloaded.
    An OpenAI-shaped server takes the context from however the model was loaded, not from
    the request, so there is nothing AETHER1 can set to get round it. See `num_ctx` in
    `ollama_payload` for the provider where there is. */
    if lowered.contains("exceeds the available context size")
        || (lowered.contains("context") && lowered.contains("exceed"))
    {
        /* Which program to go and change this in cannot be known from here -- the
        endpoint is the only honest identification -- so both common spellings of the
        same knob are named and the operator picks the one their server has. */
        let where_to = if local {
            " Load the model again with a bigger context -- 8192 or more. On a llama.cpp              server that is `--ctx-size 8192`; in LM Studio it is the context length on              the model's load settings; through Ollama's OpenAI-compatible endpoint it is              the model's `num_ctx`."
        } else {
            ""
        };
        return format!(
            "the server{at} loaded this model with a context too small for a single \
             question: the prompt alone did not fit, so the length of the conversation had \
             nothing to do with it. That context is fixed when the model is loaded rather \
             than per request, so AETHER1 cannot raise it from here.{where_to} The refusal \
             as it arrived: {raw}"
        );
    }
    /* A second 400 worth paraphrasing, for the same reason: the body names the problem
    precisely and the fix is not the one it reads like. An Anthropic key created at the
    organisation level rather than inside a workspace is refused until the request carries
    an `anthropic-workspace-id` header -- which AETHER1 has nowhere to put, since nothing
    in Settings asks for a workspace. So the actionable half is the *other* half of what
    Anthropic says: use a key that is scoped to a workspace. Without this the operator
    reads a header name and reasonably goes looking for a header field. */
    if lowered.contains("anthropic-workspace-id") || lowered.contains("scoped to a workspace") {
        return format!(
            "{named} refused this API key because it belongs to the organisation rather \
             than to one workspace, and a key like that has to name a workspace on every \
             request. AETHER1 has nowhere to put that, so the fix is a different key: \
             create one inside a workspace in the Anthropic console and paste it into the \
             {key_field}. The refusal as it arrived: {raw}"
        );
    }
    if lowered.contains("400") || lowered.contains("invalid_request") {
        return format!("{named}{at} rejected the request itself: {raw}");
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
    /// How much reasoning to ask this model for. Each provider turns it into whatever its
    /// own API calls that, or into nothing at all where there is no safe way to ask.
    pub reasoning: Reasoning,
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
    /// The model's own reasoning, kept apart from the answer rather than left in it.
    ///
    /// Empty when the model did not think, was not asked to, or thought where nothing can
    /// see it -- OpenAI's reasoning models bill for thinking tokens and return none of
    /// them. It is never stored in the conversation and never spoken: it is working, not
    /// the reply, and a transcript that keeps it reads as something nobody said.
    pub reasoning: String,
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
///
/// Nor is a turn that was only reasoning, and this is the one place that could have
/// reported it as one: a thinking model cut off after `</think>` has said something, it
/// just has not finished saying it. Showing the thought beats "stream ended without any
/// content", which is what the operator used to get.
fn finish_with(
    relay: &mut Relay,
    usage: Option<TokenUsage>,
    calls: Vec<NativeCall>,
) -> Result<Completion, String> {
    let (text, reasoning) = relay.finish();
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() && calls.is_empty() && reasoning.trim().is_empty() {
        return Err("stream ended without any content".to_string());
    }
    Ok(Completion {
        text: trimmed,
        reasoning: reasoning.trim().to_string(),
        usage,
        calls,
    })
}

// ---------------------------------------------------------------- Reasoning

/// The tag pairs a model wraps its reasoning in when the server hands it back inside the
/// answer rather than in a field of its own. qwen3, deepseek-r1 and the rest of the local
/// thinking models all do this, and llama.cpp, LM Studio and Ollama-without-`think` all
/// pass it straight through -- which is how `<think>` ended up on screen as if it were the
/// reply. Longest-first is not needed here: `<think>` is not a prefix of `<thinking>`,
/// because of the closing bracket.
const THOUGHT_TAGS: [(&str, &str); 2] = [("<think>", "</think>"), ("<thinking>", "</thinking>")];

/// How much of its own reasoning the model is asked to do, and to show.
///
/// `Auto` is the default and the reason this is five values rather than a checkbox. Asking
/// for reasoning is not free and not universally safe: Ollama refuses `think` outright for
/// a model that cannot do it, an older OpenAI-compatible server can 400 on a field it has
/// never heard of, and Anthropic and Gemini both bill for thinking tokens. So `Auto` means
/// "take the reasoning whenever the model offers it, and switch thinking on only where
/// doing so cannot break or bill anything" -- which turns out to cover the case that
/// matters on this machine, a local thinking model that was already thinking and was
/// writing `<think>` into the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reasoning {
    /// Never ask. A model that thinks anyway still has its thinking taken out of the
    /// answer -- it is simply not shown. Keeping it in would put `<think>` in the
    /// transcript and in the tool parser's way, which is what this whole change is about.
    Off,
    #[default]
    Auto,
    Low,
    Medium,
    High,
}

impl Reasoning {
    pub fn from_setting(key: &str) -> Reasoning {
        match key.trim().to_lowercase().as_str() {
            "off" | "none" | "false" => Reasoning::Off,
            "low" => Reasoning::Low,
            "medium" | "on" | "true" => Reasoning::Medium,
            "high" | "max" => Reasoning::High,
            // Including the empty string and anything unrecognised: a setting nobody has
            // touched, or one written by a newer build, should leave the sensible default
            // in place rather than silently turning reasoning off.
            _ => Reasoning::Auto,
        }
    }

    pub fn as_key(self) -> &'static str {
        match self {
            Reasoning::Off => "off",
            Reasoning::Auto => "auto",
            Reasoning::Low => "low",
            Reasoning::Medium => "medium",
            Reasoning::High => "high",
        }
    }

    /// Whether the operator wants to see reasoning at all. False only for `Off`.
    pub fn shown(self) -> bool {
        self != Reasoning::Off
    }

    /// The effort level, for providers that take one. None under `Auto` on purpose: the
    /// field itself is what an older server rejects, so the safe default is to send none
    /// and keep whatever the model does by default.
    pub fn effort(self) -> Option<&'static str> {
        match self {
            Reasoning::Low => Some("low"),
            Reasoning::Medium => Some("medium"),
            Reasoning::High => Some("high"),
            Reasoning::Off | Reasoning::Auto => None,
        }
    }

    /// Anthropic's thinking budget in tokens. Its minimum is 1024, and the budget has to
    /// leave room under `max_tokens` for the answer itself -- a budget equal to the cap is
    /// a reply that thinks and then has nothing left to say with.
    fn anthropic_budget(self) -> Option<u32> {
        match self {
            Reasoning::Low => Some(1024),
            Reasoning::Medium => Some(4096),
            Reasoning::High => Some(6144),
            Reasoning::Off | Reasoning::Auto => None,
        }
    }

    /// Gemini's `thinkingBudget`, in the same spirit.
    fn gemini_budget(self) -> Option<i64> {
        match self {
            Reasoning::Low => Some(1024),
            Reasoning::Medium => Some(8192),
            Reasoning::High => Some(24576),
            Reasoning::Off | Reasoning::Auto => None,
        }
    }
}

/// Pulls the reasoning out of an answer that has it written inline.
///
/// Streaming is the whole difficulty: `<think>` can arrive split across two deltas, so the
/// same trick `FenceFilter` uses applies -- hold back anything that could still turn out to
/// be the beginning of a tag, and release it once it cannot. Getting this wrong in the
/// cheap way (strip the tags at the end of the turn) would put the reasoning on screen as
/// the answer for as long as the model took to think, which on a 7B is most of the turn.
#[derive(Default)]
struct ThoughtSplitter {
    holding: String,
    /// Which pair of tags is open, as an index into `THOUGHT_TAGS`, so the closer looked
    /// for is the one belonging to the opener that was actually seen.
    inside: Option<usize>,
}

/// One delta, taken apart.
#[derive(Default, PartialEq, Debug)]
struct Split {
    answer: String,
    thought: String,
}

impl ThoughtSplitter {
    fn push(&mut self, delta: &str) -> Split {
        self.holding.push_str(delta);
        let mut out = Split::default();

        loop {
            match self.inside {
                Some(open) => {
                    let close = THOUGHT_TAGS[open].1;
                    match self.holding.find(close) {
                        Some(end) => {
                            out.thought.push_str(&self.holding[..end]);
                            self.holding = self.holding[end + close.len()..].to_string();
                            self.inside = None;
                        }
                        None => {
                            let keep = partial_tag_len(&self.holding, &[close]);
                            let release = self.holding.len() - keep;
                            out.thought.push_str(&self.holding[..release]);
                            self.holding = self.holding[release..].to_string();
                            return out;
                        }
                    }
                }
                None => {
                    let found = THOUGHT_TAGS
                        .iter()
                        .enumerate()
                        .filter_map(|(index, (open, _))| {
                            self.holding.find(open).map(|at| (at, index))
                        })
                        .min();
                    match found {
                        Some((at, index)) => {
                            out.answer.push_str(&self.holding[..at]);
                            self.holding =
                                self.holding[at + THOUGHT_TAGS[index].0.len()..].to_string();
                            self.inside = Some(index);
                        }
                        None => {
                            let openers: Vec<&str> =
                                THOUGHT_TAGS.iter().map(|(open, _)| *open).collect();
                            let keep = partial_tag_len(&self.holding, &openers);
                            let release = self.holding.len() - keep;
                            out.answer.push_str(&self.holding[..release]);
                            self.holding = self.holding[release..].to_string();
                            return out;
                        }
                    }
                }
            }
        }
    }

    /// Whatever is still held when the model stops talking.
    ///
    /// An unterminated block counts as reasoning rather than as answer. A thinking model
    /// cut off mid-thought has produced no answer at all, and releasing half a thought into
    /// the reply -- which is what dropping this on the answer side would do -- is how
    /// `<think>` reached the screen in the first place.
    fn finish(&mut self) -> Split {
        let held = std::mem::take(&mut self.holding);
        if self.inside.is_some() {
            Split {
                answer: String::new(),
                thought: held,
            }
        } else {
            Split {
                answer: held,
                thought: String::new(),
            }
        }
    }
}

/// The longest proper prefix of any of `tags` that is a suffix of `text`.
///
/// Every tag is ASCII, so a suffix that matches one ends on a character boundary and the
/// caller can slice at it -- the same reason `partial_fence_len` can do this.
fn partial_tag_len(text: &str, tags: &[&str]) -> usize {
    let bytes = text.as_bytes();
    let longest = tags.iter().map(|tag| tag.len()).max().unwrap_or(0);
    for len in (1..longest).rev() {
        if bytes.len() < len {
            continue;
        }
        let tail = &bytes[bytes.len() - len..];
        if tags
            .iter()
            .any(|tag| tag.len() > len && tag.as_bytes()[..len] == *tail)
        {
            return len;
        }
    }
    0
}

/// The two things a model says, on their way to two different places.
///
/// Every provider writes through this rather than straight to a sink, which is what makes
/// "the reasoning is not the answer" true in one place instead of four. Text handed to
/// `say` goes through the splitter, so a model that writes `<think>` inline is divided
/// exactly like one whose server reports thinking in a field of its own -- and the
/// `thinking` channel exists for the servers that do.
///
/// It accumulates as well as forwarding, so the completion's text is by construction the
/// same text the sink saw. Keeping the two in step by hand is what went wrong often enough
/// elsewhere in this file to be worth designing out.
pub struct Relay<'a> {
    answer: Option<Sink<'a>>,
    thought: Option<Sink<'a>>,
    /// Off entirely under `Reasoning::Off`: the thinking is still taken out of the answer,
    /// it just never reaches a sink or the completion.
    show: bool,
    split: ThoughtSplitter,
    text: String,
    reasoning: String,
}

impl<'a> Relay<'a> {
    pub fn new(answer: Sink<'a>, thought: Sink<'a>, show: bool) -> Relay<'a> {
        Relay {
            answer: Some(answer),
            thought: Some(thought),
            show,
            split: ThoughtSplitter::default(),
            text: String::new(),
            reasoning: String::new(),
        }
    }

    /// A relay with nowhere to stream to, for the blocking paths. It still splits, so a
    /// non-streamed reply from a thinking model is taken apart the same way.
    pub fn quiet(show: bool) -> Relay<'a> {
        Relay {
            answer: None,
            thought: None,
            show,
            split: ThoughtSplitter::default(),
            text: String::new(),
            reasoning: String::new(),
        }
    }

    /// Text from the provider's answer channel, which may have reasoning written into it.
    pub fn say(&mut self, delta: &str) {
        let split = self.split.push(delta);
        self.note_thought(&split.thought);
        if !split.answer.is_empty() {
            self.text.push_str(&split.answer);
            if let Some(sink) = self.answer.as_deref_mut() {
                sink(&split.answer);
            }
        }
    }

    /// Text the provider has already split out of its reasoning, pushed through without
    /// the splitter. Used on the non-streaming retry: that path's text came back from a
    /// relay of its own, so running it through this one's half-open state would be wrong.
    pub fn answered(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
        if let Some(sink) = self.answer.as_deref_mut() {
            sink(text);
        }
    }

    /// The answer so far, for deciding whether a failed stream can be retried.
    pub fn so_far(&self) -> &str {
        &self.text
    }

    /// Reasoning the provider reported in a field of its own -- Ollama's `thinking`,
    /// OpenAI's `reasoning_content`, Anthropic's `thinking_delta`, a Gemini thought part.
    /// It never goes through the splitter: it is already separated, and a `<think>` tag
    /// inside it would be the model quoting one.
    pub fn thinking(&mut self, delta: &str) {
        self.note_thought(delta);
    }

    fn note_thought(&mut self, delta: &str) {
        if delta.is_empty() || !self.show {
            return;
        }
        self.reasoning.push_str(delta);
        if let Some(sink) = self.thought.as_deref_mut() {
            sink(delta);
        }
    }

    /// Releases anything the splitter was holding and hands back both halves.
    ///
    /// Takes `&mut self` rather than consuming, because the providers are handed a relay by
    /// reference -- one relay spans the retry from the streaming path to the blocking one,
    /// so that an answer half-streamed before a failure is not counted twice.
    fn finish(&mut self) -> (String, String) {
        let tail = self.split.finish();
        if !tail.answer.is_empty() {
            self.text.push_str(&tail.answer);
            if let Some(sink) = self.answer.as_deref_mut() {
                sink(&tail.answer);
            }
        }
        let thought = tail.thought;
        self.note_thought(&thought);
        (
            std::mem::take(&mut self.text),
            std::mem::take(&mut self.reasoning),
        )
    }
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
    /// Sent only when the server has told us this model can think -- see
    /// `LocalFacts::thinking`, which comes off the same `/api/show` capabilities list the
    /// tool support is read from. Ollama rejects the field outright for a model without
    /// the capability ("does not support thinking"), so sending it blind would break every
    /// existing local setup to gain a feature on some of them.
    ///
    /// `Some(false)` is not the same as `None` and is the real off switch: a thinking
    /// model thinks by default, so turning reasoning off means saying so.
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
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
    /// Ollama's own channel for reasoning, filled when `think` was sent. A field beside
    /// `content` rather than inside it, which is the one thing that makes this provider's
    /// split trustworthy: no tag parsing is involved.
    #[serde(default)]
    thinking: String,
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

    /// The reasoning on this object, which is empty on every object of a stream but the
    /// ones the model was still thinking on.
    fn thought(&self) -> &str {
        self.message
            .as_ref()
            .map(|m| m.thinking.as_str())
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
        // From the probe rather than from the caller: `tools` above is decided the same
        // way, off the same cached body, so the two cannot disagree about one model.
        think: facts
            .thinking
            .unwrap_or(false)
            .then(|| ctx.reasoning.shown()),
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

    // Through a relay even with nowhere to stream to: a model that writes `<think>` into
    // its answer does it on the blocking path too, and the two paths must not disagree
    // about what the answer was.
    let mut relay = Relay::quiet(ctx.reasoning.shown());
    relay.thinking(response.thought());
    relay.say(response.text());
    finish_with(&mut relay, response.usage(), response.calls(0))
}

/// Ollama streams newline-delimited JSON rather than SSE: one object per token, each with
/// the incremental text under `message.content`.
pub fn stream_ollama(
    endpoint: &str,
    model: &str,
    ctx: &ChatContext,
    relay: &mut Relay,
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

    let mut usage = None;
    let mut calls: Vec<NativeCall> = Vec::new();
    for_each_line(response, |line| {
        let line = line.trim();
        if line.is_empty() {
            return Ok(true);
        }
        let chunk: OllamaResponse = serde_json::from_str(line)
            .map_err(|e| format!("unparseable chunk from Ollama: {e}"))?;
        // Thinking first: a thinking model streams its whole thought before the first word
        // of the answer, and both can land in one object on the boundary between them.
        relay.thinking(chunk.thought());
        relay.say(chunk.text());
        // Unlike OpenAI, a call is not fragmented across chunks -- it arrives whole, on
        // whichever object the model finished it on, so each one is appended as it lands.
        calls.extend(chunk.calls(calls.len()));
        // The counts ride on the last object of the stream, so this overwrites nothing.
        if let Some(reported) = chunk.usage() {
            usage = Some(reported);
        }
        Ok(true)
    })?;

    finish_with(relay, usage, calls)
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
    /// OpenAI's own name for how hard to think, which Groq and most gateways have adopted.
    /// Sent only at an explicit level and only to the providers that document it -- see
    /// `takes_reasoning_effort`. LM Studio is deliberately never asked: it is the local
    /// one, most likely to be an older build or a look-alike, and a 400 there costs a
    /// working setup to gain a field whose effect we cannot see anyway.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
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
    /// Two spellings because the shape was extended by other people: DeepSeek, vLLM, LM
    /// Studio and llama.cpp settled on `reasoning_content`, OpenRouter on `reasoning`.
    /// Reading both costs nothing and is the difference between a reasoning panel that
    /// works on a given server and one that does not.
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
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
    /// As on `OpenAiChoiceMessage`: both spellings in use.
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
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
    /// Whether the model can reason, read off the same `capabilities` list as `tools` and
    /// with the same three answers. Deliberately here rather than probed separately: it is
    /// the same request to the same endpoint, and two readers of one fact are two answers
    /// waiting to disagree.
    pub thinking: Option<bool>,
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
        tools: capability_of(&body, "tools"),
        thinking: capability_of(&body, "thinking"),
        context_tokens: context_length_of(&body),
    })
}

/// Whether an `/api/show` body says this model has one named capability -- `tools`, or
/// `thinking`.
///
/// Three answers, not two. `capabilities` present and containing the name is yes; present
/// and not containing it is a real no, and no amount of prompting will change it; absent
/// altogether is an older Ollama that does not report capabilities, which is not the same
/// as a no and must not be recorded as one.
///
/// For `thinking` the distinction is not only tidiness: Ollama refuses a request carrying
/// `think` for a model without the capability, so "did not say" has to mean "do not ask".
fn capability_of(body: &serde_json::Value, name: &str) -> Option<bool> {
    body.get("capabilities")
        .and_then(|c| c.as_array())
        .map(|list| list.iter().any(|c| c.as_str() == Some(name)))
}

/// Clears the cache, for a settings change that could have changed the answer -- pulling a
/// newer tag of the same name, or restarting Ollama after an upgrade.
pub fn forget_local_facts() {
    if let Ok(mut cache) = LOCAL_FACTS.lock() {
        cache.clear();
    }
    if let Ok(mut seen) = NO_NATIVE_TOOLS.lock() {
        seen.clear();
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

/// Which models the local server has in memory right now.
///
/// Ollama answers this on `/api/ps`, and it is the difference between a judgment that costs
/// nothing and one that costs a model load. The first desktop log of the Auto router timed
/// out on every message because nothing asked this question: the router picked the smallest
/// *installed* model, which on a machine with one large model resident meant reading a
/// second one off disk -- and, on a card with no room for both, evicting the model the
/// reply was about to use.
///
/// Empty rather than an error when the server does not answer, including for LM Studio,
/// which has no equivalent endpoint in the OpenAI-compatible surface this talks to. Empty
/// means "nothing known to be loaded", which only ever makes a caller more cautious.
/// Deliberately short-timeout: this runs in front of a reply, and a probe that stalls has
/// cost more than the knowledge is worth.
pub fn loaded_models(provider: Provider, endpoint: &str) -> Vec<String> {
    if provider != Provider::Ollama {
        return Vec::new();
    }
    let url = format!("{}/api/ps", endpoint.trim_end_matches('/'));
    let Some(body) = crate::net::get(&url, "the server was not asked what it has loaded")
        .ok()
        .and_then(|request| {
            request
                .config()
                .timeout_global(Some(std::time::Duration::from_millis(2500)))
                .build()
                .call()
                .ok()
        })
        .and_then(|response| response.into_body().read_json::<Value>().ok())
    else {
        return Vec::new();
    };
    body.get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
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
            reasoning_effort: takes_reasoning_effort(provider)
                .then(|| ctx.reasoning.effort())
                .flatten(),
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

/// Whether this provider documents `reasoning_effort` on a chat completion.
///
/// Gateways are included because the field is what a gateway is for: it passes the request
/// on to a provider that does understand it, and a gateway that does not simply ignores
/// unknown fields. LM Studio is excluded for the reason given on the field itself.
fn takes_reasoning_effort(provider: Provider) -> bool {
    matches!(
        provider,
        Provider::OpenAi | Provider::Groq | Provider::Gateway
    )
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

/// Whether a refusal is the provider objecting to `reasoning_effort` specifically.
///
/// Deliberately narrow. The field name has to be in the body, so a 400 about anything else
/// is left alone and reaches the operator as itself -- a blanket "retry 400s without the
/// extras" would hide a wrong model name behind a second identical failure.
/// Servers that answered a request carrying `tools` by rejecting the field.
///
/// Keyed on endpoint and model, like `LOCAL_FACTS`, and cleared by the same
/// `forget_local_facts` -- an operator who has changed something and reopened the hub is
/// asking for everything cached about their server to be dropped, and this is that.
static NO_NATIVE_TOOLS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<(String, String)>>,
> = std::sync::LazyLock::new(Default::default);

/// Whether this server has already said it cannot take a `tools` field for this model.
///
/// The question `native_tools_for` asks about an OpenAI-shaped server that has no
/// capability endpoint to probe. Unknown means yes: a server is asked rather than assumed
/// about, and it gets to say no once.
pub fn refused_tools(endpoint: &str, model: &str) -> bool {
    NO_NATIVE_TOOLS
        .lock()
        .map(|seen| seen.contains(&(endpoint.to_string(), model.to_string())))
        .unwrap_or(false)
}

fn remember_tool_refusal(endpoint: &str, model: &str) {
    if let Ok(mut seen) = NO_NATIVE_TOOLS.lock() {
        seen.insert((endpoint.to_string(), model.to_string()));
    }
}

/// Whether a refusal is the provider objecting to `tools` specifically.
///
/// As narrow as `refused_the_effort_field`, and for the same reason: the field name has to
/// be in the body, so a 400 about a wrong model name still reaches the operator as itself.
fn refused_the_tools_field(error: &str) -> bool {
    let lowered = error.to_lowercase();
    lowered.contains("tools")
        && (lowered.contains("400")
            || lowered.contains("unrecognized")
            || lowered.contains("unsupported")
            || lowered.contains("not supported")
            || lowered.contains("invalid"))
}

fn refused_the_effort_field(error: &str) -> bool {
    let lowered = error.to_lowercase();
    lowered.contains("reasoning_effort")
        && (lowered.contains("400")
            || lowered.contains("unrecognized")
            || lowered.contains("unsupported")
            || lowered.contains("not supported")
            || lowered.contains("invalid"))
}

/// Sends an OpenAI-shaped request, and sends it once more without the effort field if that
/// is what the provider objected to.
///
/// `reasoning_effort` is not a field every model behind an OpenAI-shaped API accepts: the
/// reasoning models take it and the ordinary ones reject the whole request for carrying
/// it. Which is which is a list that stops being true within a month, so it is not kept
/// here -- the provider is asked, and is allowed to say no once. Somebody who picked an
/// effort level and then a model that cannot use one should get a plainer answer, not a
/// conversation where every turn fails.
///
/// The cost when it happens is one refused request per turn. A model that cannot reason
/// also cannot be asked again later with a different answer, so remembering the refusal
/// would be better -- but remembering it wrongly, across a model the operator has since
/// changed, is how a reasoning model ends up silently never asked.
fn openai_send(
    provider: Provider,
    endpoint: &str,
    url: &str,
    api_key: &str,
    timeout: std::time::Duration,
    payload: &mut OpenAiRequest,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let sent = checked(openai_request(provider, url, api_key, timeout)?.send_json(&*payload));
    match sent {
        Err(refusal)
            if payload.reasoning_effort.is_some() && refused_the_effort_field(&refusal) =>
        {
            payload.reasoning_effort = None;
            checked(openai_request(provider, url, api_key, timeout)?.send_json(&*payload))
        }
        /* The same bargain for `tools`, and the reason LM Studio is allowed to be asked at
        all now. The refusal is remembered so it costs one request rather than one per
        turn, and so `native_tools_for` can put the next turn on the text protocol -- a
        prompt that says the tools are attached to the request, sent to a server that has
        just refused to carry them, describes a mechanism the model has no way to use. */
        Err(refusal) if payload.tools.is_some() && refused_the_tools_field(&refusal) => {
            remember_tool_refusal(endpoint, &payload.model);
            payload.tools = None;
            checked(openai_request(provider, url, api_key, timeout)?.send_json(&*payload))
        }
        other => other,
    }
}

pub fn call_openai_compatible(
    provider: Provider,
    endpoint: &str,
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
) -> Result<Completion, String> {
    let (url, mut payload) = openai_payload(provider, endpoint, model, ctx, false);

    let response: OpenAiResponse = openai_send(
        provider,
        endpoint,
        &url,
        api_key,
        CALL_TIMEOUT,
        &mut payload,
    )?
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
    let mut relay = Relay::quiet(ctx.reasoning.shown());
    if let Some(thought) = choice
        .message
        .reasoning_content
        .or(choice.message.reasoning)
    {
        relay.thinking(&thought);
    }
    relay.say(&choice.message.content.unwrap_or_default());
    let (text, reasoning) = relay.finish();
    if text.trim().is_empty() && calls.is_empty() && reasoning.trim().is_empty() {
        return Err("empty choices in response".to_string());
    }
    Ok(Completion {
        text: text.trim().to_string(),
        reasoning: reasoning.trim().to_string(),
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
    relay: &mut Relay,
) -> Result<Completion, String> {
    let (url, mut payload) = openai_payload(provider, endpoint, model, ctx, true);

    let response = openai_send(
        provider,
        endpoint,
        &url,
        api_key,
        STREAM_TIMEOUT,
        &mut payload,
    )?;

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
            if let Some(thought) = delta.reasoning_content.or(delta.reasoning) {
                relay.thinking(&thought);
            }
            if let Some(text) = delta.content {
                relay.say(&text);
            }
        }
        Ok(true)
    })?;

    finish_with(relay, usage, builder.finish()?)
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
    /// Carries `thinkingConfig` and nothing else, so it is absent unless reasoning was
    /// explicitly asked for. A `thinkingConfig` sent to a model without thinking is a 400,
    /// and the Gemini 2.5 models think by default anyway -- so under `Auto` the right move
    /// is to send nothing and read the thought parts if any arrive.
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<Value>,
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
    /// Gemini marks a reasoning part rather than giving it a field of its own: same `text`,
    /// with `thought: true` beside it. A part without the flag is the answer.
    #[serde(default)]
    thought: Option<bool>,
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
        generation_config: ctx.reasoning.gemini_budget().map(
            |budget| json!({"thinkingConfig": {"includeThoughts": true, "thinkingBudget": budget}}),
        ),
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

    let mut relay = Relay::quiet(ctx.reasoning.shown());
    let mut calls = Vec::new();
    for part in candidate.content.parts {
        let thought = part.thought == Some(true);
        if let Some(said) = part.text {
            if thought {
                relay.thinking(&said);
            } else {
                relay.say(&said);
            }
        }
        if let Some(call) = part.function_call {
            calls.push(NativeCall::from(call));
        }
    }

    finish_with(&mut relay, usage, calls)
}

/// `alt=sse` asks for Server-Sent Events; without it streamGenerateContent returns a
/// single JSON array, which can't be consumed incrementally.
pub fn stream_gemini(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    relay: &mut Relay,
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
                let thought = part.thought == Some(true);
                if let Some(said) = part.text.filter(|t| !t.is_empty()) {
                    if thought {
                        relay.thinking(&said);
                    } else {
                        relay.say(&said);
                    }
                }
                if let Some(call) = part.function_call {
                    calls.push(NativeCall::from(call));
                }
            }
        }
        Ok(true)
    })?;

    finish_with(relay, usage, calls)
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
    /// `{"type": "enabled", "budget_tokens": n}` when extended thinking was asked for.
    ///
    /// Not sent alongside tools, and that is a limitation rather than an oversight: with
    /// thinking on, the API requires every thinking block to be replayed on the next
    /// request *with the signature it came with*, and a tool round is exactly such a next
    /// request. Aether1's `Exchange` carries no signature, so enabling both would turn
    /// every tool-using turn into a 400 the operator cannot do anything about. Reasoning
    /// on the other six providers does not have this constraint; closing it here needs the
    /// signature captured and replayed, which is a change of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Value>,
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
    /// `thinking` blocks only -- the reasoning, in a field named after the block type
    /// rather than sharing `text` with the answer.
    #[serde(default)]
    thinking: Option<String>,
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
    /// `thinking_delta` only: one fragment of the reasoning.
    #[serde(default)]
    thinking: Option<String>,
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
        // Only without tools -- see the field's own note for why both at once cannot work
        // until thinking signatures are carried on an Exchange.
        thinking: ctx
            .tools
            .is_empty()
            .then(|| ctx.reasoning.anthropic_budget())
            .flatten()
            .map(|budget| json!({"type": "enabled", "budget_tokens": budget})),
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
    let mut relay = Relay::quiet(ctx.reasoning.shown());
    let mut calls = Vec::new();
    for block in &response.content {
        if let Some(call) = block.as_call() {
            calls.push(call);
        } else if let Some(thought) = &block.thinking {
            relay.thinking(thought);
        } else if let Some(chunk) = &block.text {
            relay.say(chunk);
        }
    }
    let (text, reasoning) = relay.finish();
    if text.trim().is_empty() && calls.is_empty() && reasoning.trim().is_empty() {
        return Err("empty content in response".to_string());
    }
    Ok(Completion {
        text: text.trim().to_string(),
        reasoning: reasoning.trim().to_string(),
        usage,
        calls,
    })
}

pub fn stream_anthropic(
    api_key: &str,
    model: &str,
    ctx: &ChatContext,
    relay: &mut Relay,
) -> Result<Completion, String> {
    let response = checked(
        anthropic_request(api_key, STREAM_TIMEOUT)?.send_json(anthropic_payload(model, ctx, true)),
    )?;

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
                } else if let Some(thought) = delta.thinking {
                    relay.thinking(&thought);
                } else if let Some(text) = delta.text {
                    relay.say(&text);
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
    finish_with(relay, usage, builder.finish())
}

// ----------------------------------------------------------- constrained judgment
//
// One short call to a local model whose answer is not prose but a shape: a fixed set of
// fields, each from a fixed set of values. The router in `llm::auto` asks a small model
// "what kind of question is this?" and needs the answer to be parseable every time,
// because a verdict it cannot read is a turn it cannot route.
//
// Three deliberate differences from a chat call, and they are the reason this is a
// function of its own rather than a flag on `ChatContext`:
//
//   * **The server does the constraining, not the prompt.** Ollama takes a JSON schema in
//     `format` and LM Studio takes one in `response_format`, and in both cases the server
//     restricts what the model is allowed to emit next. A prompt that merely *asks* for
//     JSON gets JSON from a 7B most of the time, and "most of the time" in front of every
//     reply is a fault report a week later.
//   * **No history, no persona, no tools, no stream.** The judgment is about the sentence
//     the operator just typed. Handing it the conversation makes it slower and its answer
//     dependent on what came before; handing it tools would let a model that is only
//     weighing up whether to search go and search.
//   * **Local only, and a short timeout.** This runs in front of every reply, so it has to
//     be cheap and it has to fail fast. A judge on a cloud provider would bill somebody per
//     message to decide whether to bill them per message; see `llm::auto`.
//
// Failure is always `Err`, never a guessed verdict. The router's answer to a failed
// judgment is to leave the operator's own choice alone, which is the one fallback that
// cannot surprise them.

/// How long a judgment may take, which is a question about the model's *state* and not
/// about the request.
///
/// The first real log from a desktop said `timeout: global` on every message: a twenty
/// second bound is ample for a model that is in memory and nowhere near enough for one that
/// has to be read off a disk first, and the old code could not tell the two apart because
/// it set the bound from the token budget. Each of these is a different question about who
/// is waiting and for what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// The model is already loaded. A small model answers a schema this size in well under
    /// a second, so anything past this bound means the server has gone away rather than
    /// that the model is slow -- and the cost of waiting is paid before the reply starts.
    Warm,
    /// The model has to be loaded first, and it is the model the reply was going to load
    /// anyway. The wait belongs to the reply, not to the router: judging does not add it,
    /// it only moves it earlier. Generous for that reason, and paid once per model.
    Loading,
    /// Nothing is in front of a reply: a tuning round is something somebody typed and is
    /// watching. The bound here means "the server has gone away", nothing finer.
    Unhurried,
}

impl Pace {
    fn timeout(self) -> std::time::Duration {
        std::time::Duration::from_secs(match self {
            Pace::Warm => 20,
            Pace::Loading => 120,
            Pace::Unhurried => 300,
        })
    }
}

/// Everything about one judgment except where to send it.
///
/// A struct rather than five more arguments, and the budget and the pace sit in it together
/// on purpose: a page-sized answer at a twenty-second bound is a call setting itself up to
/// fail, and pairing them here is what makes that visible at the call site.
pub struct Ask<'a> {
    pub system_prompt: &'a str,
    pub question: &'a str,
    pub schema: &'a Value,
    /// How many tokens the answer may take. Without a cap, a model that ignores the schema
    /// runs to the timeout producing nothing usable.
    pub budget: u32,
    pub pace: Pace,
}

/// Asks a local model for one structured judgment, with the schema enforced by the server.
///
/// `schema` is a JSON Schema object describing the whole answer. `budget` is how many tokens
/// the answer may take -- an explicit argument rather than one constant, because the two
/// callers are far apart: a router's verdict is four short fields (`VERDICT_TOKENS`), and a
/// rewritten page of instructions is a page. Without a cap at all, a model that ignores the
/// schema runs to the timeout producing nothing usable. Returns the parsed object, or an
/// error naming what went wrong -- the caller logs it and carries on unrouted.
///
/// Only the two local providers are accepted. The refusal for anything else is not a
/// limitation waiting to be lifted: the router exists partly to *avoid* cloud calls, and a
/// cloud judge would make the cheap half of the decision the expensive half.
pub fn judge(
    provider: Provider,
    endpoint: &str,
    model: &str,
    ask: Ask<'_>,
) -> Result<Value, String> {
    if model.trim().is_empty() {
        return Err("no model to judge with".to_string());
    }
    let text = match provider {
        Provider::Ollama => judge_ollama(endpoint, model, &ask)?,
        Provider::LmStudio => judge_openai(endpoint, model, &ask)?,
        other => return Err(format!("{other} cannot be used as a local judge")),
    };
    // A schema-constrained server still sometimes wraps the object in a fence or puts a
    // sentence in front of it, and a model with no grammar support at all ignores the
    // field entirely. Both end up here as text that is nearly JSON, so the object is cut
    // out rather than the whole string parsed -- and if there is no object in it, that is
    // an error and not a verdict.
    let body = text
        .find('{')
        .zip(text.rfind('}'))
        .filter(|(start, end)| end > start)
        .map(|(start, end)| &text[start..=end])
        .ok_or_else(|| format!("the judgment was not JSON: {}", first_line(&text)))?;
    let parsed: Value = serde_json::from_str(body)
        .map_err(|e| format!("the judgment was not JSON ({e}): {}", first_line(body)))?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Err("the judgment was JSON but not an object".to_string())
    }
}

/// The front of a model's answer, for an error message that has to fit on a line.
fn first_line(text: &str) -> String {
    text.trim()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

/// Ollama's own grammar-constrained output: a schema in `format`, not a request for JSON.
#[derive(Serialize)]
struct OllamaJudgeRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage>,
    stream: bool,
    format: &'a Value,
    options: OllamaJudgeOptions,
}

/// Temperature zero, because a judgment that differs between two identical questions is a
/// judgment nobody can check. `num_ctx` at the floor where it is sent at all: this request
/// is a system prompt and one sentence, and asking for a long window here would allocate a
/// KV cache the size of the chat's for a call that lasts a moment -- see
/// `local_context_window`. Left out entirely for a model already in memory, so that the
/// loaded instance is the one that answers; `judge_ollama` says why that matters.
#[derive(Serialize)]
struct OllamaJudgeOptions {
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u64>,
    /// A verdict is a handful of tokens. Without a cap, a model that ignores the schema
    /// runs on until the timeout and spends the whole budget saying nothing usable.
    num_predict: u32,
}

fn judge_ollama(endpoint: &str, model: &str, ask: &Ask<'_>) -> Result<String, String> {
    let request = OllamaJudgeRequest {
        model,
        messages: vec![
            OllamaMessage::plain("system", ask.system_prompt.to_string()),
            OllamaMessage::plain("user", ask.question.to_string()),
        ],
        stream: false,
        format: ask.schema,
        options: OllamaJudgeOptions {
            temperature: 0.0,
            /* Named only when the model is being loaded for this call anyway. Ollama keys a
            loaded instance partly on its context size, so naming a different `num_ctx` for
            a model that is already in memory makes it load a *second* copy of the same
            model -- which on a card with one large model on it means evicting the one the
            reply is about to use, to decide where to send the reply. See
            `local_context_window` and the server-wide cliff #249 walked into. */
            num_ctx: (ask.pace != Pace::Warm).then_some(LOCAL_CTX_FLOOR),
            num_predict: ask.budget,
        },
    };
    let response: OllamaResponse = checked(
        crate::net::post(&ollama_url(endpoint), "no judgment was asked for")?
            .config()
            .timeout_global(Some(ask.pace.timeout()))
            .http_status_as_error(false)
            .build()
            .send_json(request),
    )?
    .body_mut()
    .read_json()
    .map_err(|e| e.to_string())?;
    Ok(response.text().to_string())
}

/// How many tokens a router's verdict may take. Three or four short fields.
pub const VERDICT_TOKENS: u32 = 120;

/// The OpenAI-compatible spelling of the same thing, for LM Studio. `json_schema` rather
/// than `json_object`: the looser mode constrains the answer to *some* object, which for a
/// router means a verdict with the wrong field names in it.
#[derive(Serialize)]
struct OpenAiJudgeRequest<'a> {
    model: &'a str,
    messages: Vec<Value>,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
    response_format: Value,
}

fn judge_openai(endpoint: &str, model: &str, ask: &Ask<'_>) -> Result<String, String> {
    let (base, _) = openai_defaults(Provider::LmStudio, endpoint);
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let request = OpenAiJudgeRequest {
        model,
        messages: vec![
            json!({"role": "system", "content": ask.system_prompt}),
            json!({"role": "user", "content": ask.question}),
        ],
        stream: false,
        temperature: 0.0,
        max_tokens: ask.budget,
        response_format: json!({
            "type": "json_schema",
            "json_schema": {"name": "verdict", "strict": true, "schema": ask.schema},
        }),
    };
    let response: Value = checked(
        crate::net::post(&url, "no judgment was asked for")?
            .config()
            .timeout_global(Some(ask.pace.timeout()))
            .http_status_as_error(false)
            .build()
            .send_json(request),
    )?
    .body_mut()
    .read_json()
    .map_err(|e| e.to_string())?;
    response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the server answered without a message".to_string())
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
            reasoning: Reasoning::Off,
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
            reasoning: Reasoning::Off,
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

    /// A relay carrying `said`, as a provider would have filled it.
    fn relayed(said: &str) -> Relay<'static> {
        let mut relay = Relay::quiet(true);
        relay.say(said);
        relay
    }

    #[test]
    fn an_empty_stream_is_an_error_not_an_empty_reply() {
        assert!(finish_with(&mut relayed("   \n "), None, Vec::new()).is_err());
        assert_eq!(
            finish_with(&mut relayed(" hello "), None, Vec::new())
                .unwrap()
                .text,
            "hello"
        );
    }

    /// The failure this sits on: a thinking model whose stream ends after `</think>` has
    /// produced no answer, and reporting that as "no content" throws away the one thing it
    /// did produce.
    #[test]
    fn a_turn_that_was_only_thinking_is_not_an_empty_stream() {
        let completion = finish_with(
            &mut relayed("<think>nearly there</think>"),
            None,
            Vec::new(),
        )
        .unwrap();
        assert!(completion.text.is_empty());
        assert_eq!(completion.reasoning, "nearly there");
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
            reasoning: Reasoning::Off,
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
            reasoning: Reasoning::Off,
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
            reasoning: Reasoning::Off,
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
        let completion = finish_with(&mut Relay::quiet(true), None, vec![call]).unwrap();
        assert!(completion.text.is_empty());
        assert_eq!(completion.calls.len(), 1);

        assert!(finish_with(&mut Relay::quiet(true), None, Vec::new()).is_err());
    }

    // ------------------------------------------------------- reasoning

    /// Feeds `deltas` through a splitter one at a time, the way a stream arrives.
    fn split_stream(deltas: &[&str]) -> (String, String) {
        let mut splitter = ThoughtSplitter::default();
        let mut answer = String::new();
        let mut thought = String::new();
        for delta in deltas {
            let out = splitter.push(delta);
            answer.push_str(&out.answer);
            thought.push_str(&out.thought);
        }
        let tail = splitter.finish();
        answer.push_str(&tail.answer);
        thought.push_str(&tail.thought);
        (answer, thought)
    }

    #[test]
    fn an_inline_thought_is_taken_out_of_the_answer() {
        let (answer, thought) =
            split_stream(&["<think>the user wants F1 results</think>Lando won."]);
        assert_eq!(answer, "Lando won.");
        assert_eq!(thought, "the user wants F1 results");
    }

    /// The reason this cannot be done by stripping tags at the end of the turn: on a 7B the
    /// thought is most of the turn, and a tag split across two deltas is the normal case,
    /// not an edge one. Tokenizers break `<think>` into `<`, `think`, `>` routinely.
    #[test]
    fn a_tag_split_across_deltas_is_still_a_tag() {
        let (answer, thought) = split_stream(&["<", "thi", "nk", ">", "hmm", "</", "think", ">ok"]);
        assert_eq!(answer, "ok", "no part of the tag may reach the answer");
        assert_eq!(thought, "hmm");
    }

    #[test]
    fn the_longer_spelling_is_recognised_too() {
        let (answer, thought) = split_stream(&["<thinking>so</thinking>done"]);
        assert_eq!(answer, "done");
        assert_eq!(thought, "so");
    }

    /// Text before a thought is answer text, and it must be released as it arrives rather
    /// than held until the thought closes -- otherwise the operator watches a blank reply.
    #[test]
    fn prose_either_side_of_a_thought_is_answer() {
        let mut splitter = ThoughtSplitter::default();
        let first = splitter.push("One moment. <thi");
        assert_eq!(first.answer, "One moment. ", "held only the partial tag");
        let rest = splitter.push("nk>work</think> Done.");
        assert_eq!(rest.answer, " Done.");
        assert_eq!(rest.thought, "work");
    }

    /// A thought that never closes is a stream that was cut off mid-thought. Releasing it
    /// into the answer is exactly the bug this exists to prevent.
    #[test]
    fn an_unclosed_thought_never_becomes_the_answer() {
        let (answer, thought) = split_stream(&["<think>halfway through"]);
        assert!(answer.is_empty(), "{answer:?}");
        assert_eq!(thought, "halfway through");
    }

    /// Nothing to do, and nothing held back: the overwhelming majority of replies.
    #[test]
    fn an_answer_with_no_thought_in_it_passes_through_whole() {
        let (answer, thought) = split_stream(&["Hello", " there", "."]);
        assert_eq!(answer, "Hello there.");
        assert!(thought.is_empty());
    }

    /// The interaction the coordinator asked to be sure of, and the reason the splitter runs
    /// *before* the fence filter rather than after it.
    ///
    /// A thinking model reasons about whether to call a tool, and reasoning about a tool
    /// call means writing one. If that reached the tool protocol, two things would break at
    /// once: `FenceFilter` would open on the fence inside the thought and swallow the real
    /// answer that follows, and `parse_round` would run a call the model was only weighing.
    #[test]
    fn a_tool_fence_inside_a_thought_reaches_neither_the_filter_nor_the_parser() {
        let said = "<think>I could run ```tool\n{\"tool\": \"search_web\"}\n``` but I know \
                    this one.</think>Paris.";

        let (answer, thought) = split_stream(&[said]);
        assert_eq!(answer, "Paris.", "the answer must survive intact");
        assert!(thought.contains("search_web"), "{thought:?}");

        // What the operator sees, and what the parser is handed, are both built from the
        // answer side alone.
        let mut filter = crate::tools::protocol::FenceFilter::new();
        let mut shown = filter.push(&answer);
        shown.push_str(&filter.finish());
        assert_eq!(shown, "Paris.");

        let round = crate::tools::protocol::parse_round(&answer);
        assert!(round.calls.is_empty(), "{round:#?}");
        assert!(round.malformed.is_empty(), "{round:#?}");

        // And the same text, unsplit, is what used to happen: the call runs.
        let unsplit = crate::tools::protocol::parse_round(said);
        assert_eq!(
            unsplit.calls.len(),
            1,
            "if this stops being true the test above proves nothing"
        );
    }

    /// A real tool call, outside any thought, still has to work -- the splitter is not
    /// allowed to buy safety by eating calls.
    #[test]
    fn a_tool_fence_outside_a_thought_still_reaches_the_parser() {
        let (answer, _) = split_stream(&[
            "<think>I should look.</think>Checking.\n```tool\n{\"tool\": \"get_time\"}\n```",
        ]);
        let round = crate::tools::protocol::parse_round(&answer);
        assert_eq!(round.calls.len(), 1, "{round:#?}");
        assert_eq!(round.calls[0].tool, "get_time");
    }

    #[test]
    fn with_reasoning_off_the_thought_is_still_removed_but_not_kept() {
        let mut relay = Relay::quiet(false);
        relay.say("<think>working</think>Answer.");
        let (answer, reasoning) = relay.finish();
        assert_eq!(answer, "Answer.");
        assert!(
            reasoning.is_empty(),
            "off means not shown, but never means left in the answer"
        );
    }

    #[test]
    fn the_setting_round_trips_and_an_unknown_word_leaves_the_default() {
        for key in ["off", "auto", "low", "medium", "high"] {
            assert_eq!(Reasoning::from_setting(key).as_key(), key);
        }
        assert_eq!(Reasoning::from_setting("OFF"), Reasoning::Off);
        // Including the empty string: a setting nobody has saved must not read as "off".
        assert_eq!(Reasoning::from_setting(""), Reasoning::Auto);
        assert_eq!(Reasoning::from_setting("enthusiastically"), Reasoning::Auto);
    }

    /// The field that breaks a working local setup if it is sent carelessly: Ollama refuses
    /// `think` outright for a model without the capability.
    #[test]
    fn ollama_is_only_asked_to_think_when_its_server_says_the_model_can() {
        let base = with_tools(&[], &[]);
        let can_think = LocalFacts {
            thinking: Some(true),
            ..LocalFacts::default()
        };
        let cannot = LocalFacts {
            thinking: Some(false),
            ..LocalFacts::default()
        };
        // An older Ollama that reports no capabilities at all. Not the same as a no, and
        // the field still must not be sent -- it would be refused.
        let did_not_say = LocalFacts::default();

        let asked = ChatContext {
            reasoning: Reasoning::High,
            ..base
        };
        for facts in [cannot, did_not_say] {
            assert_eq!(
                ollama_payload("qwen3", &asked, true, facts).think,
                None,
                "a model not known to think must never be sent the field"
            );
        }

        let automatic = ChatContext {
            reasoning: Reasoning::Auto,
            ..base
        };
        assert_eq!(
            ollama_payload("qwen3", &automatic, true, can_think).think,
            Some(true)
        );

        // And `false` rather than nothing when it is switched off: a thinking model thinks
        // by default, so turning it off means saying so.
        let silenced = ChatContext {
            reasoning: Reasoning::Off,
            ..base
        };
        assert_eq!(
            ollama_payload("qwen3", &silenced, true, can_think).think,
            Some(false)
        );
    }

    #[test]
    fn ollamas_own_thinking_field_is_read_as_reasoning() {
        let chunk: OllamaResponse =
            serde_json::from_str(r#"{"message":{"content":"Hi","thinking":"who is asking"}}"#)
                .unwrap();
        let mut relay = Relay::quiet(true);
        relay.thinking(chunk.thought());
        relay.say(chunk.text());
        assert_eq!(
            relay.finish(),
            ("Hi".to_string(), "who is asking".to_string())
        );
    }

    /// The failure this exists to stop: an operator picks an effort level, then a model
    /// that cannot use one, and every reply fails for carrying a field they never typed.
    #[test]
    fn a_model_that_rejects_the_effort_field_is_asked_again_without_it() {
        assert!(refused_the_effort_field(
            "http status: 400 -- {\"error\":{\"message\":\"Unrecognized request argument \
             supplied: reasoning_effort\"}}"
        ));
        assert!(refused_the_effort_field(
            "http status: 400 -- reasoning_effort is not supported with this model"
        ));
    }

    /// Narrow on purpose. A blanket "retry a 400 without the extras" would hide a wrong
    /// model name behind a second identical failure, and the body naming the model is the
    /// most useful thing the operator gets.
    #[test]
    fn any_other_refusal_is_left_to_reach_the_operator() {
        assert!(!refused_the_effort_field(
            "http status: 404 -- model gpt-4o-mimi does not exist"
        ));
        assert!(!refused_the_effort_field(
            "http status: 400 -- invalid value for temperature"
        ));
        // The field named without a refusal around it is not a refusal of the field.
        assert!(!refused_the_effort_field("reasoning_effort: high"));
    }

    /// Trident, 2026-10-26: "Honestly I don't think I have LM Studio installed" -- after
    /// being told "lmstudio rejected the request itself". The `lmstudio` key drives every
    /// OpenAI-compatible local server, and the scanner is careful to call them "Local
    /// server on port 1234"; the product guess was being made at the last moment, in the
    /// one place he reads.
    #[test]
    fn a_local_openai_server_is_not_called_a_product_it_might_not_be() {
        let said = explain_failure(
            Provider::LmStudio,
            "http://localhost:1234/v1",
            "http status: 400 -- something went wrong",
        );
        assert!(
            !said.to_lowercase().contains("lm studio") && !said.contains("lmstudio"),
            "it cannot know which program this is: {said}"
        );
        assert!(
            said.contains("http://localhost:1234/v1"),
            "the endpoint is the only thing that identifies it: {said}"
        );
    }

    /// A context too small for the system prompt is not a long conversation, and the
    /// operator cannot fix it from inside AETHER1 -- so the message has to say both.
    #[test]
    fn a_context_overflow_says_it_is_the_prompt_and_where_to_raise_it() {
        let said = explain_failure(
            Provider::LmStudio,
            "http://localhost:1234/v1",
            "http status: 400 -- request (5242 tokens) exceeds the available context size \
             (4096 tokens), try increasing it",
        );
        assert!(said.contains("prompt alone did not fit"), "{said}");
        assert!(said.contains("ctx-size"), "{said}");
        assert!(
            said.contains("5242"),
            "the server's own words still come through: {said}"
        );
    }

    /// An organisation-level Anthropic key names a header as its fix, and AETHER1 has
    /// nowhere to put a header. The actionable half is the other half of what Anthropic
    /// says, so the paraphrase has to lead with the key rather than with the header name.
    #[test]
    fn an_organisation_scoped_anthropic_key_is_told_to_use_a_workspace_key() {
        let said = explain_failure(
            Provider::Anthropic,
            "",
            "http status: 400 -- {\"type\":\"error\",\"error\":{\"type\":\
             \"invalid_request_error\",\"message\":\"This API key is not scoped to a \
             workspace, so this request must include the anthropic-workspace-id header \
             with the ID of the workspace to use.\"}}",
        );
        assert!(
            said.contains("create one inside a workspace"),
            "it says to get a different key, not to find a header field: {said}"
        );
        assert!(
            said.contains("anthropic-workspace-id"),
            "Anthropic's own words still come through: {said}"
        );
    }

    /// LM Studio is asked rather than assumed about now, so the refusal has to be
    /// recognised as narrowly as the effort field's -- a 400 naming a missing model must
    /// still reach the operator as itself rather than being swallowed as "no tools here".
    #[test]
    fn a_server_rejecting_the_tools_field_is_recognised() {
        assert!(refused_the_tools_field(
            "http status: 400 -- unrecognized request argument supplied: tools"
        ));
        assert!(refused_the_tools_field(
            "http status: 400 -- tools are not supported by this model"
        ));
        assert!(!refused_the_tools_field(
            "http status: 404 -- model qwythos-9b does not exist"
        ));
        // The word in prose, with nothing refusing anything, is not a refusal.
        assert!(!refused_the_tools_field("the model called two tools"));
    }

    /// Unknown has to mean yes, or this is the hardcoded "LM Studio cannot" all over
    /// again. The memo only ever turns to no, and only when a server has said so.
    #[test]
    fn a_server_that_has_not_refused_is_still_asked() {
        assert!(!refused_tools(
            "http://localhost:1234/v1",
            "never-seen-model"
        ));
        remember_tool_refusal("http://localhost:1234/v1", "a-model-that-said-no");
        assert!(refused_tools(
            "http://localhost:1234/v1",
            "a-model-that-said-no"
        ));
        // One model's no is not another's, and not another endpoint's.
        assert!(!refused_tools(
            "http://localhost:1234/v1",
            "a-different-model"
        ));
        assert!(!refused_tools(
            "http://elsewhere:1234/v1",
            "a-model-that-said-no"
        ));
        forget_local_facts();
        assert!(
            !refused_tools("http://localhost:1234/v1", "a-model-that-said-no"),
            "reopening the hub asks the server again"
        );
    }

    #[test]
    fn both_spellings_of_the_openai_reasoning_field_are_read() {
        let under_score: OpenAiStreamDelta =
            serde_json::from_str(r#"{"reasoning_content":"a"}"#).unwrap();
        assert_eq!(under_score.reasoning_content.as_deref(), Some("a"));
        let plain: OpenAiStreamDelta = serde_json::from_str(r#"{"reasoning":"b"}"#).unwrap();
        assert_eq!(plain.reasoning.as_deref(), Some("b"));
    }

    /// Only the providers that document the field, and never at `Auto`: the field itself is
    /// what an older OpenAI-compatible server rejects.
    #[test]
    fn reasoning_effort_goes_only_where_it_is_documented() {
        let asked = ChatContext {
            reasoning: Reasoning::High,
            ..with_tools(&[], &[])
        };
        for provider in [Provider::OpenAi, Provider::Groq, Provider::Gateway] {
            let (_, payload) = openai_payload(provider, "https://x/v1", "m", &asked, false);
            assert_eq!(
                payload.reasoning_effort,
                Some("high"),
                "{provider} documents reasoning_effort"
            );
        }
        let (_, local) = openai_payload(Provider::LmStudio, "http://x/v1", "m", &asked, false);
        assert_eq!(
            local.reasoning_effort, None,
            "LM Studio is never sent a field it might reject"
        );

        let automatic = ChatContext {
            reasoning: Reasoning::Auto,
            ..with_tools(&[], &[])
        };
        let (_, payload) = openai_payload(Provider::OpenAi, "https://x/v1", "m", &automatic, false);
        assert_eq!(payload.reasoning_effort, None, "auto asks for nothing");
    }

    /// Anthropic cannot be asked to think and to use tools in the same request until an
    /// Exchange carries thinking signatures -- see the field's own note. Pinned here so the
    /// combination cannot be switched on by accident.
    #[test]
    fn anthropic_is_not_asked_to_think_while_tools_are_on_the_request() {
        let tools = [a_tool()];
        let with = ChatContext {
            reasoning: Reasoning::Medium,
            ..with_tools(&tools, &[])
        };
        assert!(anthropic_payload("claude-opus-5", &with, true)
            .thinking
            .is_none());

        let without = ChatContext {
            reasoning: Reasoning::Medium,
            ..with_tools(&[], &[])
        };
        let thinking = anthropic_payload("claude-opus-5", &without, true)
            .thinking
            .expect("asked for, with no tools in the way");
        assert_eq!(thinking["type"], "enabled");
        let budget = thinking["budget_tokens"].as_u64().unwrap();
        assert!(
            budget < u64::from(ANTHROPIC_MAX_TOKENS),
            "a budget at the cap is a reply that thinks and then cannot answer"
        );
        assert!(budget >= 1024, "below Anthropic's minimum is a 400");
    }

    #[test]
    fn gemini_is_only_sent_a_thinking_config_when_one_was_asked_for() {
        let automatic = ChatContext {
            reasoning: Reasoning::Auto,
            ..with_tools(&[], &[])
        };
        assert!(gemini_payload(&automatic).generation_config.is_none());

        let asked = ChatContext {
            reasoning: Reasoning::Low,
            ..with_tools(&[], &[])
        };
        let config = gemini_payload(&asked).generation_config.unwrap();
        assert_eq!(config["thinkingConfig"]["includeThoughts"], true);
        assert!(config["thinkingConfig"]["thinkingBudget"].as_i64().unwrap() > 0);
    }

    #[test]
    fn a_gemini_thought_part_is_not_part_of_the_answer() {
        let candidate: GeminiCandidate = serde_json::from_str(
            r#"{"content":{"parts":[{"text":"weighing it","thought":true},{"text":"Yes."}]}}"#,
        )
        .unwrap();
        let mut relay = Relay::quiet(true);
        for part in candidate.content.parts {
            let text = part.text.unwrap_or_default();
            if part.thought == Some(true) {
                relay.thinking(&text);
            } else {
                relay.say(&text);
            }
        }
        assert_eq!(
            relay.finish(),
            ("Yes.".to_string(), "weighing it".to_string())
        );
    }

    #[test]
    fn an_anthropic_thinking_delta_is_not_part_of_the_answer() {
        let event: AnthropicStreamEvent = serde_json::from_str(
            r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"let me see"}}"#,
        )
        .unwrap();
        let delta = event.delta.unwrap();
        assert_eq!(delta.thinking.as_deref(), Some("let me see"));
        assert!(delta.text.is_none(), "the two never share a field");
    }

    /// The gate on sending `think`, read off `/api/show`.
    #[test]
    fn the_thinking_capability_is_read_from_what_the_server_lists() {
        let decode = |body: &str| -> Option<bool> {
            capability_of(&serde_json::from_str(body).unwrap(), "thinking")
        };
        assert_eq!(
            decode(r#"{"capabilities":["completion","tools","thinking"]}"#),
            Some(true)
        );
        assert_eq!(
            decode(r#"{"capabilities":["completion","tools"]}"#),
            Some(false),
            "a server that listed its capabilities and did not list thinking has said no"
        );
        assert_eq!(
            decode("{}"),
            None,
            "an older Ollama says nothing, which is not the same as no"
        );
    }

    /// Both local providers carry tools now. LM Studio was excluded on the guess that it
    /// would be an old build and reject the field; the cost of that guess was that no
    /// model behind it could call a tool however capable it was, and nothing said so. It
    /// is asked, and `refused_tools` remembers a no -- the same bargain the effort field
    /// is on. Offline stays off because there is no server to ask.
    #[test]
    fn every_provider_with_a_server_to_ask_is_asked_about_tools() {
        assert!(Provider::Ollama.supports_native_tools());
        assert!(Provider::LmStudio.supports_native_tools());
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
        assert_eq!(capability_of(&says_yes, "tools"), Some(true));
        assert_eq!(capability_of(&says_no, "tools"), Some(false));
        assert_eq!(capability_of(&silent, "tools"), None);
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
            reasoning: Reasoning::Off,
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
            reasoning: Reasoning::Off,
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

    /// The judgment payload for a model that is already in memory names no context size.
    ///
    /// Ollama keys a loaded instance partly on its context size, so a judgment that names
    /// its own `num_ctx` for a resident model makes the server load a second copy of that
    /// model -- which is both the slow path this is meant to avoid and, on a card with one
    /// large model on it, an eviction of the model the reply is about to use.
    #[test]
    fn a_warm_judgment_does_not_name_a_context_size_and_a_loading_one_does() {
        let schema = json!({"type": "object"});
        let payload = |pace| {
            serde_json::to_value(OllamaJudgeRequest {
                model: "qwen3:14b",
                messages: vec![],
                stream: false,
                format: &schema,
                options: OllamaJudgeOptions {
                    temperature: 0.0,
                    num_ctx: (pace != Pace::Warm).then_some(LOCAL_CTX_FLOOR),
                    num_predict: VERDICT_TOKENS,
                },
            })
            .expect("a request serialises")
        };
        assert!(
            payload(Pace::Warm).pointer("/options/num_ctx").is_none(),
            "the loaded instance is the one that should answer"
        );
        assert_eq!(
            payload(Pace::Loading).pointer("/options/num_ctx"),
            Some(&json!(LOCAL_CTX_FLOOR)),
            "a model being loaded for this call is loaded at the floor, not the chat's size"
        );
    }

    /// A pace is a claim about who is waiting, so the bounds have to be ordered that way.
    #[test]
    fn a_judgment_waiting_on_a_model_load_is_allowed_longer_than_one_that_is_not() {
        assert!(Pace::Warm.timeout() < Pace::Loading.timeout());
        assert!(Pace::Loading.timeout() < Pace::Unhurried.timeout());
    }
}
