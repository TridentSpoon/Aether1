// LAN discovery for other Aether1 instances, via mDNS/DNS-SD (RFC 6762/6763) -- the same
// mechanism printers, Chromecasts and AirPlay use, and the "presence detection" discussed
// for the home-lab/smart-house setup: a netbook offering access to its LLMs should be
// findable without anyone typing in its IP, but without turning the LAN into a chatty mess.
//
// Deliberately on-demand, not a background daemon: `announce()` only answers other
// machines' queries for as long as its returned guard is held (paired with `--serve`,
// which already keeps the process alive for that duration); `discover()` browses for a
// fixed window and then explicitly stops browsing and shuts its daemon down, so a LAN with
// nobody currently discovering stays silent. Actual routing (which peer is the strongest
// or least loaded right now) is a separate, later piece -- this only answers "who's out
// there and what did they publish about themselves," via free-form TXT properties.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

/// The DNS-SD service type every Aether1 instance announces itself under.
pub const SERVICE_TYPE: &str = "_aether1._tcp.local.";

/// One other Aether1 instance found on the LAN.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub instance_name: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
    /// Whatever `announce()` was given as `properties` on the other end -- e.g. a version
    /// string today; capability/load data once the routing piece exists.
    pub properties: HashMap<String, String>,
}

/// Keeps this instance announced on the LAN for as long as it's held. Dropping it
/// unregisters the service and shuts down the mDNS daemon thread -- nothing needs to be
/// called explicitly to stop announcing.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Best-effort: if the network is already gone there's nothing left to unregister,
        // and this isn't worth surfacing as an error during shutdown.
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Announces this instance as `_aether1._tcp.local.` on every local network interface.
/// `instance_name` should be unique on the LAN (e.g. the machine's hostname) -- mDNS handles
/// name collisions, but a stable name is what makes a peer recognizable across restarts.
pub fn announce(
    instance_name: &str,
    port: u16,
    properties: &[(&str, &str)],
) -> Result<Announcement, String> {
    let daemon =
        ServiceDaemon::new().map_err(|e| format!("could not start the mDNS daemon: {e}"))?;
    let host_name = format!("{instance_name}.local.");
    // Empty address + enable_addr_auto(): let the library discover this host's own LAN
    // addresses rather than guessing one ourselves (see the mdns-sd register example).
    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        instance_name,
        &host_name,
        "",
        port,
        properties,
    )
    .map_err(|e| format!("invalid mDNS service info: {e}"))?
    .enable_addr_auto();
    let fullname = service_info.get_fullname().to_string();
    daemon
        .register(service_info)
        .map_err(|e| format!("could not announce this instance on the LAN: {e}"))?;
    Ok(Announcement { daemon, fullname })
}

/// Browses for other Aether1 instances for `timeout`, then stops browsing and shuts the
/// daemon down before returning. This is the whole "on-demand, not a noisy background
/// listener" design point: nothing here keeps the network active once the call returns.
pub fn discover(timeout: Duration) -> Result<Vec<Peer>, String> {
    let daemon =
        ServiceDaemon::new().map_err(|e| format!("could not start the mDNS daemon: {e}"))?;
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| format!("could not browse the LAN: {e}"))?;

    let mut peers = Vec::new();
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                // A machine with more than one network interface (Wi-Fi + Ethernet, a
                // container bridge, ...) resolves the same instance once per interface --
                // observed directly while testing this against a real announce(). Merge
                // those into one entry instead of listing the same peer several times.
                let instance_name = strip_service_suffix(&info.fullname);
                let addresses: Vec<IpAddr> =
                    info.addresses.iter().map(|ip| ip.to_ip_addr()).collect();
                let properties = info
                    .txt_properties
                    .iter()
                    .map(|p| (p.key().to_string(), p.val_str().to_string()))
                    .collect();
                match peers
                    .iter_mut()
                    .find(|p: &&mut Peer| p.instance_name == instance_name && p.port == info.port)
                {
                    Some(existing) => {
                        for addr in addresses {
                            if !existing.addresses.contains(&addr) {
                                existing.addresses.push(addr);
                            }
                        }
                        existing.properties = properties;
                    }
                    None => peers.push(Peer {
                        instance_name,
                        host: info.host.clone(),
                        port: info.port,
                        addresses,
                        properties,
                    }),
                }
            }
            // Any other event (ServiceFound, ServiceRemoved, ...) needs no action here --
            // ServiceResolved is what carries the address/port/TXT data this returns.
            Ok(_) => {}
            // Timed out waiting, or the daemon's channel closed -- either way, stop.
            Err(_) => break,
        }
    }

    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();
    Ok(peers)
}

/// `"my-machine._aether1._tcp.local."` -> `"my-machine"` -- the instance name is the only
/// part of the fullname that identifies *which* Aether1 this is, the rest is just the
/// service type every instance shares.
fn strip_service_suffix(fullname: &str) -> String {
    fullname
        .strip_suffix(&format!(".{SERVICE_TYPE}"))
        .unwrap_or(fullname)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_shared_service_suffix_from_a_fullname() {
        assert_eq!(
            strip_service_suffix("kitchen-netbook._aether1._tcp.local."),
            "kitchen-netbook"
        );
    }

    #[test]
    fn leaves_an_unrecognized_fullname_alone() {
        assert_eq!(strip_service_suffix("something-else."), "something-else.");
    }

    // Real announce()/discover() round-trips are covered by manually running the CLI's
    // `announce` and `discover` subcommands against each other, not as an automated test
    // here -- CI runners aren't guaranteed the same multicast-capable network setup as a
    // real machine, so a flaky network environment shouldn't fail the build.
}
