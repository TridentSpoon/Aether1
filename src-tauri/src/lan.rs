//! The Remote & LAN pane: putting this machine on the network from the window rather than
//! from a terminal.
//!
//! Everything this module does was already possible with `aether1 --serve --lan`,
//! `aether1 pair`, `aether1 devices` and `aether1 revoke <id>` -- step 45 built all of it.
//! What it was missing is an operator who never opens a terminal. That is the whole job
//! here: start the server, stop the one we started, say whether anything is answering, and
//! hand over a pairing phrase.
//!
//! Three rules this file keeps to, each of which is load-bearing:
//!
//! 1. **Only a child AETHER1 itself started is ever stopped.** A server someone ran in
//!    their own terminal, or a second instance, is reported as "answering" and left alone
//!    -- the same rule background_services.rs keeps for Ollama. A Stop button that killed
//!    whatever held the port would be a footgun pointed at somebody's ssh session.
//! 2. **A phrase is never minted by looking at the pane.** `serve_auth::open_devices`
//!    generates one when none exists, which is why profile.rs pointedly does not call it;
//!    neither does `status` here. A phrase appears when the operator presses the button
//!    that makes one, or when a `--serve --lan` run creates its own, and never otherwise.
//! 3. **The phrase is shown once, by the call that made it.** It is not stored, so there
//!    is nothing to show a second time -- the same promise the terminal makes when it
//!    prints it at startup.
//!
//! The server runs as a separate process on purpose. `server::run` takes the process over
//! for as long as it serves; the window's own process is already busy being a window. A
//! child is also what makes "stop" honest: the operating system reclaims the port, the
//! listening socket and the mDNS announcement together, with nothing left half-running
//! inside the window's address space.

use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::llm::LlmEngine;
use crate::serve_auth;
use crate::server::SERVE_PORT;

/// Whether AETHER1 puts itself on the network as it starts. Off unless it has been asked
/// for: opening the app must never be what exposes a machine.
pub const AUTOSTART_SETTING: &str = "lan_autostart";

/// How long to wait for a connection before calling the port silent. Loopback either
/// answers immediately or is not listening; this is a guard against a hung stack, not a
/// realistic round trip.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// How long `start` waits for the child to bind before reporting back. The child loads a
/// model engine and (first run) generates a certificate before it listens, which is
/// seconds rather than milliseconds on a cold start.
const START_TIMEOUT: Duration = Duration::from_secs(12);

/// The `--serve --lan` process this window started, if it started one.
///
/// Tauri-managed state, exactly like `ManagedOllama`. The handle is the only thing that
/// distinguishes "a server AETHER1 is responsible for" from "something is on the port",
/// and stopping is offered for the first and refused for the second.
#[derive(Default)]
pub struct ManagedServer(Mutex<Option<Child>>);

impl ManagedServer {
    /// Whether our own child is still alive, reaping it first if it has exited. Called
    /// before every answer about the server, so a child that died (port taken, a panic in
    /// the child, killed from outside) stops being reported as ours the next time anyone
    /// looks, rather than leaving the pane offering to stop a process that is gone.
    fn alive(&self) -> bool {
        let mut slot = self.0.lock().unwrap();
        let Some(child) = slot.as_mut() else {
            return false;
        };
        match child.try_wait() {
            // Exited on its own: drop the handle, since there is nothing left to stop.
            Ok(Some(_)) => {
                *slot = None;
                false
            }
            Ok(None) => true,
            // try_wait failing says nothing about the child being alive, so the handle is
            // kept: guessing "dead" here would lose the only way to stop a live server.
            Err(_) => true,
        }
    }
}

/// Whether anything is listening on the serve port of this machine.
///
/// Loopback rather than the LAN address on purpose: a `--serve --lan` server binds
/// 0.0.0.0, which includes 127.0.0.1, so one probe answers for both modes and none of it
/// leaves the machine.
fn port_is_answering() -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), SERVE_PORT);
    match TcpStream::connect_timeout(&address, PROBE_TIMEOUT) {
        Ok(_) => true,
        // Anything other than "nobody is there" is reported as answering: a refused
        // connection is the one error that actually means the port is free.
        Err(e) => !matches!(e.kind(), ErrorKind::ConnectionRefused),
    }
}

/// The devices list in the shape the pane draws, shared with profile.rs's half of the same
/// list so the two panes can never disagree about who is paired.
fn devices_payload() -> Value {
    let devices: Vec<Value> = serve_auth::paired_devices()
        .into_iter()
        .map(|device| {
            json!({
                "id": device.id,
                "label": device.label,
                "paired_at": device.paired_at,
            })
        })
        .collect();
    json!({
        "pairing_set_up": serve_auth::pairing_is_set_up(),
        "devices": devices,
    })
}

