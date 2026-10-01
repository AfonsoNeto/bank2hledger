//! Secret storage: OS keychain first, 0600 file fallback.
//!
//! Keys are namespaced like `monzo:monzo-personal:refresh_token`. Secrets are
//! never logged; error messages from this module never embed secret material.

use anyhow::{Context, Result};

const SERVICE: &str = "bank2hledger";

fn fallback_path(key: &str) -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .context("cannot determine config directory for secret fallback")?
        .join("bank2hledger")
        .join("secrets");
    std::fs::create_dir_all(&dir)?;
    tighten_dir_permissions(&dir)?;
    Ok(dir.join(sanitize(key)))
}

/// The fallback directory holds secrets: it must not be group/world readable
/// regardless of umask or pre-existing loose permissions.
#[cfg(unix)]
fn tighten_dir_permissions(dir: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(dir)?.permissions();
    if perms.mode() & 0o777 != 0o700 {
        perms.set_mode(0o700);
        std::fs::set_permissions(dir, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_dir_permissions(_dir: &std::path::Path) -> Result<()> {
    Ok(())
}

fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn entry(key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, key).context("cannot access OS keychain")
}

pub fn store(key: &str, secret: &str) -> Result<()> {
    match entry(key).and_then(|e| e.set_password(secret).map_err(anyhow::Error::from)) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!(
                "warning: OS keychain unavailable ({}), storing secret in a 0600 file instead",
                redact(&e.to_string())
            );
            let path = fallback_path(key)?;
            crate::fs_guard::write_refusing_symlinks(&path, secret.as_bytes())?;
            enforce_secret_file_permissions(&path)?;
            Ok(())
        }
    }
}

/// 0600 must hold on every write, not only at creation — a file that
/// pre-existed with looser permissions (e.g. from a umask change) would
/// otherwise keep them.
#[cfg(unix)]
fn enforce_secret_file_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    if perms.mode() & 0o777 != 0o600 {
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn enforce_secret_file_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

pub fn load(key: &str) -> Result<Option<String>> {
    match entry(key) {
        Ok(e) => match e.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => load_fallback(key),
            // The keychain service itself is broken/unavailable (e.g. no
            // Secret Service daemon on a headless Linux box). `store` falls
            // back to the file in that situation, so `load` must too —
            // otherwise secrets written to the fallback could never be read
            // back. Mirrors store()'s fallback-with-warning behavior.
            Err(e) => {
                eprintln!(
                    "warning: OS keychain unavailable ({}), reading secret from the 0600 fallback file",
                    redact(&e.to_string())
                );
                load_fallback(key)
            }
        },
        Err(_) => load_fallback(key),
    }
}

#[cfg(unix)]
#[cfg(test)]
pub(crate) fn fallback_store_for_test(key: &str, secret: &str) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = fallback_path(key)?;
    std::fs::write(&path, secret)?;
    // Simulate a file created under a loose umask before bank2hledger wrote it.
    let mut loose = std::fs::metadata(&path)?.permissions();
    loose.set_mode(0o644);
    std::fs::set_permissions(&path, loose)?;
    enforce_secret_file_permissions(&path)?;
    Ok(path)
}

fn load_fallback(key: &str) -> Result<Option<String>> {
    let path = fallback_path(key)?;
    if path.exists() {
        Ok(Some(std::fs::read_to_string(path)?.trim().to_string()))
    } else {
        Ok(None)
    }
}

pub fn delete(key: &str) -> Result<()> {
    if let Ok(e) = entry(key) {
        let _ = e.delete_credential();
    }
    let path = fallback_path(key)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn redact(msg: &str) -> String {
    // Defensive: keychain errors occasionally echo credential material.
    let mut out = String::with_capacity(msg.len());
    for word in msg.split_whitespace() {
        if word.len() > 24
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            out.push_str("<redacted>");
        } else {
            out.push_str(word);
        }
        out.push(' ');
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Works against the real keychain when available; otherwise exercises
    /// the 0600-file fallback. Either path must round-trip.
    #[test]
    fn store_load_delete_round_trip() {
        let key = "test:round-trip:secret";
        store(key, "s3cret-value").unwrap();
        assert_eq!(load(key).unwrap().as_deref(), Some("s3cret-value"));
        delete(key).unwrap();
        assert_eq!(load(key).unwrap(), None);
    }

    #[test]
    fn overwriting_a_secret_replaces_it() {
        let key = "test:overwrite:secret";
        store(key, "first").unwrap();
        store(key, "second").unwrap();
        assert_eq!(load(key).unwrap().as_deref(), Some("second"));
        delete(key).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn loose_preexisting_fallback_file_is_tightened_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let key = "test:fallback:perms";
        // store() prefers the keychain when available, so exercise the
        // fallback write path directly.
        let path = fallback_store_for_test(key, "s3cret").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "secret file must be 0600 after write");
        delete(key).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fallback_secrets_dir_is_0700() {
        use std::os::unix::fs::PermissionsExt;
        let key = "test:dir:perms";
        let path = fallback_path(key).unwrap();
        let dir = path.parent().unwrap();
        let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "secrets directory must be 0700");
    }

    #[test]
    fn fallback_files_never_embed_raw_key_characters() {
        // The sanitized file name must be filesystem-safe for keys with ':'.
        assert_eq!(
            sanitize("monzo:acct-1:refresh_token"),
            "monzo_acct-1_refresh_token"
        );
    }

    #[test]
    fn redact_masks_long_token_like_words() {
        let msg = "error with token ABCDEFGHIJKLMNOPQRSTUVWXYZ012345 inside";
        let out = redact(msg);
        assert!(!out.contains("ABCDEFGHIJKLMNOP"), "{out}");
        assert!(out.contains("<redacted>"), "{out}");
        assert!(out.contains("error"), "{out}");
    }
}
