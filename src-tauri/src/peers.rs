//! Pairing *outward*: this machine as the device that asks, rather than the one that is
//! asked.
//!
//! Everything before this made AETHER1 a good host. It hands out a one-time code, checks
//! it, mints a token for whoever typed it, and lists who is paired. The only thing that
//! ever *typed* a code was a browser on the other machine -- so the window could show a
//! code while having nothing that could answer one, which is what "multiple places to
//! present codes and the only place to submit one is linked to nothing" meant.
//!
//! This is the other half. Given a machine found by a scan (discovery.rs, surfaced in the
//! pane) and the code it is showing, this pairs with it from here and keeps what came back.
//!
//! Three decisions worth keeping:
//!
//! 1. **The certificate is trusted on first pairing and pinned afterwards.** A LAN server
//!    signs its own certificate -- no public authority will vouch for an address on your own
//!    network -- so there is nothing to check it against the first time. What there *is* is
//!    the next time: the fingerprint seen while pairing is stored with the token, and a
//!    later connection to that machine must present the same one or it is refused. That
//!    turns an unauthenticated first hop into a relationship that cannot be silently taken
//!    over, which is the same bargain SSH makes.
//! 2. **The request is written by hand over rustls rather than through the HTTP client.**
//!    ureq cannot both accept an unknown certificate and tell us which one it accepted, and
//!    the fingerprint is the entire point. One POST with a known body is a small enough
//!    thing to write out.
//! 3. **The token is stored, and nothing uses it yet.** Being paired is what this step
//!    delivers; routing work to a paired machine is a later one. The store is where that
//!    will read from.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, IpAddr as PkiIpAddr, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde::{Deserialize, Serialize};

/// How long to wait on the other machine at each stage. A machine on the same network
/// answers in milliseconds; this is a guard against one that has gone away mid-pairing,
/// not a realistic round trip.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

/// A cap on what is read back. The answer is a small JSON object or a line of text; anything
/// larger is a machine that is not what it said it was, and is not worth buffering.
const MAX_RESPONSE: u64 = 64 * 1024;

fn peers_path() -> PathBuf {
    crate::project_root()
        .join("backend")
        .join("serve_peers.json")
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// One machine this one has paired with, and what it takes to reach it again.
///
/// The token is here in the clear, unlike the *incoming* half in serve_auth.rs, which only
/// ever stores hashes. The difference is what the two are for: a server only has to
/// recognise a token it is shown, so it need not be able to reproduce one; a client has to
/// send it, so it must. This file carries a credential and is written 0600 for that reason.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Peer {
    /// What the other machine announced itself as. A label for a person, not an identity.
    pub name: String,
    pub address: String,
    pub port: u16,
    pub token: String,
    /// The certificate this machine presented while pairing, and must present again.
    pub fingerprint: String,
    pub paired_at: u64,
}

