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
    Ok(dir.join(sanitize(key)))
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
            #[cfg(unix)]
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&path)?;
                f.write_all(secret.as_bytes())?;
            }
            #[cfg(not(unix))]
            std::fs::write(&path, secret)?;
            Ok(())
        }
    }
}

pub fn load(key: &str) -> Result<Option<String>> {
    match entry(key) {
        Ok(e) => match e.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => load_fallback(key),
            Err(e) => Err(anyhow::Error::new(e).context("reading secret from keychain")),
        },
        Err(_) => load_fallback(key),
    }
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
