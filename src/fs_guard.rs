//! File-write guard for paths derived from untrusted or shared locations
//! (`in/`, `staging/`, `rules/`).
//!
//! `std::fs::write` follows symlinks: a symlink planted at the destination
//! (a synced/cloud folder, a shared mount, another local process) redirects
//! the write outside the intended tree. These helpers refuse to follow one.
//!
//! Note this is check-then-use, not atomic O_NOFOLLOW; it closes the
//! realistic "planted symlink" case, not active local attackers racing the
//! write. Everything here is plain std — no unsafe.

use std::path::Path;

use anyhow::{bail, Context, Result};

pub fn write_refusing_symlinks(path: &Path, contents: &[u8]) -> Result<()> {
    if let Ok(md) = std::fs::symlink_metadata(path) {
        if md.file_type().is_symlink() {
            bail!(
                "refusing to write through symlink at {} — remove it if this location is trusted",
                path.display()
            );
        }
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn refuses_to_write_through_a_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"innocent").unwrap();
        let target = dir.path().join("drop.csv");
        std::os::unix::fs::symlink(&victim, &target).unwrap();

        let err = write_refusing_symlinks(&target, b"evil").unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        // The victim file is untouched.
        assert_eq!(std::fs::read(&victim).unwrap(), b"innocent");
    }

    #[test]
    fn ordinary_files_are_created_and_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        write_refusing_symlinks(&path, b"a").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"a");
        write_refusing_symlinks(&path, b"b").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"b");
    }

    #[cfg(unix)]
    #[test]
    fn writing_a_symlinked_directory_component_is_not_blocked() {
        // The guard targets the final component only; a real directory
        // reached via symlinked parent dirs is the user's own config choice.
        let dir = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir_in(&dir).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        assert!(write_refusing_symlinks(&link.join("f.txt"), b"x").is_ok());
    }
}