fn load_at(file: &Path) -> Vec<Peer> {
    // A store that cannot be read is treated as empty rather than as an error: nothing here
    // is load-bearing enough to stop the pane drawing, and pairing again rewrites it.
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_at(file: &Path, peers: &[Peer]) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not make the folder for the paired machines list: {e}"))?;
    }
    let text = serde_json::to_string_pretty(peers)
        .map_err(|e| format!("could not write the paired machines list: {e}"))?;
    std::fs::write(file, text)
        .map_err(|e| format!("could not write the paired machines list: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // It holds tokens in the clear, so it is readable by this account and nobody else --
        // the same promise serve_devices.json and the key file make.
        let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Every machine this one has paired with. The token is deliberately left out: a list is
/// something an operator might paste into a message asking for help.
pub fn paired_peers() -> Vec<Peer> {
    load_at(&peers_path())
}

/// Forgets one machine. Only this side is touched -- the other machine still lists the
/// device it gave a token to until somebody revokes it there, which is the same asymmetry
/// as deleting a saved password.
pub fn forget(address: &str, port: u16) -> Result<(), String> {
    forget_at(&peers_path(), address, port)
}

fn forget_at(file: &Path, address: &str, port: u16) -> Result<(), String> {
    let mut peers = load_at(file);
    peers.retain(|peer| !(peer.address == address && peer.port == port));
    save_at(file, &peers)
}

/// A certificate verifier that accepts one connection and remembers what it accepted.
///
/// With `expected` set it is not trusting at all: the certificate must hash to exactly that,
/// or the handshake fails. With it unset -- only ever on the first pairing with a machine --
/// it accepts what it is given and records the fingerprint so that every connection after
/// this one can be checked.
#[derive(Debug)]
struct PinnedCertificate {
    expected: Option<String>,
    seen: Mutex<Option<String>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = crate::serve_tls::fingerprint(end_entity.as_ref());
        if let Some(expected) = &self.expected {
            if expected != &fingerprint {
                return Err(rustls::Error::General(
                    "that machine is presenting a different certificate than the one it \
                     paired with. Either it was reinstalled, or something is answering in \
                     its place."
                        .to_string(),
                ));
            }
        }
        *self.seen.lock().unwrap() = Some(fingerprint);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The status line and body of one HTTP/1.1 answer.
///
/// Split out from the connection so the part that can be got wrong -- where the headers
/// stop -- is testable without a server on the other end.
fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "that machine sent an answer that stopped mid-way".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = String::from_utf8_lossy(&raw[split + 4..]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "that machine did not answer like a web server".to_string())?;
    Ok((status, body))
}

/// Pairs with a machine found on the network, and keeps what it gave back.
///
/// `secret` is the one-time code that machine is showing, or its twelve-word phrase --
/// whichever the operator has. This end does not care which: `/api/pair` tries the code
/// first and falls back to the phrase, so there is no wrong box to type it into.
/// One request to a machine on the network, over TLS whose certificate is checked against
/// `expected` when there is one and recorded either way.
///
/// Shared by pairing and by everything done with a token afterwards, so there is exactly one
/// place that decides what "trusted" means on this side of the wire.
fn request(
    address: &str,
    port: u16,
    expected: Option<String>,
    head: &str,
    body: Option<&str>,
) -> Result<(u16, String, String), String> {
    let ip: IpAddr = address
        .parse()
        .map_err(|_| format!("{address} is not an address this can connect to"))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedCertificate {
        expected,
        seen: Mutex::new(None),
        provider: provider.clone(),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("could not set up a secure connection: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();

    let socket = SocketAddr::new(ip, port);
    let stream = TcpStream::connect_timeout(&socket, NETWORK_TIMEOUT).map_err(|e| {
        format!("could not reach {address} on port {port}: {e}. Is LAN access still on over there?")
    })?;
    stream.set_read_timeout(Some(NETWORK_TIMEOUT)).ok();
    stream.set_write_timeout(Some(NETWORK_TIMEOUT)).ok();

    // The address is the name: there is nothing else to check it against, and the
    // fingerprint is what identifies the machine either way.
    let server_name = ServerName::IpAddress(PkiIpAddr::from(ip));
    let connection = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| format!("could not start a secure connection to {address}: {e}"))?;
    let mut tls = StreamOwned::new(connection, stream);

    let mut wire = head.to_string();
    if let Some(body) = body {
        wire.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    } else {
        wire.push_str("\r\n");
    }
    tls.write_all(wire.as_bytes())
        .map_err(|e| format!("could not reach {address}: {e}"))?;
    tls.flush().ok();

    let mut raw = Vec::new();
    // `Connection: close` means the answer ends when the stream does; a peer that stops
    // talking mid-answer shows up as a parse failure rather than as a hang, because the
    // socket has a read timeout.
    let mut limited = Read::take(&mut tls, MAX_RESPONSE);
    if let Err(e) = limited.read_to_end(&mut raw) {
        // A server that closes without a clean TLS shutdown is normal here and has already
        // given us everything it was going to say, so an error with a body in hand is not
        // worth failing over.
        if raw.is_empty() {
            return Err(format!("{address} stopped answering part-way through: {e}"));
        }
    }
    let (status, answer) = parse_response(&raw)?;
    let fingerprint =
        verifier.seen.lock().unwrap().clone().ok_or_else(|| {
            "the connection ended before that machine identified itself".to_string()
        })?;
    Ok((status, answer, fingerprint))
}

pub fn pair_with(name: &str, address: &str, port: u16, secret: &str) -> Result<Peer, String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("Type the code that machine is showing, or its pairing phrase.".to_string());
    }

    // A machine already paired with is pinned to the certificate it had. A new one is not,
    // and what it presents becomes the pin.
    let known = paired_peers()
        .into_iter()
        .find(|peer| peer.address == address && peer.port == port);

    let body = serde_json::json!({
        "phrase": secret,
        "device": device_label(),
    })
    .to_string();
    let head = format!(
        "POST /api/pair HTTP/1.1\r\nHost: {address}:{port}\r\n\
         Content-Type: application/json\r\nConnection: close\r\n"
    );
    let (status, answer, fingerprint) = request(
        address,
        port,
        known.as_ref().map(|peer| peer.fingerprint.clone()),
        &head,
        Some(&body),
    )?;

    match status {
        200 => {}
        401 | 400 => {
            return Err(if answer.trim().is_empty() {
                "That machine did not accept the code. Codes run out after ten minutes -- \
                 ask it for a new one."
                    .to_string()
            } else {
                answer.trim().to_string()
            })
        }
        429 => {
            return Err(
                "That machine has stopped accepting attempts for a minute. Wait, then try again."
                    .to_string(),
            )
        }
        other => {
            return Err(format!(
                "That machine answered with {other}: {}",
                answer.trim()
            ))
        }
    }

    let token = serde_json::from_str::<serde_json::Value>(&answer)
        .ok()
        .and_then(|value| value.get("token")?.as_str().map(str::to_string))
        .ok_or_else(|| "That machine paired but sent no token back.".to_string())?;

    let peer = Peer {
        name: name.trim().to_string(),
        address: address.to_string(),
        port,
        token,
        fingerprint,
        paired_at: now_seconds(),
    };
    let file = peers_path();
    let mut peers = load_at(&file);
    // Pairing again with a machine already here replaces it rather than adding a second
    // entry: the new token is the one that works, and the old one is now dead weight.
    peers.retain(|existing| !(existing.address == peer.address && existing.port == peer.port));
    peers.push(peer.clone());
    save_at(&file, &peers)?;
    Ok(peer)
}

/// Asks a machine already paired with what models it can run, using the token it gave.
///
/// The first thing the stored token is for. Until now being paired was a fact with no
/// consequence; this is the smallest honest use of it, and it answers the question the
/// operator actually has -- "what is that machine good for" -- while proving the credential
/// works. Routing a conversation to one of these models is the step after.
///
/// Pinned, with no first-use exception: this machine's certificate was recorded when it
/// paired, so anything else answering on its address is refused rather than trusted.
pub fn models_on(address: &str, port: u16) -> Result<Vec<String>, String> {
    let peer = paired_peers()
        .into_iter()
        .find(|peer| peer.address == address && peer.port == port)
        .ok_or_else(|| "This machine is not paired with that one.".to_string())?;

    let head = format!(
        "GET /api/scanner/status HTTP/1.1\r\nHost: {address}:{port}\r\n\
         Authorization: Bearer {}\r\nConnection: close\r\n",
        peer.token
    );
    let (status, answer, _) = request(address, port, Some(peer.fingerprint.clone()), &head, None)?;
    match status {
        200 => {}
        401 => {
            return Err(
                "That machine no longer accepts this one -- its pairing was revoked, or its \
                 phrase was replaced. Pair with it again."
                    .to_string(),
            )
        }
        other => {
            return Err(format!(
                "That machine answered with {other}: {}",
                answer.trim()
            ))
        }
    }
    Ok(models_in(&answer))
}

/// The setting naming the machine that answers when this one cannot. Empty means nobody,
/// which is the default: a question leaving this machine for another is something the
/// operator asks for, never something that starts happening by itself.
pub const HELPER_SETTING: &str = "lan_chat_peer";

/// The header a relayed question carries, and the thread-local that remembers one arriving.
///
/// Two machines can each name the other as their helper -- a perfectly reasonable thing for
/// an operator to set up, and without this a single question would bounce between them until
/// both gave up on a timeout. **Found by running it**, not by reading it: the first live
/// test pointed a machine at itself and the answer never came back.
///
/// So a question that arrived from another AETHER1 is answered here or not at all. One hop,
/// never two. The flag is thread-local because `/api/chat` answers on one blocking thread,
/// the same reason `vault::consulted` is.
pub const RELAY_HEADER: &str = "X-Aether1-Relayed";

thread_local! {
    static RELAYING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks this thread as answering for another machine until it is dropped.
pub struct Relaying;

impl Relaying {
    pub fn begin() -> Self {
        RELAYING.with(|flag| flag.set(true));
        Relaying
    }
}

impl Drop for Relaying {
    fn drop(&mut self) {
        RELAYING.with(|flag| flag.set(false));
    }
}

/// Whether this answer is being produced for another machine, and so must not be passed on
/// again.
pub fn is_relaying() -> bool {
    RELAYING.with(|flag| flag.get())
}

/// Asks a paired machine to answer a message, and gives back what it said.
///
/// This is the whole point of the network, in Trident's own words: basic chat answered by a
/// small model somewhere on it. The other machine runs its own engine, with its own persona
/// and history, so what comes back is an answer rather than a completion -- that is why this
/// posts to `/api/chat` rather than reaching for the model server behind it.
///
/// `session_id` is this machine's, passed through so the conversation over there stays one
/// conversation rather than a series of unrelated questions.
pub fn chat_on(
    address: &str,
    port: u16,
    message: &str,
    session_id: &str,
) -> Result<String, String> {
    let peer = paired_peers()
        .into_iter()
        .find(|peer| peer.address == address && peer.port == port)
        .ok_or_else(|| "This machine is not paired with that one.".to_string())?;

    let body = serde_json::json!({
        "message": message,
        "session_id": session_id,
        // The answer is read here, not spoken there: voice belongs to the machine the
        // operator is sitting at.
        "generate_voice": false,
    })
    .to_string();
    let head = format!(
        "POST /api/chat HTTP/1.1\r\nHost: {address}:{port}\r\nContent-Type: application/json\r\n\
         Authorization: Bearer {}\r\n{RELAY_HEADER}: 1\r\nConnection: close\r\n",
        peer.token
    );
    let (status, answer, _) = request(
        address,
        port,
        Some(peer.fingerprint.clone()),
        &head,
        Some(&body),
    )?;
    match status {
        200 => {}
        401 => {
            return Err(
                "That machine no longer accepts this one -- its pairing was revoked, or its \
                 phrase was replaced. Pair with it again."
                    .to_string(),
            )
        }
        other => {
            return Err(format!(
                "That machine answered with {other}: {}",
                answer.trim()
            ))
        }
    }
    reply_in(&answer)
}

/// The words out of a chat answer. A machine that answered with no reply in it is a failure
/// rather than an empty message: something is wrong over there, and an empty bubble would
/// hide it.
fn reply_in(answer: &str) -> Result<String, String> {
    serde_json::from_str::<serde_json::Value>(answer)
        .ok()
        .and_then(|value| value.get("reply")?.as_str().map(str::to_string))
        .filter(|reply| !reply.trim().is_empty())
        .ok_or_else(|| "That machine answered, but with nothing in it.".to_string())
}

/// The machine set to answer when this one cannot, as address and port, if it is still
/// paired with. A setting naming a machine that has since been forgotten answers nobody
/// rather than failing: forgetting is how an operator turns this off in the obvious way.
pub fn helper(setting: &str) -> Option<(String, u16)> {
    let (address, port) = setting.trim().rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    paired_peers()
        .into_iter()
        .find(|peer| peer.address == address && peer.port == port)
        .map(|peer| (peer.address, peer.port))
}

/// The model names out of a scan, in the order that machine reported them and without
/// repeats -- two servers on one machine often offer the same model, and a list that says it
/// twice reads as a mistake.
fn models_in(scan: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(scan) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for server in value["local_servers"].as_array().unwrap_or(&Vec::new()) {
        for model in server["models"].as_array().unwrap_or(&Vec::new()) {
            let Some(name) = model.as_str() else { continue };
            if !names.iter().any(|seen| seen == name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// What the other machine will list this one as. The hostname, because that is what its
/// operator will recognise in a list they may later revoke from.
fn device_label() -> String {
    match sysinfo::System::host_name() {
        Some(name) if !name.trim().is_empty() => format!("AETHER1 on {name}"),
        _ => "another AETHER1".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aether1-peers-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("serve_peers.json")
    }

    fn peer(address: &str) -> Peer {
        Peer {
            name: "kitchen-netbook".to_string(),
            address: address.to_string(),
            port: 8378,
            token: "abc123".to_string(),
            fingerprint: "AA BB".to_string(),
            paired_at: 1,
        }
    }

    #[test]
    fn a_missing_store_reads_as_nobody_paired_rather_than_failing() {
        assert!(load_at(&temp_file("missing")).is_empty());
    }

    #[test]
    fn a_stored_machine_comes_back_as_it_went_in() {
        let file = temp_file("round-trip");
        save_at(&file, &[peer("192.168.1.44")]).unwrap();
        assert_eq!(load_at(&file), vec![peer("192.168.1.44")]);
    }

    /// The token is a live credential, so the file it sits in must not be readable by
    /// anyone else on a shared machine.
    #[cfg(unix)]
    #[test]
    fn the_store_is_readable_by_this_account_alone() {
        use std::os::unix::fs::PermissionsExt;
        let file = temp_file("permissions");
        save_at(&file, &[peer("192.168.1.44")]).unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "group or others can read {file:?}");
    }

    #[test]
    fn forgetting_one_machine_leaves_the_others_alone() {
        let file = temp_file("forget");
        save_at(&file, &[peer("192.168.1.44"), peer("192.168.1.45")]).unwrap();
        forget_at(&file, "192.168.1.44", 8378).unwrap();
        assert_eq!(load_at(&file), vec![peer("192.168.1.45")]);
    }

    #[test]
    fn an_answer_is_split_at_the_blank_line_and_not_before() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"token\":\"x\"}";
        assert_eq!(
            parse_response(raw).unwrap(),
            (200, "{\"token\":\"x\"}".to_string())
        );
    }

    /// A body that itself contains a blank line must not be cut at the first one it holds.
    #[test]
    fn a_body_with_a_blank_line_in_it_survives_intact() {
        let raw = b"HTTP/1.1 400 Bad Request\r\n\r\nfirst\r\n\r\nsecond";
        assert_eq!(
            parse_response(raw).unwrap(),
            (400, "first\r\n\r\nsecond".to_string())
        );
    }

    #[test]
    fn an_answer_that_stops_before_its_headers_end_is_reported() {
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Ty").is_err());
    }

    #[test]
    fn something_that_is_not_a_web_server_is_reported_as_such() {
        assert!(parse_response(b"hello there\r\n\r\nbody").is_err());
    }

    #[test]
    fn model_names_come_out_of_a_scan_in_order_and_once_each() {
        let scan = r#"{"local_servers":[
            {"port":11434,"models":["qwen2.5:7b","llama3.2:3b"]},
            {"port":1234,"models":["qwen2.5:7b"]}
        ]}"#;
        assert_eq!(models_in(scan), vec!["qwen2.5:7b", "llama3.2:3b"]);
    }

    /// A machine that is serving but has nothing loaded is not an error, and must not be
    /// reported as one: "reached it, nothing loaded" is a different fact from "could not
    /// reach it".
    #[test]
    fn a_machine_with_nothing_loaded_reads_as_an_empty_list() {
        assert!(models_in(r#"{"local_servers":[{"port":11434,"models":[]}]}"#).is_empty());
        assert!(models_in("not json at all").is_empty());
        assert!(models_in("{}").is_empty());
    }

    /// The loop guard, which a live test found the need for: while an answer is being
    /// produced for another machine, this one must not pass the question on again.
    #[test]
    fn a_relayed_question_is_marked_for_as_long_as_it_is_being_answered() {
        assert!(!is_relaying(), "nothing is relayed until something says so");
        {
            let _relaying = Relaying::begin();
            assert!(is_relaying(), "answering for another machine");
        }
        assert!(!is_relaying(), "and not a moment longer");
    }

    #[test]
    fn a_reply_is_the_words_in_it_and_nothing_else() {
        assert_eq!(
            reply_in(r#"{"reply":"I can hear you.","session_id":"x"}"#).unwrap(),
            "I can hear you."
        );
    }

    /// An empty answer is a failure over there, not an empty message from a model. Passing
    /// it on would put a blank bubble on screen and hide whatever went wrong.
    #[test]
    fn an_answer_with_nothing_in_it_is_a_failure_rather_than_a_silence() {
        assert!(reply_in(r#"{"reply":"   "}"#).is_err());
        assert!(reply_in(r#"{"session_id":"x"}"#).is_err());
        assert!(reply_in("not json").is_err());
    }

    #[test]
    fn a_helper_setting_that_names_nobody_this_machine_knows_answers_nobody() {
        assert!(helper("").is_none());
        assert!(helper("not-an-address").is_none());
        assert!(helper("192.168.1.44:notaport").is_none());
        // A well-formed setting is deliberately not asserted here: whether it names a peer
        // depends on this machine's own store, which a test must not depend on.
    }

    #[test]
    fn a_machine_that_was_never_paired_with_is_not_asked_anything() {
        let outcome = models_on("203.0.113.7", 8378);
        assert!(outcome.unwrap_err().contains("not paired"));
    }

    #[test]
    fn an_empty_secret_never_reaches_the_network() {
        let outcome = pair_with("somewhere", "127.0.0.1", 1, "   ");
        assert!(outcome.unwrap_err().contains("Type the code"));
    }

    /// Also manual, and the other two halves of the same session: a refusal has to arrive
    /// in the other machine's own words, and a machine whose certificate has changed since
    /// pairing has to be refused rather than quietly re-trusted.
    ///
    ///   cargo test -- --ignored a_wrong_code_is_refused_in_the_other_machines_words
    ///   rm backend/serve_cert.pem backend/serve_key.pem   # then restart the server
    ///   cargo test -- --ignored a_changed_certificate_is_refused
    #[test]
    #[ignore = "needs a real --serve --lan on this machine"]
    fn a_wrong_code_is_refused_in_the_other_machines_words() {
        let outcome = pair_with("this machine", "127.0.0.1", 8378, "NOTTHECODE");
        let message = outcome.expect_err("a wrong code must not pair");
        assert!(
            message.contains("pairing code") || message.contains("pairing phrase"),
            "the other machine's own words, not ours: {message}"
        );
    }

    /// Also manual, and the point of the whole step: the token that came back is good for
    /// something. Run it straight after `pairing_with_a_real_server`, which leaves this
    /// machine paired with the server it just talked to.
    ///
    ///   cargo test -- --ignored asking_a_real_server_what_it_can_run
    #[test]
    #[ignore = "needs a real --serve --lan this machine has already paired with"]
    fn asking_a_real_server_what_it_can_run_uses_the_stored_token() {
        // Not an assertion about *which* models: a container has none, and a machine with
        // nothing loaded is a legitimate answer. What is asserted is that the request was
        // accepted at all, which is what the token is for.
        let models = models_on("127.0.0.1", 8378).expect("the server accepted the token");
        println!("models: {models:?}");
    }

    /// The point of the whole thing, and manual for the same reason as the rest: a question
    /// this machine could not answer, answered by another one over the token it gave.
    ///
    ///   cargo test -- --ignored asking_a_real_server_to_answer_a_question
    #[test]
    #[ignore = "needs a real --serve --lan this machine has already paired with"]
    fn asking_a_real_server_to_answer_a_question_gets_words_back() {
        let reply = chat_on("127.0.0.1", 8378, "Are you there?", "peer-test")
            .expect("the other machine answered");
        assert!(!reply.trim().is_empty(), "and it said something");
        println!("reply: {reply}");
    }

    #[test]
    #[ignore = "needs a real --serve --lan whose certificate has been replaced"]
    fn a_changed_certificate_is_refused() {
        let phrase = std::env::var("AETHER1_TEST_PHRASE").expect("AETHER1_TEST_PHRASE");
        let message = pair_with("this machine", "127.0.0.1", 8378, &phrase)
            .expect_err("a changed certificate must not pair");
        assert!(
            message.contains("different certificate"),
            "the pin is what should have stopped it: {message}"
        );
    }

    /// Not run in CI: it needs a real `aether1 --serve --lan` on this machine and its
    /// pairing phrase. Run it by hand:
    ///
    ///   aether1 pair                       # prints twelve words
    ///   aether1 --serve --lan &
    ///   AETHER1_TEST_PHRASE="the twelve words" \
    ///     cargo test -- --ignored pairing_with_a_real_server
    #[test]
    #[ignore = "needs a real --serve --lan on this machine"]
    fn pairing_with_a_real_server_stores_a_token_and_a_fingerprint() {
        let phrase = std::env::var("AETHER1_TEST_PHRASE").expect("AETHER1_TEST_PHRASE");
        let peer = pair_with("this machine", "127.0.0.1", 8378, &phrase).expect("pairing");
        assert!(!peer.token.is_empty(), "a token came back");
        assert!(!peer.fingerprint.is_empty(), "the certificate was recorded");
        assert!(
            paired_peers().iter().any(|p| p.token == peer.token),
            "and it was written down"
        );
    }
}
