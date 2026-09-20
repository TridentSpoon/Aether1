// TLS for `--lan`, with a certificate this machine signs for itself.
//
// Until this existed, the token `serve_auth` goes to such lengths to protect travelled in a
// plain `Authorization` header: anyone sharing the network could read it off the wire once
// and reuse it indefinitely, which made the care taken over the phrase largely decorative.
//
// **The certificate is self-signed by necessity, not as a shortcut.** No public authority
// will issue for a `.local` name or a private address, so the alternative is not "a real
// certificate" but "own a domain and point it at your living room". What replaces the
// authority is the operator: the certificate's SHA-256 fingerprint is printed when the
// server starts, and comparing that short string against what the other end shows is the
// check a browser's padlock would otherwise be standing in for. A browser will still warn on
// first connection, and that warning is the moment the fingerprint is for.
//
// Loopback keeps plain HTTP deliberately. Nothing can reach 127.0.0.1 from off the machine,
// so there is no wire to listen to, and turning the default mode into one that greets every
// operator with a certificate warning would be a cost with nothing bought.

use std::path::{Path, PathBuf};

use base64::Engine;
use sha2::{Digest, Sha256};

/// Where the pair lives, beside `serve_token.hash` for the same reason: these are this
/// installation's identity on the network, not something to regenerate per run. A stable
/// certificate is what lets a fingerprint someone wrote down still mean something tomorrow.
fn cert_path() -> PathBuf {
    crate::project_root().join("backend").join("serve_cert.pem")
}

fn key_path() -> PathBuf {
    crate::project_root().join("backend").join("serve_key.pem")
}

/// A certificate's fingerprint, rendered for a person to read out loud.
///
/// The whole SHA-256 rather than a prefix of it: a truncated fingerprint is a weaker check,
/// and the only thing truncation buys is a shorter line. Grouping is what makes it
/// comparable by eye, the way a card number is easier to check in fours than as sixteen
/// digits.
pub fn fingerprint(certificate_der: &[u8]) -> String {
    let digest = Sha256::digest(certificate_der);
    let hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    hex.as_bytes()
        .chunks(8)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pulls the first certificate out of a PEM file as DER.
///
/// Hand-rolled rather than pulled in as another dependency: the format is a base64 body
/// between two marker lines, `base64` is already a dependency, and this reads one file this
/// program wrote itself.
fn first_certificate_der(pem: &str) -> Result<Vec<u8>, String> {
    let body: String = pem
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END CERTIFICATE-----"))
        .collect();
    if body.is_empty() {
        return Err("no certificate found in the PEM file".to_string());
    }
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .map_err(|e| format!("the certificate is not valid base64: {e}"))
}

/// The names the certificate claims. Verification here is by fingerprint rather than by
/// name, so this list is about a browser having one less thing to complain about, not about
/// what the certificate is trusted for.
fn subject_names() -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Some(host) = sysinfo::System::host_name() {
        // The mDNS name too, since `discovery` announces this machine under it and that is
        // the address someone on the network is most likely to type.
        names.push(format!("{host}.local"));
        names.push(host);
    }
    names
}

fn generate_at(cert_file: &Path, key_file: &Path) -> Result<(String, String), String> {
    let generated = rcgen::generate_simple_self_signed(subject_names())
        .map_err(|e| format!("could not generate a certificate: {e}"))?;
    let cert_pem = generated.cert.pem();
    let key_pem = generated.signing_key.serialize_pem();

    if let Some(parent) = cert_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(cert_file, &cert_pem)
        .map_err(|e| format!("could not save {}: {e}", cert_file.display()))?;
    std::fs::write(key_file, &key_pem)
        .map_err(|e| format!("could not save {}: {e}", key_file.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort, as with the pairing token: a filesystem that refuses permission
        // changes still gets a working key, just not one this can lock down further.
        let _ = std::fs::set_permissions(key_file, std::fs::Permissions::from_mode(0o600));
    }
    Ok((cert_pem, key_pem))
}

/// Whether the certificate was already on disk or had to be made, so the caller can say
/// "write this down" the once rather than every run.
pub enum Origin {
    Existing,
    New,
}

pub struct Tls {
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
    pub origin: Origin,
}

fn load_or_create_at(cert_file: &Path, key_file: &Path) -> Result<Tls, String> {
    let existing = std::fs::read_to_string(cert_file).ok().zip(
        std::fs::read_to_string(key_file)
            .ok()
            .filter(|key| !key.trim().is_empty()),
    );
    let (cert_pem, key_pem, origin) = match existing {
        Some((cert, key)) if !cert.trim().is_empty() => (cert, key, Origin::Existing),
        // A half-written pair -- a certificate with no key, or either one empty -- is
        // replaced rather than repaired. There is nothing in it worth keeping, and failing
        // here would leave --lan unable to start with no way to fix it but deleting files.
        _ => {
            let (cert, key) = generate_at(cert_file, key_file)?;
            (cert, key, Origin::New)
        }
    };

    let fingerprint = fingerprint(&first_certificate_der(&cert_pem)?);
    Ok(Tls {
        cert_pem,
        key_pem,
        fingerprint,
        origin,
    })
}

/// Loads this installation's certificate, making one on first use.
pub fn load_or_create() -> Result<Tls, String> {
    load_or_create_at(&cert_path(), &key_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("aether1_serve_tls_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn paths(name: &str) -> (PathBuf, PathBuf) {
        let dir = temp_dir(name);
        (dir.join("cert.pem"), dir.join("key.pem"))
    }

    #[test]
    fn a_fresh_install_generates_a_certificate_and_keeps_it() {
        let (cert_file, key_file) = paths("fresh");

        let first = load_or_create_at(&cert_file, &key_file).expect("should generate");
        assert!(matches!(first.origin, Origin::New));
        assert!(first.cert_pem.contains("BEGIN CERTIFICATE"));

        let second = load_or_create_at(&cert_file, &key_file).expect("should reload");
        assert!(matches!(second.origin, Origin::Existing));
        // The fingerprint is what an operator writes down, so a restart must not change it.
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn a_half_written_pair_is_replaced_rather_than_failing() {
        let (cert_file, key_file) = paths("half-written");
        std::fs::write(&cert_file, "-----BEGIN CERTIFICATE-----\n").unwrap();

        let tls = load_or_create_at(&cert_file, &key_file)
            .expect("a certificate with no key should be regenerated, not fatal");
        assert!(matches!(tls.origin, Origin::New));
        assert!(!tls.fingerprint.is_empty());
    }

    #[test]
    fn the_fingerprint_is_the_whole_digest_grouped_for_reading() {
        let printed = fingerprint(b"anything at all");
        let digits: String = printed.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(
            digits.len(),
            64,
            "a truncated fingerprint is a weaker check"
        );
        assert!(digits.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(printed.split(' ').count(), 8);
    }

    #[test]
    fn two_machines_do_not_share_a_fingerprint() {
        let (cert_a, key_a) = paths("distinct-a");
        let (cert_b, key_b) = paths("distinct-b");

        let a = load_or_create_at(&cert_a, &key_a).unwrap();
        let b = load_or_create_at(&cert_b, &key_b).unwrap();
        assert_ne!(
            a.fingerprint, b.fingerprint,
            "the fingerprint has to identify this machine, not this program"
        );
    }

    #[test]
    fn a_pem_with_no_certificate_in_it_is_reported() {
        assert!(first_certificate_der("not a certificate").is_err());
    }
}
