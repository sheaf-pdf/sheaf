//! Where sheaf keeps its own files.
//!
//! - **Config:** what the user chooses and may edit by hand
//!   (`settings.toml`, `keymap.json`, `marks.json`, `patterns.json`,
//!   `toolbar.json`, plugins). `%APPDATA%` on Windows, which roams with the
//!   user's profile; `~/.config/sheaf` on Linux; `~/Library/Application
//!   Support/sheaf` on macOS.
//! - **State:** what sheaf remembers for itself (`state.json`, recent
//!   files). Local to the machine and safe to delete:
//!   `%LOCALAPPDATA%\sheaf\state`, `~/.local/state/sheaf`, and
//!   `~/Library/Application Support/sheaf/state`. Its `recovery/` is the
//!   exception: it keeps the crash-recovery sidecars of documents no file
//!   holds yet ([`recovery_dir`]), whose unsaved pages go with them (a
//!   file's sidecar is beside the file).
//!
//! `SHEAF_CONFIG_DIR` moves both, state to `<config>/state`, so tests and
//! scripts stay clear of the user's own files.

use std::path::PathBuf;

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The user's home folder.
pub fn home() -> Option<PathBuf> {
    var("HOME").or_else(|| var("USERPROFILE"))
}

/// The config folder.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = var("SHEAF_CONFIG_DIR") {
        return dir;
    }
    let base = if cfg!(target_os = "windows") {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("sheaf")
}

/// The state folder.
pub fn state_dir() -> PathBuf {
    if let Some(dir) = var("SHEAF_CONFIG_DIR") {
        return dir.join("state");
    }
    let dir = if cfg!(target_os = "windows") {
        var("LOCALAPPDATA").map(|d| d.join("sheaf").join("state"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/sheaf/state"))
    } else {
        var("XDG_STATE_HOME")
            .map(|d| d.join("sheaf"))
            .or_else(|| home().map(|h| h.join(".local/state/sheaf")))
    };
    dir.unwrap_or_else(|| config_dir().join("state"))
}

/// The state folder's `recovery/`: the crash-recovery sidecars of
/// untitled documents (M5 proposal 2), which have no file to sit beside.
pub fn recovery_dir() -> PathBuf {
    state_dir().join("recovery")
}
