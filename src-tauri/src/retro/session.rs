//! An EmulatorJS play session: which ROM (and optional BIOS) a running
//! local server instance is allowed to serve. The session id, which game
//! it belongs to, and when it started are tracked by the caller
//! (`commands::AppState::retro_sessions`) rather than duplicated here —
//! this struct only carries what `retro::server` actually needs to serve
//! the session's content.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct EmulatorSession {
    pub rom_path: PathBuf,
    pub core: String,
    pub bios_path: Option<PathBuf>,
}

/// A random 128-bit id, hex-encoded — long and unguessable enough that
/// knowing one session's URL doesn't help you find another's, since the
/// server also refuses any request whose id doesn't match a live session.
pub fn generate_session_id() -> String {
    let high: u64 = rand::random();
    let low: u64 = rand::random();
    format!("{high:016x}{low:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_ids() {
        let a = generate_session_id();
        let b = generate_session_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
