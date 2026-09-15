//! Composes a per-game wrapper script from whichever Linux gaming tools are
//! enabled for that game (MangoHud, GameMode, custom environment
//! variables), and Steam Launch Options detection for it.
//!
//! Wervik never edits Steam's `localconfig.vdf` itself — Steam is known to
//! overwrite externally-made changes to it while running (GOverlay avoids
//! this the same way, generating a command for the user to paste rather
//! than writing it for them). Instead, this module generates one small
//! per-game wrapper script and Wervik shows the user the exact Launch
//! Options string to paste into Steam's own Properties dialog once. From
//! then on, the wrapper checks marker files on every launch and only
//! activates what's enabled — so toggling anything afterwards is just
//! rewriting those local files, entirely outside of Steam.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::mangohud;
use crate::sysutil;

fn app_dir(dir: &Path, key: &str) -> PathBuf {
    dir.join(key)
}

pub fn wrapper_path(dir: &Path, key: &str) -> PathBuf {
    app_dir(dir, key).join("run")
}

fn gamemode_marker_path(dir: &Path, key: &str) -> PathBuf {
    app_dir(dir, key).join("gamemode-enabled")
}

pub fn is_gamemode_enabled(dir: &Path, key: &str) -> bool {
    gamemode_marker_path(dir, key).is_file()
}

