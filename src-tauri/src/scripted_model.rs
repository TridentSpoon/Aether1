//! A model server that says what it is told to, one reply per round, and nothing else.
//!
//! The question this answers: what does a tool loop do when the model on the other end
//! behaves in one particular way -- asks twice, asks for the same thing for ever, writes a
//! result it was never given? Those are decisions about a conversation, and the only honest
//! way to test a decision about a conversation is to have one. Before this existed neither
//! loop had a test that went round even once, which is how the coding panel's loop came to
//! have four silent endings that the companion's had already fixed.
//!
//! It speaks Ollama's `/api/chat` and answers 404 to everything else, which is what an
//! Ollama too old to report capabilities does -- and that is read as "can call tools", so
//! a companion pointed here is on the native path.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

/// Serves `messages` in order, one per question, as the assistant's turn.
///
/// The last one is repeated, so a script that ends on a tool call is a model that never
/// stops asking. Returns the endpoint and a count of how many times it was asked, because
/// "stopped early" is a claim about that number.
pub fn serving(messages: Vec<Value>) -> (String, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // The whole request has to be read before answering, or closing the socket
            // with unread bytes in it resets the connection under the client.
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(read) = stream.read(&mut buf) {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                let text = String::from_utf8_lossy(&request);
                let Some(split) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..split]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if request.len() >= split + 4 + length {
                    break;
                }
            }

            // Only a question counts as a round. The provider also asks the server what
            // the model can do, and that must not use up a scripted reply.
            if !String::from_utf8_lossy(&request).starts_with("POST /api/chat") {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                continue;
            }

            let round = counter.fetch_add(1, Ordering::SeqCst);
            let message = messages.get(round).or(messages.last()).cloned().unwrap();
            let body = format!(
                "{}\n{}\n",
                json!({"message": message, "done": false}),
                json!({"message": {"role": "assistant", "content": ""}, "done": true}),
            );
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.flush();
        }
    });

    (format!("http://127.0.0.1:{port}"), asked)
}

/// The same, for a model that only ever writes text -- the fenced protocol's whole world.
pub fn saying(replies: &[&str]) -> (String, Arc<AtomicUsize>) {
    serving(replies.iter().map(|text| said(text)).collect())
}

/// An assistant turn that is prose and nothing else.
pub fn said(text: &str) -> Value {
    json!({"role": "assistant", "content": text})
}

/// An assistant turn that asks for one tool through the provider's own tool calling.
pub fn called(tool: &str, arguments: Value) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{"function": {"name": tool, "arguments": arguments}}],
    })
}
