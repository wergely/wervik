//! Small shared helper for detecting external Linux gaming tools Wervik
//! integrates with (MangoHud, GameMode, ...) without implementing them
//! itself or assuming a specific distro/package manager.

use std::path::Path;

pub fn binary_installed(name: &str) -> bool {
    [format!("/usr/bin/{name}"), format!("/usr/local/bin/{name}")]
        .iter()
        .any(|p| Path::new(p).is_file())
        || std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
            .unwrap_or(false)
}

const SHARED_LIB_DIRS: &[&str] = &[
    "/usr/lib",
    "/usr/lib64",
    "/usr/local/lib",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
];

/// Whether a shared library Wervik doesn't manage itself (a system package,
/// not something a downloaded emulator bundles) is present — e.g. FBNeo's
/// official Linux release is dynamically linked against `libSDL2_image`,
/// which isn't guaranteed to already be on the system the way base SDL2
/// usually is. Checked by filename prefix, since the exact `.so.N` suffix
/// varies by distro/version.
pub fn shared_library_available(file_prefix: &str) -> bool {
    SHARED_LIB_DIRS.iter().any(|dir| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with(file_prefix))
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// `libc` is present on every real Linux system Wervik runs on —
    /// checked for real rather than mocked, same as `binary_installed`'s
    /// own real `$PATH` lookup.
    #[test]
    fn libc_is_found_as_a_real_installed_shared_library() {
        assert!(shared_library_available("libc.so"));
    }

    #[test]
    fn a_made_up_library_name_is_not_found() {
        assert!(!shared_library_available(
            "libtotally-not-a-real-wervik-test-lib.so"
        ));
    }
}