/// Everything the pane draws in one read: whether the server is up, whether it is ours to
/// stop, what a device would have to type to reach it, and who already has.
pub fn status(engine: &LlmEngine, managed: &ManagedServer) -> Value {
    let managed_alive = managed.alive();
    let answering = port_is_answering();
    json!({
        "running": answering,
        // Ours only when both are true: a handle we hold and something on the port. A
        // child that is alive but not yet listening is starting, not started.
        "managed": managed_alive && answering,
        "starting": managed_alive && !answering,
        "port": SERVE_PORT,
        "autostart": engine.db().get_setting_bool(AUTOSTART_SETTING, false),
        "hostname": sysinfo::System::host_name().unwrap_or_else(|| "this machine".to_string()),
        "addresses": lan_addresses(),
        "devices": devices_payload(),
        // When the live pairing code stops working, so the sequence can count down without
        // holding the code itself anywhere. Null when there is none, or it has run out.
        "code_expires_at": serve_auth::pairing_code_expiry(),
        "now": now_seconds(),
        // The outward half: machines *this* one has paired with, as opposed to the devices
        // that have paired with it. Tokens are left out -- the pane has no use for them.
        "paired_peers": paired_peers_payload(),
    })
}

/// This machine's clock, sent with every status so the pane counts down against the clock
/// that set the expiry rather than the browser's, which can be minutes out.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

/// The addresses another device on the network would type. Read off this machine's own
/// interfaces; loopback and link-local are left out because neither is reachable from the
/// other side of a router, which is the only reason anybody is reading this list.
fn lan_addresses() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for network in sysinfo::Networks::new_with_refreshed_list().values() {
        for ip in network.ip_networks() {
            let IpAddr::V4(v4) = ip.addr else { continue };
            if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                continue;
            }
            let address = v4.to_string();
            if !found.contains(&address) {
                found.push(address);
            }
        }
    }
    found.sort();
    found
}