pub fn set_gamemode_enabled(dir: &Path, key: &str, enabled: bool) -> Result<(), String> {
    let marker = gamemode_marker_path(dir, key);
    if enabled {
        std::fs::create_dir_all(app_dir(dir, key)).map_err(|e| e.to_string())?;
        std::fs::write(&marker, b"").map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&marker) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn is_gamemode_installed() -> bool {
    sysutil::binary_installed("gamemoderun")
}

pub fn is_gamescope_installed() -> bool {
    sysutil::binary_installed("gamescope")
}

fn gamescope_marker_path(dir: &Path, key: &str) -> PathBuf {
    app_dir(dir, key).join("gamescope-enabled")
}

pub fn is_gamescope_enabled(dir: &Path, key: &str) -> bool {
    gamescope_marker_path(dir, key).is_file()
}

pub fn set_gamescope_enabled(dir: &Path, key: &str, enabled: bool) -> Result<(), String> {
    let marker = gamescope_marker_path(dir, key);
    if enabled {
        std::fs::create_dir_all(app_dir(dir, key)).map_err(|e| e.to_string())?;
        std::fs::write(&marker, b"").map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&marker) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Gamescope has no config file of its own — every setting is a command-line
/// flag passed at launch, so unlike MangoHud, Wervik's own JSON here *is*
/// the full config; there's no external format to match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamescopeConfig {
    /// 0 means "not set" — gamescope runs at its default/native resolution.
    pub internal_width: u32,
    pub internal_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    /// "none" | "fsr" | "nis"
    pub upscaler: String,
}

impl Default for GamescopeConfig {
    fn default() -> Self {
        Self {
            internal_width: 0,
            internal_height: 0,
            output_width: 0,
            output_height: 0,
            upscaler: "none".into(),
        }
    }
}

fn gamescope_config_path(dir: &Path, key: &str) -> PathBuf {
    app_dir(dir, key).join("gamescope.json")
}

pub fn read_gamescope_config(dir: &Path, key: &str) -> GamescopeConfig {
    std::fs::read_to_string(gamescope_config_path(dir, key))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_gamescope_config(
    dir: &Path,
    key: &str,
    config: &GamescopeConfig,
) -> Result<(), String> {
    std::fs::create_dir_all(app_dir(dir, key)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(gamescope_config_path(dir, key), json).map_err(|e| e.to_string())
}

/// Builds gamescope's CLI flags for `config`. Every value here is either a
/// plain integer or one of a fixed, Wervik-controlled set of strings, so
/// none of it needs shell-escaping like free-form env var values do.
/// `pub(crate)` so `retro::native` can reuse it when composing a native
/// emulator's launch command directly, without going through a wrapper
/// script the way Steam games do.
pub(crate) fn gamescope_args(config: &GamescopeConfig) -> Vec<String> {
    let mut args = Vec::new();
    if config.internal_width > 0 && config.internal_height > 0 {
        args.push("-w".to_string());
        args.push(config.internal_width.to_string());
        args.push("-h".to_string());
        args.push(config.internal_height.to_string());
    }
    if config.output_width > 0 && config.output_height > 0 {
        args.push("-W".to_string());
        args.push(config.output_width.to_string());
        args.push("-H".to_string());
        args.push(config.output_height.to_string());
    }
    if config.upscaler == "fsr" || config.upscaler == "nis" {
        args.push("-F".to_string());
        args.push(config.upscaler.clone());
        // Without an explicit scaler mode, "auto" may not actually invoke
        // the chosen filter when internal/output sizes share an aspect
        // ratio — "fit" reliably applies it.
        args.push("-S".to_string());
        args.push("fit".to_string());
    }
    args
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

fn env_vars_path(dir: &Path, key: &str) -> PathBuf {
    app_dir(dir, key).join("env-vars.json")
}

pub fn read_env_vars(dir: &Path, key: &str) -> Vec<EnvVar> {
    std::fs::read_to_string(env_vars_path(dir, key))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn write_env_vars(dir: &Path, key: &str, vars: &[EnvVar]) -> Result<(), String> {
    for var in vars {
        if !is_valid_env_key(&var.key) {
            return Err(format!(
                "\"{}\" isn't a valid environment variable name",
                var.key
            ));
        }
    }
    std::fs::create_dir_all(app_dir(dir, key)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(vars).map_err(|e| e.to_string())?;
    std::fs::write(env_vars_path(dir, key), json).map_err(|e| e.to_string())
}

/// Regenerates the wrapper script for `app_id` from whatever state
/// (MangoHud marker/config, GameMode marker, Gamescope marker/config, env
/// vars) is currently on disk. Idempotent — safe to call after any single
/// setting changes.
///
/// Composition order (outermost to innermost) is gamescope, then gamemode,
/// then mangohud, then the game itself — gamescope has to own the display
/// session before anything else runs inside it, and gamemode conventionally
/// wraps the whole game+overlay invocation. We deliberately keep wrapping
/// with plain `mangohud` here rather than switching to gamescope's own
/// `--mangoapp` flag when gamescope is active: plain `mangohud` is verified
/// to work the same way regardless of gamescope, while `--mangoapp` is a
/// distinct code path we haven't been able to verify behaves identically
/// (e.g. for `no_display`-driven FPS-limit-only mode).
pub fn regenerate_wrapper(dir: &Path, key: &str) -> Result<(), String> {
    std::fs::create_dir_all(app_dir(dir, key)).map_err(|e| e.to_string())?;

    let wrapper = wrapper_path(dir, key);
    let mangohud_marker = mangohud::enabled_marker_path(dir, key);
    let mangohud_conf = mangohud::config_path(dir, key);
    let gamemode_marker = gamemode_marker_path(dir, key);
    let gamescope_marker = gamescope_marker_path(dir, key);
    let gamescope_flags = gamescope_args(&read_gamescope_config(dir, key));
    let env_vars = read_env_vars(dir, key);

    let mut script = String::new();
    script.push_str("#!/bin/sh\n");
    script.push_str(
        "# Generated by Wervik. Safe to leave in place: this only activates\n\
         # tools enabled from within Wervik; otherwise the game runs exactly\n\
         # as Steam would have, unmodified.\n",
    );
    for var in &env_vars {
        script.push_str(&format!(
            "export {}='{}'\n",
            var.key,
            escape_single_quoted(&var.value)
        ));
    }
    script.push_str(&format!(
        "if [ -f \"{marker}\" ]; then\n\
         \texport MANGOHUD=1\n\
         \texport MANGOHUD_CONFIGFILE=\"{conf}\"\n\
         \tset -- mangohud \"$@\"\n\
         fi\n",
        marker = mangohud_marker.display(),
        conf = mangohud_conf.display(),
    ));
    script.push_str(&format!(
        "if [ -f \"{marker}\" ]; then\n\tset -- gamemoderun \"$@\"\nfi\n",
        marker = gamemode_marker.display(),
    ));
    if gamescope_flags.is_empty() {
        script.push_str(&format!(
            "if [ -f \"{marker}\" ]; then\n\tset -- gamescope -- \"$@\"\nfi\n",
            marker = gamescope_marker.display(),
        ));
    } else {
        script.push_str(&format!(
            "if [ -f \"{marker}\" ]; then\n\tset -- gamescope {flags} -- \"$@\"\nfi\n",
            marker = gamescope_marker.display(),
            flags = gamescope_flags.join(" "),
        ));
    }
    script.push_str("exec \"$@\"\n");

    std::fs::write(&wrapper, script).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&wrapper)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn escape_single_quoted(value: &str) -> String {
    value.replace('\'', "'\\''")
}

/// Whether this game's Launch Options already point at our wrapper — i.e.
/// whether the one-time setup (and its Steam restart) has already happened.
pub fn is_wrapper_installed(current_launch_options: Option<&str>, wrapper_path: &Path) -> bool {
    wrapper_path.is_file()
        && current_launch_options
            .map(|opts| opts.contains(&wrapper_path.to_string_lossy().to_string()))
            .unwrap_or(false)
}

/// Builds the Launch Options string that wraps the game with our script,
/// preserving any custom launch options the user already had.
pub fn build_wrapper_launch_options(original: &str, wrapper_path: &Path) -> String {
    let prefix = format!("\"{}\"", wrapper_path.to_string_lossy());
    let trimmed = original.trim();
    if trimmed.is_empty() {
        format!("{prefix} %command%")
    } else if trimmed.contains("%command%") {
        format!("{prefix} {trimmed}")
    } else {
        format!("{prefix} {trimmed} %command%")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn wrapper_launch_options_wraps_around_existing_command() {
        let wrapper = Path::new("/home/x/.config/wervik/mangohud/123/run");
        assert_eq!(
            build_wrapper_launch_options("", wrapper),
            "\"/home/x/.config/wervik/mangohud/123/run\" %command%"
        );
        assert_eq!(
            build_wrapper_launch_options("SOME_VAR=1 %command%", wrapper),
            "\"/home/x/.config/wervik/mangohud/123/run\" SOME_VAR=1 %command%"
        );
        assert_eq!(
            build_wrapper_launch_options("SOME_VAR=1", wrapper),
            "\"/home/x/.config/wervik/mangohud/123/run\" SOME_VAR=1 %command%"
        );
    }

    #[test]
    fn rejects_invalid_env_var_names() {
        let dir = std::env::temp_dir().join(format!("wervik-envvar-test-{}", std::process::id()));
        let vars = vec![EnvVar {
            key: "not valid!".into(),
            value: "1".into(),
        }];
        assert!(write_env_vars(&dir, "1", &vars).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn escapes_single_quotes_in_env_values() {
        assert_eq!(escape_single_quoted("it's"), "it'\\''s");
    }

    #[test]
    fn regenerated_wrapper_composes_env_vars_gamemode_and_mangohud_in_order() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("wervik-wrapper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        const APP_ID: &str = "42";

        write_env_vars(
            &dir,
            APP_ID,
            &[EnvVar {
                key: "DXVK_HUD".into(),
                value: "fps".into(),
            }],
        )
        .unwrap();
        set_gamemode_enabled(&dir, APP_ID, true).unwrap();
        mangohud::set_enabled(&dir, APP_ID, true).unwrap();
        set_gamescope_enabled(&dir, APP_ID, true).unwrap();
        write_gamescope_config(
            &dir,
            APP_ID,
            &GamescopeConfig {
                internal_width: 1280,
                internal_height: 800,
                output_width: 1920,
                output_height: 1200,
                upscaler: "fsr".into(),
            },
        )
        .unwrap();

        regenerate_wrapper(&dir, APP_ID).unwrap();

        let script = std::fs::read_to_string(wrapper_path(&dir, APP_ID)).unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("export DXVK_HUD='fps'\n"));
        assert!(script.contains("set -- mangohud \"$@\""));
        assert!(script.contains("set -- gamemoderun \"$@\""));
        assert!(script
            .contains("set -- gamescope -w 1280 -h 800 -W 1920 -H 1200 -F fsr -S fit -- \"$@\""));
        // Outermost to innermost must be gamescope, then gamemode, then mangohud.
        let mangohud_pos = script.find("set -- mangohud").unwrap();
        let gamemode_pos = script.find("set -- gamemoderun").unwrap();
        let gamescope_pos = script.find("set -- gamescope").unwrap();
        assert!(
            mangohud_pos < gamemode_pos,
            "mangohud must be composed before gamemode"
        );
        assert!(
            gamemode_pos < gamescope_pos,
            "gamemode must be composed before gamescope"
        );
        assert!(script.trim_end().ends_with("exec \"$@\""));

        let perms = std::fs::metadata(wrapper_path(&dir, APP_ID))
            .unwrap()
            .permissions();
        assert_eq!(perms.mode() & 0o111, 0o111, "wrapper must be executable");

        // Disabling everything should still regenerate a valid passthrough.
        set_gamemode_enabled(&dir, APP_ID, false).unwrap();
        mangohud::set_enabled(&dir, APP_ID, false).unwrap();
        set_gamescope_enabled(&dir, APP_ID, false).unwrap();
        write_env_vars(&dir, APP_ID, &[]).unwrap();
        regenerate_wrapper(&dir, APP_ID).unwrap();
        let script = std::fs::read_to_string(wrapper_path(&dir, APP_ID)).unwrap();
        assert!(!script.contains("export DXVK_HUD"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gamescope_args_only_include_fields_that_are_set() {
        assert_eq!(
            gamescope_args(&GamescopeConfig::default()),
            Vec::<String>::new()
        );

        let internal_only = GamescopeConfig {
            internal_width: 1280,
            internal_height: 800,
            ..GamescopeConfig::default()
        };
        assert_eq!(
            gamescope_args(&internal_only),
            vec!["-w", "1280", "-h", "800"]
        );

        let upscaler_only = GamescopeConfig {
            upscaler: "nis".into(),
            ..GamescopeConfig::default()
        };
        assert_eq!(
            gamescope_args(&upscaler_only),
            vec!["-F", "nis", "-S", "fit"]
        );

        // "none" must never emit -F/-S.
        let none_upscaler = GamescopeConfig {
            upscaler: "none".into(),
            ..GamescopeConfig::default()
        };
        assert!(gamescope_args(&none_upscaler).is_empty());
    }

    #[test]
    fn regenerated_wrapper_handles_gamescope_enabled_with_no_flags_set() {
        let dir =
            std::env::temp_dir().join(format!("wervik-gamescope-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        const APP_ID: &str = "43";

        set_gamescope_enabled(&dir, APP_ID, true).unwrap();
        regenerate_wrapper(&dir, APP_ID).unwrap();

        let script = std::fs::read_to_string(wrapper_path(&dir, APP_ID)).unwrap();
        assert!(script.contains("set -- gamescope -- \"$@\""));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, unix))]
mod shell_syntax_check {
    use super::*;

    #[test]
    fn generated_wrapper_is_valid_posix_shell() {
        let dir = std::env::temp_dir().join(format!("wervik-shellcheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        const APP_ID: &str = "7";

        write_env_vars(
            &dir,
            APP_ID,
            &[EnvVar {
                key: "FOO".into(),
                value: "it's a test".into(),
            }],
        )
        .unwrap();
        set_gamemode_enabled(&dir, APP_ID, true).unwrap();
        mangohud::set_enabled(&dir, APP_ID, true).unwrap();
        set_gamescope_enabled(&dir, APP_ID, true).unwrap();
        write_gamescope_config(
            &dir,
            APP_ID,
            &GamescopeConfig {
                internal_width: 1280,
                internal_height: 800,
                output_width: 1920,
                output_height: 1200,
                upscaler: "fsr".into(),
            },
        )
        .unwrap();
        regenerate_wrapper(&dir, APP_ID).unwrap();

        let output = std::process::Command::new("sh")
            .arg("-n")
            .arg(wrapper_path(&dir, APP_ID))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "sh -n failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, unix))]
mod runtime_passthrough_check {
    use super::*;

    #[test]
    fn wrapper_runs_the_real_command_and_applies_env_vars_when_nothing_enabled() {
        let dir = std::env::temp_dir().join(format!("wervik-passthrough-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        const APP_ID: &str = "9";

        write_env_vars(
            &dir,
            APP_ID,
            &[EnvVar {
                key: "WERVIK_TEST_VAR".into(),
                value: "hello world".into(),
            }],
        )
        .unwrap();
        regenerate_wrapper(&dir, APP_ID).unwrap();

        let output = std::process::Command::new(wrapper_path(&dir, APP_ID))
            .arg("sh")
            .arg("-c")
            .arg("echo \"arg=$1 env=$WERVIK_TEST_VAR\"")
            .arg("_")
            .arg("real-game-arg")
            .output()
            .unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(stdout.trim(), "arg=real-game-arg env=hello world");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, unix))]
mod real_gamescope_check {
    use super::*;

    /// Requires a real `gamescope` binary — not run by default. Verifies
    /// our exact generated flags (via `gamescope_args`) are accepted by the
    /// real thing, using its headless backend so this can't touch the
    /// actual display or hang: `gamescope --backend headless <flags> --
    /// true` should start, run `true`, and exit 0 within a few seconds.
    #[test]
    #[ignore]
    fn generated_flags_are_accepted_by_real_gamescope() {
        if !is_gamescope_installed() {
            eprintln!("gamescope not installed, skipping");
            return;
        }

        let config = GamescopeConfig {
            internal_width: 1280,
            internal_height: 800,
            output_width: 1920,
            output_height: 1200,
            upscaler: "fsr".into(),
        };
        let args = gamescope_args(&config);
        eprintln!("gamescope_args: {args:?}");

        let output = std::process::Command::new("timeout")
            .arg("15")
            .arg("gamescope")
            .arg("--backend")
            .arg("headless")
            .args(&args)
            .arg("--")
            .arg("true")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "gamescope rejected our flags {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
