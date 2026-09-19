// Author: Jeff
// Date: 2026-09-19
// Description: Where the ticker socket lives, and taking it over safely at start
// Notes: $XDG_RUNTIME_DIR is per-user and 0700; the socket itself is made 0600 too.
//        At start: nothing there → bind; a socket someone answers on → another daemon is
//        running, refuse; a socket nobody answers → left over from a crash, remove it;
//        anything that is not a socket → never delete it, refuse

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const SOCKET_NAME: &str = "mg-feedr.sock";
const OWNER_ONLY: u32 = 0o600;

// $XDG_RUNTIME_DIR/mg-feedr.sock — no runtime folder means no safe place for it
pub fn default_path() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .context("XDG_RUNTIME_DIR is not set; mg-feedr needs the per-user runtime folder")?;
    Ok(PathBuf::from(dir).join(SOCKET_NAME))
}

// Bind the socket, clearing only a stale one, and make it owner-only
pub fn prepare(path: &Path) -> Result<UnixListener> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_socket() => {
            bail!(
                "{} exists and is not a socket; leaving it alone",
                path.display()
            )
        }
        Ok(_) => {
            if UnixStream::connect(path).is_ok() {
                bail!("another mg-feedr is already running on {}", path.display())
            }
            fs::remove_file(path).with_context(|| format!("removing stale {}", path.display()))?;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("checking {}", path.display())),
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_path_binds_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let _listener = prepare(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, OWNER_ONLY);
    }

    #[test]
    fn a_live_socket_is_refused_a_stale_one_replaced_and_a_file_never_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let live = prepare(&path).unwrap();
        assert!(prepare(&path).is_err(), "someone is listening");
        drop(live);
        assert!(prepare(&path).is_ok(), "nobody answers: stale, replaced");

        let file = dir.path().join("not-a-socket");
        fs::write(&file, "keep me").unwrap();
        assert!(prepare(&file).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "keep me");
    }
}