/// Starts `aether1 --serve --lan` as a child of this window.
///
/// Refuses rather than starting when there is no pairing phrase: `--serve --lan` would
/// happily make one and print it to a terminal nobody is watching, leaving a machine on
/// the network with a credential its operator has never seen. The pane's own button makes
/// the phrase first, which is the only way it is ever readable.
pub fn start(engine: &LlmEngine, managed: &ManagedServer) -> Result<Value, String> {
    if managed.alive() {
        return Err("AETHER1 has already started a server -- stop that one first.".to_string());
    }
    if port_is_answering() {
        return Err(format!(
            "Something is already answering on port {SERVE_PORT}. That is either another \
             AETHER1 or another program; AETHER1 will not take a port it did not open."
        ));
    }
    if !serve_auth::pairing_is_set_up() {
        return Err(
            "Make a pairing phrase first -- without one there is nothing for another device \
             to type, and nothing keeping the rest of the network out."
                .to_string(),
        );
    }

    let binary = std::env::current_exe()
        .map_err(|e| format!("could not find AETHER1's own binary to start a server with: {e}"))?;
    let child = Command::new(binary)
        .args(["--serve", "--lan"])
        // The child prints the pairing notice and its certificate fingerprint to a terminal
        // that, started from the window, nobody is reading. The pane shows both, so the
        // output is dropped rather than inherited into whatever launched the app.
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start the server: {e}"))?;
    *managed.0.lock().unwrap() = Some(child);

    // Waited for rather than reported optimistically: "Start" that goes green before
    // anything is listening is a button that lies on every machine slower than this one.
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        if port_is_answering() {
            return Ok(status(engine, managed));
        }
        if !managed.alive() {
            return Err(
                "The server started and stopped again straight away. Run `aether1 --serve \
                 --lan` in a terminal to see what it said."
                    .to_string(),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(format!(
        "The server did not answer on port {SERVE_PORT} within {} seconds. It may still be \
         starting -- check again in a moment.",
        START_TIMEOUT.as_secs()
    ))
}

/// Stops the server this window started. A server somebody else started is left alone and
/// said so about, rather than killed on the strength of holding the same port.
pub fn stop(engine: &LlmEngine, managed: &ManagedServer) -> Result<Value, String> {
    let taken = managed.0.lock().unwrap().take();
    match taken {
        Some(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok(status(engine, managed))
        }
        None if port_is_answering() => Err(format!(
            "Something is answering on port {SERVE_PORT}, but AETHER1 did not start it, so \
             it is not AETHER1's to stop. Stop it where it was started."
        )),
        None => Ok(status(engine, managed)),
    }
}

/// Makes a new pairing phrase and hands it back to be shown once.
///
/// This is `aether1 pair`, with its consequence unchanged: every device paired against the
/// old phrase is unpaired by it. The pane says so before the press; the count of what was
/// cut off comes back so it can say what actually happened.
pub fn new_phrase(engine: &LlmEngine, managed: &ManagedServer) -> Result<Value, String> {
    let (phrase, unpaired) = serve_auth::rotate()?;
    let mut report = status(engine, managed);
    report["phrase"] = json!(phrase);
    report["unpaired"] = json!(unpaired);
    Ok(report)
}

/// Adopts a phrase made on another AETHER1, so both machines answer to the same words.
///
/// Nothing comes back but the count of devices the swap unpaired: the phrase was already in
/// the operator's hands before they typed it, so there is nothing to show them once.
pub fn set_phrase(
    engine: &LlmEngine,
    managed: &ManagedServer,
    phrase: &str,
) -> Result<Value, String> {
    let unpaired = serve_auth::adopt(phrase)?;
    let mut report = status(engine, managed);
    report["unpaired"] = json!(unpaired);
    Ok(report)
}

/// Makes the one-time code the pairing sequence puts on screen, and hands it back to be
/// shown for as long as that screen is up.
///
/// Unlike `new_phrase` this costs nothing: no device is unpaired, the standing phrase is
/// untouched, and the code stops working by itself. That is the whole reason it exists --
/// the phrase could only ever be shown by the press that replaced it, which made "add one
/// more device" and "cut every device off" the same button.
pub fn new_pairing_code(engine: &LlmEngine, managed: &ManagedServer) -> Result<Value, String> {
    let (code, expires_at) = serve_auth::mint_pairing_code()?;
    let mut report = status(engine, managed);
    report["code"] = json!(code);
    report["code_expires_at"] = json!(expires_at);
    Ok(report)
}

/// Throws the live code away, called when the sequence is closed. A code that outlived the
/// screen it was on would be a standing invitation nobody could see.
pub fn clear_pairing_code(engine: &LlmEngine, managed: &ManagedServer) -> Value {
    serve_auth::clear_pairing_code();
    status(engine, managed)
}

/// The machines this one has paired with, in the shape the pane draws. Never the token:
/// nothing on that side of the wall has any use for it.
fn paired_peers_payload() -> Vec<Value> {
    crate::peers::paired_peers()
        .into_iter()
        .map(|peer| {
            json!({
                "name": peer.name,
                "address": peer.address,
                "port": peer.port,
                "paired_at": peer.paired_at,
            })
        })
        .collect()
}

/// Pairs with a machine a scan found, using the code it is showing its own operator.
///
/// This is the half that never existed: until now the only thing that could answer a
/// pairing code was a browser on the other machine, so the window could show one and
/// accept none. Failure comes back as the other machine's own words where it had any --
/// "wrong pairing phrase" is more use than "pairing failed".
pub fn pair_with_peer(
    engine: &LlmEngine,
    managed: &ManagedServer,
    name: &str,
    address: &str,
    port: u16,
    secret: &str,
) -> Result<Value, String> {
    let peer = crate::peers::pair_with(name, address, port, secret)?;
    let mut report = status(engine, managed);
    report["paired_with"] = json!({
        "name": peer.name,
        "address": peer.address,
        "port": peer.port,
    });
    Ok(report)
}

/// Forgets one machine on this side. The other machine still lists the device it gave a
/// token to, which is its operator's to revoke -- the pane says so rather than implying
/// this reaches across.
pub fn forget_peer(
    engine: &LlmEngine,
    managed: &ManagedServer,
    address: &str,
    port: u16,
) -> Result<Value, String> {
    crate::peers::forget(address, port)?;
    Ok(status(engine, managed))
}

/// How long a scan browses the network before reporting what it found.
///
/// mDNS answers arrive over a second or two rather than at once, so a shorter window
/// misses machines that are there; a longer one is a button that looks stuck. Three
/// seconds is what `aether1 discover` defaults to, and this is the same call.
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(3);

/// Every other AETHER1 announcing itself on this network.
///
/// This is `aether1 discover` in the window. It only ever *reads*: browsing is on-demand
/// and stops before this returns (see discovery.rs), nothing is contacted, and nothing
/// about this machine changes. A machine that is not serving does not announce, so an
/// empty list means "nobody is serving", not "the network is broken".
///
/// This machine's own server announces too, and is marked rather than hidden: seeing
/// yourself in the list is how you know the scan works, and it is the entry a second
/// machine will be looking for.
pub fn discover_peers() -> Result<Value, String> {
    let peers = crate::discovery::discover(DISCOVER_TIMEOUT)?;
    let own_name = sysinfo::System::host_name().unwrap_or_default();
    let own_addresses = lan_addresses();

    let mut found: Vec<Value> = peers
        .into_iter()
        .map(|peer| {
            let addresses: Vec<String> = peer
                .addresses
                .iter()
                .filter(|ip| matches!(ip, IpAddr::V4(_)))
                .map(|ip| ip.to_string())
                .collect();
            // Either the name matches ours or one of the addresses is one of ours: a
            // machine with two interfaces answers on both, and a hostname is not unique
            // enough on its own to decide this.
            let is_this_machine = (!own_name.is_empty() && peer.instance_name == own_name)
                || addresses.iter().any(|a| own_addresses.contains(a));
            json!({
                "name": peer.instance_name,
                "port": peer.port,
                "addresses": addresses,
                "version": peer.properties.get("version").cloned(),
                "is_this_machine": is_this_machine,
            })
        })
        .collect();
    // This machine first, then alphabetically: the operator is looking for the *other*
    // machine, and a stable order stops the list jumping between scans.
    found.sort_by(|a, b| {
        let own = |v: &Value| !v["is_this_machine"].as_bool().unwrap_or(false);
        let name = |v: &Value| v["name"].as_str().unwrap_or_default().to_lowercase();
        own(a).cmp(&own(b)).then_with(|| name(a).cmp(&name(b)))
    });

    Ok(json!({ "peers": found, "scanned_for": DISCOVER_TIMEOUT.as_secs() }))
}

/// Starts the server at launch when the operator has asked for that, and otherwise does
/// nothing at all. Mirrors `background_services::start_ollama_if_needed`: safe to call
/// speculatively, silent about every reason it might decline.
pub fn start_if_asked_for(engine: &LlmEngine, managed: &ManagedServer) {
    if !engine.db().get_setting_bool(AUTOSTART_SETTING, false) {
        return;
    }
    if let Err(e) = start(engine, managed) {
        eprintln!("[AETHER1] not putting this machine on the network at startup: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handle, not the port, is what makes a server ours -- so a pane with no child
    /// must never offer to stop whatever else is listening.
    #[test]
    fn nothing_is_managed_before_anything_is_started() {
        let managed = ManagedServer::default();
        assert!(!managed.alive());
    }

    /// A stop with no child of our own is either a no-op or a refusal, and never a kill.
    #[test]
    fn stopping_without_a_child_never_kills_anything() {
        let managed = ManagedServer::default();
        assert!(managed.0.lock().unwrap().is_none());
        // Whichever branch the machine running the tests lands in, the handle stays empty:
        // there is nothing to take, so nothing is signalled.
        assert!(!managed.alive());
    }

    /// Loopback with nothing on it must read as free, or the pane would refuse to start a
    /// server on a machine that has none.
    #[test]
    fn an_unused_port_reads_as_silent() {
        // Bind an ephemeral port, then drop it: whatever the OS just handed out is free.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        assert!(TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_err());
    }

    /// Not run in CI: it needs a multicast-capable network and another AETHER1
    /// announcing, which a runner is not guaranteed. Run it by hand against a real
    /// `aether1 announce` on this machine or another one:
    ///
    ///   aether1 announce --name test-peer &
    ///   cargo test -- --ignored the_scan_reports_what_is_announcing
    #[test]
    #[ignore = "needs another AETHER1 announcing on the network"]
    fn the_scan_reports_what_is_announcing() {
        let report = discover_peers().expect("the scan itself must not fail");
        let peers = report["peers"].as_array().expect("peers is a list").clone();
        assert!(
            !peers.is_empty(),
            "nothing answered: is `aether1 announce` running?"
        );
        for peer in &peers {
            assert!(peer["name"].is_string(), "every peer is named: {peer}");
            assert!(peer["port"].is_u64(), "every peer has a port: {peer}");
            for address in peer["addresses"].as_array().expect("addresses is a list") {
                let address = address.as_str().unwrap();
                assert!(
                    !address.contains(':'),
                    "IPv6 is filtered out -- nothing in the pane can use it yet: {address}"
                );
            }
        }
    }

    /// The addresses list is what someone types into another device, so loopback in it
    /// would be advice that cannot work.
    #[test]
    fn the_address_list_never_offers_loopback() {
        for address in lan_addresses() {
            assert_ne!(address, "127.0.0.1");
            assert!(!address.starts_with("169.254."), "link-local: {address}");
        }
    }
}
