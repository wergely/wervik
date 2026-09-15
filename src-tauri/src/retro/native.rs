//! Native (non-EmulatorJS) emulator launching, for platforms where a real
//! installed emulator meaningfully outperforms the browser/WASM path:
//! gopher64 for N64 (its native Vulkan renderer runs dramatically better
//! than any WASM N64 core running in a browser — WASM overhead, WebGL
//! instead of native GL/Vulkan, no real multi-threading without
//! cross-origin-isolation headers this local server doesn't set),
//! DuckStation for PlayStation (the modern standard PS1 emulator — better
//! compatibility and performance than the WASM `mednafen_psx_hw` core),
//! and RetroArch cores for N64, arcade, and Neo Geo CD on Windows (the latter
//! two have no vendored EmulatorJS core — see `Platform::default_core`).
//!
//! Wervik downloads and manages its native emulators itself (see `install`)
//! rather than relying on the system package manager or Flatpak — the same idea
//! as RetroArch managing its own cores. This sidesteps a real, verified
//! problem with gopher64's official Flatpak build: its sandbox has no
//! filesystem permission to see anything under the user's home directory,
//! so it can never actually open a ROM Wervik hands it — reproduced
//! directly via `flatpak run`, independent of Wervik entirely, and fixable
//! only by the user manually running `flatpak override --filesystem=home`,
//! which isn't something Wervik should ask anyone to do. Downloading each
//! emulator's own official Linux release straight from GitHub and running
//! it directly avoids the sandbox — and its permissions — altogether.
//! DuckStation's official Linux release is an AppImage rather than a
//! plain binary, and FBNeo's is a zip containing the binary alongside
//! docs/license files; see `install`/`ManagedKind` for how each is handled.
//!
//! On Windows, N64 uses the managed Mupen64Plus-Next RetroArch core so it
//! follows the same download, configuration, and launch path as the arcade
//! cores. Linux keeps its existing gopher64 path.

use std::path::{Path, PathBuf};
use std::process::Child;

use serde::Deserialize;

use super::platform::Platform;
#[cfg(target_os = "linux")]
use crate::game_setup;
#[cfg(target_os = "linux")]
use crate::mangohud;

#[cfg_attr(target_os = "windows", allow(dead_code))]
#[derive(Debug, Clone, Copy)]
enum Launcher {
    /// A plain binary already on `$PATH` — not used by any currently
    /// registered emulator, but kept for a future one Wervik doesn't
    /// manage itself (the user installs it via their own package manager).
    #[allow(dead_code)]
    Binary(&'static str),
    /// A binary Wervik downloads and manages itself under its own app
    /// data directory (see `install`, `managed_executable_path`) — no
    /// system package, no Flatpak sandbox to fight with.
    Managed,
}

#[derive(Debug, Clone, Copy)]
pub struct NativeEmulator {
    /// Stable id for storage/selection (see `Store::default_emulator`)
    /// — independent of `display_name` so a cosmetic rename never changes
    /// what a saved preference points at.
    pub id: &'static str,
    /// Shown to the user so it's never a silent, unexplained switch in how
    /// a game launches.
    pub display_name: &'static str,
    launcher: Launcher,
    /// Whether this is the best default for its platform — shown as
    /// "Recommended" in Platform Setup instead of EmulatorJS. Verified,
    /// not assumed: gopher64, DuckStation, FBNeo, and the RetroArch cores
    /// all provide a better native path for their supported systems than
    /// outperform (or, for FBNeo/Neo Geo CD, simply exist where) their
    /// WASM equivalents.
    pub recommended: bool,
    /// This emulator's real command-line argument order, with `"{rom}"`
    /// standing in for the actual ROM path — e.g. gopher64's
    /// `["--fullscreen", "{rom}"]`, DuckStation's batch-mode
    /// `["-batch", "-fullscreen", "{rom}"]` (closes when the game exits
    /// instead of returning to its own game-library GUI), or FBNeo's Neo
    /// Geo CD mode, which needs a fixed system name *before* the disc
    /// path: `["neocdz", "-cd", "{rom}", "-fullscreen"]`. Verified against
    /// each emulator's own `--help`/source — never assumed to match
    /// another emulator's argument style or order.
    launch_args: &'static [&'static str],
    /// Arguments to open this emulator's own settings/config UI with no
    /// ROM loaded (see `open_settings`). Empty for gopher64 and DuckStation,
    /// which both open their config UI when launched with no arguments at
    /// all — but FBNeo's binary refuses to start with no romname unless
    /// `-menu` is also given (verified against its real usage text: running
    /// it with zero arguments just prints usage and exits, it does not open
    /// any UI).
    settings_args: &'static [&'static str],
    /// Where this emulator expects its BIOS placed, relative to its own
    /// managed directory — `None` if it doesn't need one, or takes it via
    /// a simple CLI flag instead (neither gopher64 nor DuckStation do).
    /// FBNeo's Neo Geo CD driver expects a MAME-style BIOS romset zip at a
    /// fixed path next to the binary; Wervik just copies the user-selected
    /// BIOS file there as-is (see `install_bios`), trusting them to have
    /// the right one — the same policy as every other BIOS Wervik
    /// handles (see `retro::bios`): never fetched or fabricated by Wervik
    /// itself.
    bios_relpath: Option<&'static str>,
    /// Shared libraries this emulator's release depends on that Wervik
    /// doesn't manage itself (a system package, not something the
    /// download bundles) — e.g. FBNeo's official Linux build is
    /// dynamically linked against `libSDL2_image`, which isn't guaranteed
    /// to already be on the system the way base SDL2 usually is (verified
    /// directly: a real `ldd` on the downloaded binary). Checked in
    /// addition to the executable existing, in `is_available`, so Wervik
    /// never claims something is "Installed" when it would actually fail
    /// to launch.
    required_system_libs: &'static [&'static str],
}

impl NativeEmulator {
    /// Whether Wervik can download and install this emulator itself (see
    /// `install`) — as opposed to a `Launcher::Binary` the user has to
    /// install some other way. Drives whether the Platform Setup UI offers
    /// a "Download" action instead of just showing "Not installed".
    pub fn is_managed(&self) -> bool {
        matches!(self.launcher, Launcher::Managed)
    }
}

/// Every native emulator Wervik knows how to launch for this platform —
/// purely a description of what Wervik *could* use, in a fixed preference
/// order (first = default when nothing's been explicitly chosen). Whether
/// each one is actually installed is a separate, real check (`is_available`).
#[cfg(not(target_os = "windows"))]
pub fn native_emulators_for(platform: Platform) -> Vec<NativeEmulator> {
    match platform {
        Platform::Nintendo64 => vec![NativeEmulator {
            id: "gopher64",
            display_name: "gopher64",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["--fullscreen", "{rom}"],
            settings_args: &[],
            bios_relpath: None,
            required_system_libs: &[],
        }],
        Platform::PlayStation => vec![NativeEmulator {
            id: "duckstation",
            display_name: "DuckStation",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["-batch", "-fullscreen", "{rom}"],
            settings_args: &[],
            bios_relpath: None,
            required_system_libs: &[],
        }],
        // "neocdz" is FBNeo's fixed internal driver name for the Neo Geo
        // CD console itself (verified against its real `-help` usage
        // text) — the actual game disc is handed to it separately via
        // `-cd`, not as the trailing positional argument like every other
        // managed emulator here.
        Platform::NeoGeoCd => vec![NativeEmulator {
            id: "fbneo",
            display_name: "FBNeo",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["neocdz", "-cd", "{rom}", "-fullscreen"],
            settings_args: &["-menu"],
            bios_relpath: Some("roms/neocdz.zip"),
            required_system_libs: &["libSDL2_image"],
        }],
        _ => Vec::new(),
    }
}

/// A plain "one RetroArch core, no BIOS" entry — every platform below that
/// EmulatorJS already fully covers (see `Platform::default_core`) gets one
/// of these as an *additional* choice, never `recommended` over it: these
/// exist for anyone who wants native performance instead of the WASM path,
/// not because EmulatorJS is missing anything for these systems the way it
/// is for Arcade/Neo Geo CD. Each one intentionally downloads the exact
/// same libretro core EmulatorJS already vendors for that platform (see
/// `managed_release_for`) — a native build of a core already trusted here,
/// not an unrelated one that could behave differently.
#[cfg(target_os = "windows")]
fn plain_retroarch_core(id: &'static str, display_name: &'static str) -> NativeEmulator {
    NativeEmulator {
        id,
        display_name,
        launcher: Launcher::Managed,
        recommended: false,
        launch_args: &["-L", "{core}", "{rom}"],
        settings_args: &["--menu"],
        bios_relpath: None,
        required_system_libs: &[],
    }
}

#[cfg(target_os = "windows")]
pub fn native_emulators_for(platform: Platform) -> Vec<NativeEmulator> {
    match platform {
        Platform::Nes => vec![plain_retroarch_core("retroarch-nes", "RetroArch · FCEUmm")],
        Platform::Snes => vec![plain_retroarch_core("retroarch-snes", "RetroArch · Snes9x")],
        Platform::GameBoy => vec![plain_retroarch_core("retroarch-gameboy", "RetroArch · Gambatte")],
        Platform::GameBoyColor => {
            vec![plain_retroarch_core("retroarch-gameboy-color", "RetroArch · Gambatte")]
        }
        Platform::GameBoyAdvance => vec![plain_retroarch_core("retroarch-gba", "RetroArch · mGBA")],
        Platform::MasterSystem => {
            vec![plain_retroarch_core("retroarch-mastersystem", "RetroArch · Genesis Plus GX")]
        }
        Platform::Genesis => vec![plain_retroarch_core("retroarch-genesis", "RetroArch · Genesis Plus GX")],
        Platform::GameGear => vec![plain_retroarch_core("retroarch-gamegear", "RetroArch · Genesis Plus GX")],
        Platform::Sega32X => vec![plain_retroarch_core("retroarch-32x", "RetroArch · PicoDrive")],
        Platform::PlayStation => vec![NativeEmulator {
            id: "retroarch-playstation",
            display_name: "RetroArch · Mednafen PSX",
            launcher: Launcher::Managed,
            recommended: false,
            launch_args: &["-L", "{core}", "{rom}"],
            settings_args: &["--menu"],
            // Same BIOS directory convention as the other RetroArch cores
            // above that need one — mednafen_psx_hw looks directly in
            // RetroArch's `system/` for the PS1 BIOS files by name.
            bios_relpath: Some("system/"),
            required_system_libs: &[],
        }],
        Platform::Atari5200 => vec![plain_retroarch_core("retroarch-atari5200", "RetroArch · a5200")],
        Platform::Atari7800 => vec![plain_retroarch_core("retroarch-atari7800", "RetroArch · ProSystem")],
        Platform::AtariLynx => vec![plain_retroarch_core("retroarch-atarilynx", "RetroArch · Handy")],
        Platform::AtariJaguar => {
            vec![plain_retroarch_core("retroarch-atarijaguar", "RetroArch · Virtual Jaguar")]
        }
        Platform::WonderSwan => {
            vec![plain_retroarch_core("retroarch-wonderswan", "RetroArch · Mednafen WonderSwan")]
        }
        Platform::NeoGeoPocket => {
            vec![plain_retroarch_core("retroarch-neogeopocket", "RetroArch · Mednafen NeoPop")]
        }
        Platform::PcEngine => vec![plain_retroarch_core("retroarch-pcengine", "RetroArch · Mednafen PCE")],
        Platform::ColecoVision => {
            vec![plain_retroarch_core("retroarch-colecovision", "RetroArch · Gearcoleco")]
        }
        Platform::Nintendo64 => vec![NativeEmulator {
            id: "retroarch-mupen64plus-next",
            display_name: "RetroArch · Mupen64Plus-Next",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["-L", "{core}", "{rom}"],
            settings_args: &["--menu"],
            bios_relpath: None,
            required_system_libs: &[],
        }],
        Platform::Arcade => vec![NativeEmulator {
            id: "retroarch-fbneo",
            display_name: "RetroArch · FBNeo",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["-L", "{core}", "{rom}"],
            settings_args: &["--menu"],
            bios_relpath: Some("system/fbneo/"),
            required_system_libs: &[],
        }],
        Platform::NeoGeoCd => vec![
            NativeEmulator {
                id: "retroarch-neocd",
                display_name: "RetroArch · NeoCD",
                launcher: Launcher::Managed,
                recommended: true,
                launch_args: &["-L", "{core}", "{rom}"],
                settings_args: &["--menu"],
                bios_relpath: Some("system/neocd/"),
                required_system_libs: &[],
            },
            NativeEmulator {
                id: "retroarch-geolith",
                display_name: "RetroArch · Geolith",
                launcher: Launcher::Managed,
                recommended: false,
                launch_args: &["-L", "{core}", "{rom}"],
                settings_args: &["--menu"],
                bios_relpath: Some("system/"),
                required_system_libs: &[],
            },
            NativeEmulator {
                id: "retroarch-fbneo",
                display_name: "RetroArch · FBNeo",
                launcher: Launcher::Managed,
                recommended: false,
                launch_args: &["-L", "{core}", "--subsystem", "neocd", "{rom}"],
                settings_args: &["--menu"],
                bios_relpath: Some("system/fbneo/"),
                required_system_libs: &[],
            },
        ],
        // No libretro/EmulatorJS core exists for PS2 at anywhere near
        // usable quality (RetroArch's only PS2 core, "Play!", is far behind
        // standalone PCSX2 in both compatibility and performance) — PCSX2
        // itself is the only real option, launched directly rather than
        // through RetroArch. `-portable` keeps its config/memory
        // cards/BIOS search path inside its own managed directory instead
        // of the user's system config folder (verified against the real
        // binary's `-h` output, same as every flag below).
        Platform::PlayStation2 => vec![NativeEmulator {
            id: "pcsx2",
            display_name: "PCSX2",
            launcher: Launcher::Managed,
            recommended: true,
            launch_args: &["-portable", "-batch", "-fullscreen", "{rom}"],
            settings_args: &["-portable"],
            // PCSX2 (in portable mode) looks for the user-supplied PS2 BIOS
            // dump in a `bios` folder inside its own data directory —
            // verified against the real binary's embedded BIOS-setup text.
            bios_relpath: Some("bios/"),
            required_system_libs: &[],
        }],
    }
}

/// Where a `Launcher::Managed` emulator's real release comes from and what
/// shape the downloaded asset is in. The Windows RetroArch package comes
/// from the official Libretro buildbot rather than GitHub.
#[derive(Debug, Clone, Copy)]
enum DownloadSource {
    Github {
        /// "<owner>/<repo>" on GitHub.
        repo: &'static str,
        /// Exact release asset filename to download.
        asset_name: &'static str,
    },
    Url(&'static str),
}

struct ManagedRelease {
    source: DownloadSource,
    kind: ManagedKind,
}

#[derive(Debug, Clone, Copy)]
enum ManagedKind {
    /// The downloaded asset already is the executable (e.g. gopher64's
    /// own Linux release).
    Binary,
    /// The downloaded asset is an AppImage (e.g. DuckStation's), extracted
    /// once at install time via `--appimage-extract` — verified to work
    /// without libfuse installed, unlike mounting the AppImage directly
    /// would require — into its own `<id>-appimage/` directory (see
    /// `install`); `managed_executable_path` then points at the extracted
    /// `AppRun` inside it.
    AppImage,
    /// The downloaded asset is a zip containing the executable alongside
    /// other files (docs, license — FBNeo's official release) — extracted
    /// once at install time into its own `<id>-files/` directory;
    /// `binary_name` is the executable's name inside it.
    ZippedBinary { binary_name: &'static str },
    /// Same idea as `ZippedBinary`, but the asset is a `.7z` archive (e.g.
    /// PCSX2's official Windows portable release) rather than a `.zip` —
    /// extracted via `sevenz_rust` instead of the `zip` crate, same as
    /// `install_retroarch` already does for RetroArch's own portable package.
    SevenZipBinary { binary_name: &'static str },
    /// RetroArch is distributed as an official Windows portable archive and
    /// its cores as small per-core archives from the official Libretro buildbot.
    RetroArch { core_name: &'static str },
}

/// Whether `emulator_id`'s release is actually RetroArch (as opposed to
/// some other `Launcher::Managed` emulator, e.g. PCSX2) — every managed
/// emulator on Windows used to be RetroArch-based, so `ensure_retroarch_defaults`
/// used to be safe to call for any `Launcher::Managed` emulator; that
/// stopped being true once a non-RetroArch one (PCSX2) existed.
#[cfg(target_os = "windows")]
fn is_retroarch_based(emulator_id: &str) -> bool {
    matches!(
        managed_release_for(emulator_id).map(|release| release.kind),
        Some(ManagedKind::RetroArch { .. })
    )
}

const RETROARCH_WINDOWS_PORTABLE: &str =
    "https://buildbot.libretro.com/stable/1.22.1/windows/x86_64/RetroArch.7z";
const RETROARCH_WINDOWS_CORE_BASE: &str =
    "https://buildbot.libretro.com/nightly/windows/x86_64/latest";

fn managed_release_for(id: &str) -> Option<ManagedRelease> {
    match id {
        "gopher64" => Some(ManagedRelease {
            source: DownloadSource::Github {
                repo: "gopher64/gopher64",
                asset_name: "gopher64-linux-x86_64",
            },
            kind: ManagedKind::Binary,
        }),
        "duckstation" => Some(ManagedRelease {
            source: DownloadSource::Github {
                repo: "stenzek/duckstation",
                asset_name: "DuckStation-x64.AppImage",
            },
            kind: ManagedKind::AppImage,
        }),
        "fbneo" => Some(ManagedRelease {
            source: DownloadSource::Github {
                repo: "finalburnneo/FBNeo",
                asset_name: "linux-sdl2-x86_64.zip",
            },
            kind: ManagedKind::ZippedBinary {
                binary_name: "fbneo",
            },
        }),
        "retroarch-fbneo" => Some(ManagedRelease {
            source: DownloadSource::Url(RETROARCH_WINDOWS_PORTABLE),
            kind: ManagedKind::RetroArch { core_name: "fbneo" },
        }),
        "retroarch-geolith" => Some(ManagedRelease {
            source: DownloadSource::Url(RETROARCH_WINDOWS_PORTABLE),
            kind: ManagedKind::RetroArch {
                core_name: "geolith",
            },
        }),
        "retroarch-neocd" => Some(ManagedRelease {
            source: DownloadSource::Url(RETROARCH_WINDOWS_PORTABLE),
            kind: ManagedKind::RetroArch { core_name: "neocd" },
        }),
        "retroarch-mupen64plus-next" => Some(ManagedRelease {
            source: DownloadSource::Url(RETROARCH_WINDOWS_PORTABLE),
            kind: ManagedKind::RetroArch {
                core_name: "mupen64plus_next",
            },
        }),
        // Every plain `plain_retroarch_core` entry from `native_emulators_for`
        // above — same core name as `Platform::default_core` for that
        // system, verified against the real Libretro buildbot listing
        // rather than assumed.
        "retroarch-nes" => Some(retroarch_core("fceumm")),
        "retroarch-snes" => Some(retroarch_core("snes9x")),
        "retroarch-gameboy" | "retroarch-gameboy-color" => Some(retroarch_core("gambatte")),
        "retroarch-gba" => Some(retroarch_core("mgba")),
        "retroarch-mastersystem" | "retroarch-genesis" | "retroarch-gamegear" => {
            Some(retroarch_core("genesis_plus_gx"))
        }
        "retroarch-32x" => Some(retroarch_core("picodrive")),
        "retroarch-playstation" => Some(retroarch_core("mednafen_psx_hw")),
        "retroarch-atari5200" => Some(retroarch_core("a5200")),
        "retroarch-atari7800" => Some(retroarch_core("prosystem")),
        "retroarch-atarilynx" => Some(retroarch_core("handy")),
        "retroarch-atarijaguar" => Some(retroarch_core("virtualjaguar")),
        "retroarch-wonderswan" => Some(retroarch_core("mednafen_wswan")),
        "retroarch-neogeopocket" => Some(retroarch_core("mednafen_ngp")),
        "retroarch-pcengine" => Some(retroarch_core("mednafen_pce")),
        "retroarch-colecovision" => Some(retroarch_core("gearcoleco")),
        // PCSX2's official Windows release is a portable `.7z` with
        // `pcsx2-qt.exe` sitting at the archive root (verified against the
        // real v2.8.2 asset) — `asset_name`'s `*` stands in for the version
        // number the real filename embeds (e.g. `pcsx2-v2.8.2-windows-x64-Qt.7z`),
        // which changes on every release.
        "pcsx2" => Some(ManagedRelease {
            source: DownloadSource::Github {
                repo: "PCSX2/pcsx2",
                asset_name: "pcsx2-*-windows-x64-Qt.7z",
            },
            kind: ManagedKind::SevenZipBinary {
                binary_name: "pcsx2-qt.exe",
            },
        }),
        _ => None,
    }
}

/// The standard shape of a managed RetroArch core release: the same
/// official portable RetroArch package, with only the core name varying.
fn retroarch_core(core_name: &'static str) -> ManagedRelease {
    ManagedRelease {
        source: DownloadSource::Url(RETROARCH_WINDOWS_PORTABLE),
        kind: ManagedKind::RetroArch { core_name },
    }
}

fn managed_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("emulators")
}

/// The real, directly-runnable executable for a managed emulator, whatever
/// shape its release actually ships in.
fn managed_executable_path(app_data_dir: &Path, id: &str) -> PathBuf {
    match managed_release_for(id).map(|r| r.kind) {
        Some(ManagedKind::RetroArch { .. }) => retroarch_executable_path(app_data_dir),
        Some(ManagedKind::AppImage) => managed_dir(app_data_dir)
            .join(format!("{id}-appimage"))
            .join("squashfs-root")
            .join("AppRun"),
        Some(ManagedKind::ZippedBinary { binary_name })
        | Some(ManagedKind::SevenZipBinary { binary_name }) => managed_dir(app_data_dir)
            .join(format!("{id}-files"))
            .join(binary_name),
        _ => managed_dir(app_data_dir).join(id),
    }
}

/// The official portable RetroArch archive currently has a top-level
/// `RetroArch-Win64` directory, while older packages placed the executable at
/// the archive root. Keep both layouts working so an already extracted
/// package can be reused without another 200 MB download.
fn retroarch_executable_path(app_data_dir: &Path) -> PathBuf {
    let package_dir = managed_dir(app_data_dir).join("retroarch");
    let fallback = package_dir.join("retroarch.exe");
    find_retroarch_executable(&package_dir, 2).unwrap_or(fallback)
}

#[cfg(target_os = "windows")]
const RETROARCH_DEFAULTS_MARKER: &str = ".wervik-defaults-v2";

/// Sets one RetroArch key without discarding the rest of the user's config.
/// RetroArch rewrites this file itself, so this intentionally edits only the
/// first-level `key = "value"` entry and leaves comments and unrelated
/// settings alone.
#[cfg(target_os = "windows")]
fn set_retroarch_option(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };

    let mut output = String::with_capacity(contents.len() + key.len() + value.len() + 8);
    let mut replaced = false;
    for raw_line in contents.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        let is_target = !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(left, _)| left.trim() == key);
        if is_target {
            let indentation = &line[..line.len() - trimmed.len()];
            output.push_str(indentation);
            output.push_str(key);
            output.push_str(" = \"");
            output.push_str(value);
            output.push_str("\"\n");
            replaced = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !replaced {
        output.push_str(key);
        output.push_str(" = \"");
        output.push_str(value);
        output.push_str("\"\n");
    }
    // A per-core options file (e.g. `config/NeoCD/NeoCD.opt`) lives in a
    // directory RetroArch only creates itself the first time that core
    // actually saves its own options at runtime — on a freshly extracted
    // RetroArch, `config/<CoreName>/` doesn't exist yet, so writing here
    // ahead of that failed with "path not found" (Windows os error 3) on a
    // brand-new install.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, output).map_err(|error| error.to_string())
}

/// The managed RetroArch copy must remain at normal content speed on Windows
/// even when the desktop monitor runs above 60 Hz. RetroArch normally uses
/// audio synchronization as its timing anchor, so a working audio backend is
/// part of the default rather than leaving it to whichever backend happens to
/// be selected on the host.
/// Apply this once so the user can still change settings in RetroArch later
/// without Wervik overwriting them on every launch.
#[cfg(target_os = "windows")]
fn ensure_retroarch_defaults(app_data_dir: &Path) -> Result<(), String> {
    let executable = retroarch_executable_path(app_data_dir);
    let runtime_dir = executable
        .parent()
        .ok_or_else(|| "Could not determine RetroArch's installation directory.".to_string())?;
    let marker = runtime_dir.join(RETROARCH_DEFAULTS_MARKER);
    if marker.is_file() {
        return Ok(());
    }

    let config_path = runtime_dir.join("retroarch.cfg");
    for (key, value) in [
        // WASAPI can reject the shared device format on otherwise healthy
        // Windows endpoints. DirectSound works with the same endpoints and
        // keeps RetroArch's audio clock available for frame pacing.
        ("audio_driver", "dsound"),
        ("audio_sync", "true"),
        ("audio_out_rate", "48000"),
        ("video_vsync", "true"),
        ("video_swap_interval", "1"),
        ("video_threaded", "false"),
        ("run_ahead_enabled", "false"),
        // A value of 1.0 provides a real 1x cap if the audio driver cannot
        // initialize, while keeping ordinary content at its native rate.
        ("fastforward_ratio", "1.000000"),
    ] {
        set_retroarch_option(&config_path, key, value)?;
    }

    // NeoCD's loadskip is useful for testing but makes the default boot
    // sequence look like a game is running at turbo speed. Keep the actual
    // loading sequence and leave the speed hack/CPU clock at safe defaults.
    let neocd_options = runtime_dir.join("config").join("NeoCD").join("NeoCD.opt");
    for (key, value) in [
        ("neocd_loadskip", "Off"),
        ("neocd_cdspeedhack", "Off"),
        ("neocd_cpu_overclock", "100%"),
    ] {
        set_retroarch_option(&neocd_options, key, value)?;
    }

    std::fs::write(marker, b"Wervik managed RetroArch defaults\n")
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn retroarch_config_path(app_data_dir: &Path) -> Result<PathBuf, String> {
    let executable = retroarch_executable_path(app_data_dir);
    if !executable.is_file() {
        return Err(
            "Install a RetroArch-based emulator first (e.g. for Arcade or Neo Geo, in Platforms) — \
             RetroAchievements only applies to games launched through it."
                .to_string(),
        );
    }
    let runtime_dir = executable
        .parent()
        .ok_or_else(|| "Could not determine RetroArch's installation directory.".to_string())?;
    Ok(runtime_dir.join("retroarch.cfg"))
}

/// Points the managed RetroArch install's own RetroAchievements engine
/// (real-time unlock popups, hardcore mode) at `username`. RetroArch
/// exchanges `password` for a session token and overwrites its own config
/// with that token on next successful login — Wervik never stores this
/// password itself, only relays it into RetroArch's config this once.
#[cfg(target_os = "windows")]
pub fn enable_retroarch_achievements(
    app_data_dir: &Path,
    username: &str,
    password: &str,
    hardcore: bool,
) -> Result<(), String> {
    let config_path = retroarch_config_path(app_data_dir)?;
    set_retroarch_option(&config_path, "cheevos_enable", "true")?;
    set_retroarch_option(&config_path, "cheevos_username", username)?;
    set_retroarch_option(&config_path, "cheevos_password", password)?;
    set_retroarch_option(
        &config_path,
        "cheevos_hardcore_mode_enable",
        if hardcore { "true" } else { "false" },
    )
}

#[cfg(target_os = "windows")]
pub fn disable_retroarch_achievements(app_data_dir: &Path) -> Result<(), String> {
    let config_path = retroarch_config_path(app_data_dir)?;
    set_retroarch_option(&config_path, "cheevos_enable", "false")
}

#[cfg(not(target_os = "windows"))]
pub fn enable_retroarch_achievements(
    _app_data_dir: &Path,
    _username: &str,
    _password: &str,
    _hardcore: bool,
) -> Result<(), String> {
    Err("RetroArch achievements setup is only available on Windows.".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn disable_retroarch_achievements(_app_data_dir: &Path) -> Result<(), String> {
    Err("RetroArch achievements setup is only available on Windows.".to_string())
}

/// Whether `platform`'s native RetroArch core can meaningfully render
/// above the original hardware's own resolution — the only two platforms
/// where "Optimized" (see `apply_graphics_preset`) is a real, well-known
/// enhancement rather than just a subjective shader/filter choice. Every
/// 2D system Wervik supports already renders at its one native resolution,
/// so there's nothing to offer there.
pub fn supports_graphics_preset(platform: Platform) -> bool {
    matches!(platform, Platform::Nintendo64 | Platform::PlayStation)
}

/// Sets the internal render resolution for the platforms above, via each
/// core's own option file — same mechanism as `set_retroarch_option`, just
/// under a per-core `config/<core name>/<core name>.opt` rather than the
/// shared `retroarch.cfg`, which is where RetroArch itself keeps per-core
/// options. `"Optimized"` always writes an explicit value rather than just
/// leaving the option unset, so switching back to `"Original"` is a real,
/// reversible write instead of relying on whatever the core's own default
/// happens to be.
///
/// Core option keys and corename-derived paths verified against each
/// core's real `libretro_core_options.h` / `.info` file rather than
/// assumed — `mupen64plus-next-43screensize`/`-169screensize` from
/// `libretro/mupen64plus-libretro-nx`, `beetle_psx_hw_internal_resolution`
/// from `libretro/beetle-psx-libretro` (the Mednafen PSX HW core keeps its
/// original "Beetle" option prefix and corename for compatibility with
/// existing configs, even though the shipped binary is
/// `mednafen_psx_hw_libretro.dll`).
#[cfg(target_os = "windows")]
pub fn apply_graphics_preset(app_data_dir: &Path, platform: Platform, optimized: bool) -> Result<(), String> {
    let (core_name, options): (&str, &[(&str, &str)]) = match platform {
        Platform::Nintendo64 => (
            "Mupen64Plus-Next",
            if optimized {
                &[
                    ("mupen64plus-next-43screensize", "1920x1440"),
                    ("mupen64plus-next-169screensize", "1920x1080"),
                ]
            } else {
                &[
                    ("mupen64plus-next-43screensize", "640x480"),
                    ("mupen64plus-next-169screensize", "640x360"),
                ]
            },
        ),
        Platform::PlayStation => (
            "Beetle PSX HW",
            if optimized {
                &[("beetle_psx_hw_internal_resolution", "4x")]
            } else {
                &[("beetle_psx_hw_internal_resolution", "1x(native)")]
            },
        ),
        _ => {
            return Err(format!(
                "{} has no graphics preset to apply.",
                platform.display_name()
            ))
        }
    };

    let executable = retroarch_executable_path(app_data_dir);
    if !executable.is_file() {
        return Err(
            "Install this platform's RetroArch core first, in Platform Setup.".to_string(),
        );
    }
    let runtime_dir = executable
        .parent()
        .ok_or_else(|| "Could not determine RetroArch's installation directory.".to_string())?;
    let options_path = runtime_dir
        .join("config")
        .join(core_name)
        .join(format!("{core_name}.opt"));
    for (key, value) in options {
        set_retroarch_option(&options_path, key, value)?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn apply_graphics_preset(_app_data_dir: &Path, _platform: Platform, _optimized: bool) -> Result<(), String> {
    Err("Graphics presets are only available on Windows.".to_string())
}

fn find_retroarch_executable(dir: &Path, remaining_depth: u8) -> Option<PathBuf> {
    let direct = dir.join("retroarch.exe");
    if direct.is_file() {
        return Some(direct);
    }
    if remaining_depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_retroarch_executable(&path, remaining_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn managed_core_path(app_data_dir: &Path, id: &str) -> Option<PathBuf> {
    let Some(ManagedKind::RetroArch { core_name }) = managed_release_for(id).map(|r| r.kind) else {
        return None;
    };
    let executable = managed_executable_path(app_data_dir, id);
    let runtime_dir = executable
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| managed_dir(app_data_dir).join("retroarch"));
    Some(
        runtime_dir
            .join("cores")
            .join(format!("{core_name}_libretro.dll")),
    )
}

pub fn is_available(app_data_dir: &Path, emulator: &NativeEmulator) -> bool {
    let executable_present = match emulator.launcher {
        Launcher::Binary(bin) => crate::sysutil::binary_installed(bin),
        Launcher::Managed => managed_executable_path(app_data_dir, emulator.id).is_file(),
    };
    let core_present = managed_core_path(app_data_dir, emulator.id)
        .map(|path| path.is_file())
        .unwrap_or(true);
    executable_present
        && core_present
        && emulator
            .required_system_libs
            .iter()
            .all(|lib| crate::sysutil::shared_library_available(lib))
}

/// A single download can take a while on a slower connection — this is a
/// one-shot user-initiated install, not a background job, so it's worth
/// waiting well past `artwork::new_agent`'s short per-request timeout
/// rather than failing a slow-but-working download.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
/// RetroArch's Windows installer is currently a little over 200 MB. Keep a
/// bounded response size while leaving room for the official package to grow.
const MAX_DOWNLOAD_SIZE: u64 = 320 * 1024 * 1024;

#[derive(Deserialize)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    assets: Vec<GithubReleaseAsset>,
}

fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into()
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Most managed releases (DuckStation, gopher64, FBNeo...) ship under a
/// fixed asset filename every release, so an exact match is enough. PCSX2's
/// real Windows asset embeds its version in the name (e.g.
/// `pcsx2-v2.8.2-windows-x64-Qt.7z`, verified against its actual GitHub
/// release), which would silently stop matching on every new release if
/// hardcoded — `pattern` may contain one `*` standing in for that varying
/// part.
fn asset_name_matches(actual: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => actual.starts_with(prefix) && actual.ends_with(suffix),
        None => actual == pattern,
    }
}

fn download_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    agent
        .get(url)
        .header("User-Agent", "Wervik")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_SIZE)
        .read_to_vec()
        .map_err(|e| format!("Download failed: {e}"))
}

fn download_managed_release(
    agent: &ureq::Agent,
    release: &ManagedRelease,
    display_name: &str,
) -> Result<Vec<u8>, String> {
    let url = match release.source {
        DownloadSource::Url(url) => url.to_string(),
        DownloadSource::Github { repo, asset_name } => {
            let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
            let github_release: GithubRelease = agent
                .get(&api_url)
                .header("User-Agent", "Wervik")
                .call()
                .map_err(|e| format!("Could not reach GitHub: {e}"))?
                .body_mut()
                .read_json()
                .map_err(|e| format!("Could not read GitHub's release info: {e}"))?;
            github_release
                .assets
                .iter()
                .find(|asset| asset_name_matches(&asset.name, asset_name))
                .map(|asset| asset.browser_download_url.clone())
                .ok_or_else(|| {
                    format!("{}'s latest release has no compatible build.", display_name)
                })?
        }
    };
    download_bytes(agent, &url)
}

fn install_retroarch_core(bytes: Vec<u8>, cores_dir: &Path, core_name: &str) -> Result<(), String> {
    let expected_name = format!("{core_name}_libretro.dll");
    std::fs::create_dir_all(cores_dir).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Could not read the RetroArch core archive: {e}"))?;

    let mut installed = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("Could not read the RetroArch core archive: {e}"))?;
        let Some(file_name) = Path::new(entry.name()).file_name() else {
            continue;
        };
        if !file_name.eq_ignore_ascii_case(std::ffi::OsStr::new(&expected_name)) {
            continue;
        }
        let destination = cores_dir.join(&expected_name);
        let mut output = std::fs::File::create(&destination).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        installed = true;
        break;
    }

    if installed {
        Ok(())
    } else {
        Err(format!(
            "The RetroArch download did not contain {expected_name}."
        ))
    }
}

fn install_retroarch(
    app_data_dir: &Path,
    agent: &ureq::Agent,
    emulator: NativeEmulator,
    core_name: &str,
) -> Result<(), String> {
    let dir = managed_dir(app_data_dir);
    let retroarch_dir = dir.join("retroarch");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join("retroarch-installer.exe"));

    let mut executable = managed_executable_path(app_data_dir, emulator.id);
    if !executable.is_file() {
        let portable_bytes = download_managed_release(
            agent,
            &managed_release_for(emulator.id).expect("RetroArch release must exist"),
            emulator.display_name,
        )?;

        // Use the official portable 7z package instead of setup.exe. The
        // latter carries an elevation manifest and Windows rejects
        // CreateProcess with ERROR_ELEVATION_REQUIRED before its silent
        // install flags can take effect. Extracting the archive in Wervik's
        // app data keeps this install self-contained and UAC-free.
        let _ = std::fs::remove_dir_all(&retroarch_dir);
        sevenz_rust::decompress(std::io::Cursor::new(portable_bytes), &retroarch_dir)
            .map_err(|e| format!("Could not extract the RetroArch package: {e}"))?;

        executable = managed_executable_path(app_data_dir, emulator.id);

        if !executable.is_file() {
            return Err(format!(
                "The RetroArch package was extracted, but {} was not found.",
                executable.display()
            ));
        }
    }

    let core_url = format!("{RETROARCH_WINDOWS_CORE_BASE}/{core_name}_libretro.dll.zip");
    let core_bytes = download_bytes(agent, &core_url)?;
    let runtime_dir = executable
        .parent()
        .ok_or_else(|| "Could not determine RetroArch's installation directory.".to_string())?;
    install_retroarch_core(core_bytes, &runtime_dir.join("cores"), core_name)?;

    #[cfg(target_os = "windows")]
    ensure_retroarch_defaults(app_data_dir)?;

    Ok(())
}

/// Downloads `platform`'s `emulator_id` emulator from its official release
/// channel (the same binary its maintainers publish, not a third-party
/// repackaging) and installs it into Wervik's managed
/// emulators directory, replacing any previous copy. Only emulators
/// registered with `Launcher::Managed` can be installed this way — this
/// is not a generic "install anything" hook.
pub fn install(app_data_dir: &Path, platform: Platform, emulator_id: &str) -> Result<(), String> {
    let emulator = native_emulators_for(platform)
        .into_iter()
        .find(|e| e.id == emulator_id)
        .ok_or_else(|| format!("\"{emulator_id}\" is not a known emulator for this platform."))?;
    if !emulator.is_managed() {
        return Err(format!(
            "{} isn't managed by Wervik.",
            emulator.display_name
        ));
    }
    let release = managed_release_for(emulator.id)
        .ok_or_else(|| format!("{} has no known release source.", emulator.display_name))?;

    let agent = download_agent();
    if let ManagedKind::RetroArch { core_name } = release.kind {
        return install_retroarch(app_data_dir, &agent, emulator, core_name);
    }

    let bytes = download_managed_release(&agent, &release, emulator.display_name)?;

    let dir = managed_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    match release.kind {
        ManagedKind::Binary => {
            let dest = dir.join(emulator.id);
            std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
            make_executable(&dest)?;
        }
        ManagedKind::AppImage => {
            // Extracted fresh each install/update, so a re-install never
            // mixes an old extraction's files with the new one.
            let appimage_dir = dir.join(format!("{}-appimage", emulator.id));
            let _ = std::fs::remove_dir_all(&appimage_dir);
            std::fs::create_dir_all(&appimage_dir).map_err(|e| e.to_string())?;

            let downloaded = appimage_dir.join("download.AppImage");
            std::fs::write(&downloaded, bytes).map_err(|e| e.to_string())?;
            make_executable(&downloaded)?;

            // `--appimage-extract` is a built-in AppImage runtime feature
            // that unpacks the bundled squashfs directly (into
            // `./squashfs-root`, relative to the working directory) —
            // verified to work even without libfuse installed, unlike
            // running the AppImage directly (which needs FUSE to mount
            // it). Extracting once here means every actual game launch
            // afterwards runs the real binary directly, no FUSE, no
            // per-launch extraction overhead.
            let status = std::process::Command::new(&downloaded)
                .arg("--appimage-extract")
                .current_dir(&appimage_dir)
                .status()
                .map_err(|e| format!("Could not extract {}: {e}", emulator.display_name))?;
            let _ = std::fs::remove_file(&downloaded);
            if !status.success() {
                return Err(format!("Could not extract {}.", emulator.display_name));
            }
        }
        ManagedKind::ZippedBinary { binary_name } => {
            // Extracted fresh each install/update, same reasoning as the
            // AppImage case above.
            let extract_dir = dir.join(format!("{}-files", emulator.id));
            let _ = std::fs::remove_dir_all(&extract_dir);
            std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;

            let cursor = std::io::Cursor::new(bytes);
            let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
            archive.extract(&extract_dir).map_err(|e| e.to_string())?;

            make_executable(&extract_dir.join(binary_name))?;
        }
        ManagedKind::SevenZipBinary { binary_name } => {
            // Extracted fresh each install/update, same reasoning as the
            // AppImage/ZippedBinary cases above.
            let extract_dir = dir.join(format!("{}-files", emulator.id));
            let _ = std::fs::remove_dir_all(&extract_dir);

            sevenz_rust::decompress(std::io::Cursor::new(bytes), &extract_dir)
                .map_err(|e| format!("Could not extract {}: {e}", emulator.display_name))?;

            make_executable(&extract_dir.join(binary_name))?;
        }
        ManagedKind::RetroArch { .. } => {
            unreachable!("RetroArch is installed by install_retroarch")
        }
    }

    Ok(())
}

/// Copies `bios_source` to wherever `emulator` expects its BIOS placed
/// (see `NativeEmulator::bios_relpath`) — a harmless no-op if this
/// emulator doesn't need one placed this way. A path ending in `/` denotes a
/// directory and keeps the source file name, which is how RetroArch core
/// firmware paths are laid out. Called before every launch (see
/// `commands::launch_retro_game`) rather than only once at BIOS-set time, so
/// switching *which* file is configured as the BIOS always takes effect on
/// the next play, the same as every other BIOS Wervik handles.
pub fn install_bios(
    app_data_dir: &Path,
    emulator: &NativeEmulator,
    bios_source: &Path,
) -> Result<(), String> {
    let Some(relpath) = emulator.bios_relpath else {
        return Ok(());
    };
    let base_dir = managed_executable_path(app_data_dir, emulator.id)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            format!(
                "Could not determine where {} expects its files.",
                emulator.display_name
            )
        })?;
    let file_name = bios_source
        .file_name()
        .ok_or_else(|| "Selected BIOS path has no file name".to_string())?;

    if emulator.id == "retroarch-geolith" {
        return install_geolith_bios(app_data_dir, bios_source);
    }

    let dest = if relpath.ends_with('/') {
        base_dir.join(relpath).join(file_name)
    } else {
        base_dir.join(relpath)
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(bios_source, &dest).map_err(|e| e.to_string())?;
    Ok(())
}

/// Geolith follows the MAME archive layout and looks for `neocd.bin` inside
/// `neocdz.zip`. Some valid BIOS packs used by other Neo Geo CD cores contain
/// the exact same ROM under the name `neocdz.bin`. Normalize that member in
/// RetroArch's private copy so one user-selected BIOS works with both naming
/// conventions without modifying the user's original file.
fn install_geolith_bios(app_data_dir: &Path, bios_source: &Path) -> Result<(), String> {
    let executable = managed_executable_path(app_data_dir, "retroarch-geolith");
    let base_dir = executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not determine RetroArch's installation directory.".to_string())?;
    let system_dir = base_dir.join("system");
    std::fs::create_dir_all(&system_dir).map_err(|e| e.to_string())?;
    let destination = system_dir.join("neocdz.zip");

    let is_zip = bios_source
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    let temporary = system_dir.join("neocdz.wervik.tmp.zip");
    let output = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(output);
    let options = zip::write::SimpleFileOptions::default();
    let mut has_cd_bios = false;
    let mut has_logo_rom = false;

    if is_zip {
        let input = std::fs::File::open(bios_source).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(input).map_err(|e| e.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let entry_name = if Path::new(entry.name())
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(std::ffi::OsStr::new("neocdz.bin")))
            {
                "neocd.bin".to_string()
            } else {
                entry.name().to_string()
            };
            if entry_name.eq_ignore_ascii_case("neocd.bin") {
                has_cd_bios = true;
            }
            if Path::new(entry.name())
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(std::ffi::OsStr::new("000-lo.lo")))
            {
                has_logo_rom = true;
            }
            writer
                .start_file(entry_name, options)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut writer).map_err(|e| e.to_string())?;
        }
    } else {
        writer
            .start_file("neocd.bin", options)
            .map_err(|e| e.to_string())?;
        has_cd_bios = true;
        let mut input = std::fs::File::open(bios_source).map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut writer).map_err(|e| e.to_string())?;
    }

    writer.finish().map_err(|e| e.to_string())?;
    if !has_cd_bios || !has_logo_rom {
        let _ = std::fs::remove_file(&temporary);
        return Err(
            "Geolith requires a complete Neo Geo CD MAME BIOS containing neocd.bin and 000-lo.lo. Use NeoCD with the current BIOS or configure a complete MAME BIOS pack."
                .to_string(),
        );
    }
    let _ = std::fs::remove_file(&destination);
    std::fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    Ok(())
}

fn emulator_path(app_data_dir: &Path, emulator: &NativeEmulator) -> String {
    match emulator.launcher {
        Launcher::Binary(bin) => bin.to_string(),
        Launcher::Managed => managed_executable_path(app_data_dir, emulator.id)
            .to_string_lossy()
            .to_string(),
    }
}

/// Launches the emulator's own UI with no ROM — controller mapping, video
/// backend, audio, and every other in-emulator setting is real
/// functionality gopher64 (or a future native emulator) already has and
/// Wervik has no business reimplementing or faking a Wervik-side control
/// for. Not wrapped in MangoHud/GameMode/Gamescope (see `spawn`) since
/// this is a settings screen, not a game session.
pub fn open_settings(app_data_dir: &Path, emulator: NativeEmulator) -> Result<Child, String> {
    #[cfg(target_os = "windows")]
    if is_retroarch_based(emulator.id) {
        ensure_retroarch_defaults(app_data_dir)?;
    }

    let path = emulator_path(app_data_dir, &emulator);
    let mut cmd = std::process::Command::new(&path);
    cmd.args(emulator.settings_args);
    if matches!(emulator.launcher, Launcher::Managed) {
        if let Some(dir) = managed_executable_path(app_data_dir, emulator.id).parent() {
            cmd.current_dir(dir);
        }
    }
    cmd.spawn()
        .map_err(|e| format!("Could not start {path}: {e}"))
}

/// Spawns the native emulator on `rom_path` as its own separate process,
/// composing in whichever of MangoHud/GameMode/Gamescope are enabled for
/// `platform_key` (see `commands::apply_platform_setup`) — the same three
/// tools Steam games get, just applied directly to this command instead
/// of through a generated wrapper script, since Wervik is the one
/// spawning the process here rather than Steam.
///
/// Composition order mirrors `game_setup::regenerate_wrapper`: gamescope
/// outermost, then gamemode, then mangohud, then the emulator itself.
///
/// Wervik doesn't embed or manage the emulator's window the way it does
/// an EmulatorJS session, only tracks when the process exits (see
/// `commands::launch_retro_game`) so playtime can still be recorded.
pub fn spawn(
    app_data_dir: &Path,
    emulator: NativeEmulator,
    rom_path: &Path,
    setup_dir: &Path,
    platform_key: &str,
) -> Result<Child, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = (setup_dir, platform_key);
        if is_retroarch_based(emulator.id) {
            ensure_retroarch_defaults(app_data_dir)?;
        }
    }

    let argv = build_argv(app_data_dir, emulator, rom_path, setup_dir, platform_key);

    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    // A managed emulator resolves its own relative paths (config, saves,
    // and — for FBNeo specifically — its `roms/` BIOS directory, see
    // `install_bios`) against its working directory, not the exe's own
    // location, on Linux — so run it from there rather than wherever
    // Wervik's own process happens to be.
    if matches!(emulator.launcher, Launcher::Managed) {
        if let Some(dir) = managed_executable_path(app_data_dir, emulator.id).parent() {
            cmd.current_dir(dir);
        }
    }
    #[cfg(target_os = "linux")]
    if mangohud::is_enabled(setup_dir, platform_key) {
        cmd.env("MANGOHUD", "1");
        cmd.env(
            "MANGOHUD_CONFIGFILE",
            mangohud::config_path(setup_dir, platform_key),
        );
    }
    cmd.spawn()
        .map_err(|e| format!("Could not start {}: {e}", argv[0]))
}

/// Pure argv construction, kept separate from `spawn` so the composition
/// order can be tested directly without actually launching a process.
fn build_argv(
    app_data_dir: &Path,
    emulator: NativeEmulator,
    rom_path: &Path,
    setup_dir: &Path,
    platform_key: &str,
) -> Vec<String> {
    #[cfg(target_os = "windows")]
    let _ = (setup_dir, platform_key);

    let rom_path = rom_path.to_string_lossy();
    let mut argv = vec![emulator_path(app_data_dir, &emulator)];
    argv.extend(emulator.launch_args.iter().map(|arg| {
        if *arg == "{rom}" {
            rom_path.to_string()
        } else if *arg == "{core}" {
            managed_core_path(app_data_dir, emulator.id)
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            arg.to_string()
        }
    }));

    #[cfg(target_os = "linux")]
    {
        if mangohud::is_enabled(setup_dir, platform_key) {
            argv.insert(0, "mangohud".to_string());
        }
        if game_setup::is_gamemode_enabled(setup_dir, platform_key) {
            argv.insert(0, "gamemoderun".to_string());
        }
        if game_setup::is_gamescope_enabled(setup_dir, platform_key) {
            let flags = game_setup::gamescope_args(&game_setup::read_gamescope_config(
                setup_dir,
                platform_key,
            ));
            let mut wrapped = vec!["gamescope".to_string()];
            wrapped.extend(flags);
            wrapped.push("--".to_string());
            wrapped.extend(argv);
            argv = wrapped;
        }
    }

    argv
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wervik-native-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn each_platform_with_a_native_emulator_has_exactly_the_one_expected() {
        assert_eq!(native_emulators_for(Platform::Nintendo64).len(), 1);
        assert_eq!(native_emulators_for(Platform::Nintendo64)[0].id, "gopher64");
        assert_eq!(native_emulators_for(Platform::PlayStation).len(), 1);
        assert_eq!(
            native_emulators_for(Platform::PlayStation)[0].id,
            "duckstation"
        );
        assert_eq!(native_emulators_for(Platform::NeoGeoCd).len(), 1);
        assert_eq!(native_emulators_for(Platform::NeoGeoCd)[0].id, "fbneo");
        assert!(native_emulators_for(Platform::Snes).is_empty());
    }

    #[test]
    fn gopher64_duckstation_and_fbneo_are_all_managed_by_wervik() {
        for platform in [
            Platform::Nintendo64,
            Platform::PlayStation,
            Platform::NeoGeoCd,
        ] {
            let emulator = native_emulators_for(platform).into_iter().next().unwrap();
            assert!(
                emulator.is_managed(),
                "{} should be managed by Wervik",
                emulator.display_name
            );
        }
    }

    #[test]
    fn a_managed_binary_emulator_is_available_once_its_file_exists() {
        let dir = scratch_dir("availability-binary");
        let gopher = native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .next()
            .unwrap();

        assert!(!is_available(&dir, &gopher));

        std::fs::create_dir_all(managed_dir(&dir)).unwrap();
        std::fs::write(managed_executable_path(&dir, "gopher64"), b"fake binary").unwrap();
        assert!(is_available(&dir, &gopher));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A managed AppImage emulator's executable path points into its own
    /// extracted directory, not a flat file directly under `emulators/`
    /// (see `install`'s `ManagedKind::AppImage` handling) — this is what
    /// `is_available` checks for DuckStation.
    #[test]
    fn a_managed_appimage_emulators_executable_lives_in_its_extracted_directory() {
        let dir = scratch_dir("availability-appimage");
        let duckstation = native_emulators_for(Platform::PlayStation)
            .into_iter()
            .next()
            .unwrap();

        assert!(!is_available(&dir, &duckstation));

        let apprun = managed_executable_path(&dir, "duckstation");
        assert!(apprun.ends_with("duckstation-appimage/squashfs-root/AppRun"));
        std::fs::create_dir_all(apprun.parent().unwrap()).unwrap();
        std::fs::write(&apprun, b"fake apprun").unwrap();
        assert!(is_available(&dir, &duckstation));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sample_emulator() -> NativeEmulator {
        NativeEmulator {
            id: "fake",
            display_name: "Fake",
            launcher: Launcher::Binary("fake-emulator"),
            recommended: false,
            launch_args: &["--fullscreen", "{rom}"],
            settings_args: &[],
            bios_relpath: None,
            required_system_libs: &[],
        }
    }

    /// `open_settings` launches the emulator's own UI directly — no ROM
    /// argument, no gamescope/gamemode/mangohud wrapping, since it's a
    /// settings screen, not a game session. Uses the real `true` binary
    /// (guaranteed present on any Linux box) rather than a mock, so this
    /// proves the process really spawns and runs.
    #[test]
    fn open_settings_launches_the_emulator_with_no_rom_argument() {
        let dir = scratch_dir("open-settings");
        let emulator = NativeEmulator {
            id: "true",
            display_name: "true",
            launcher: Launcher::Binary("true"),
            recommended: false,
            launch_args: &[],
            settings_args: &[],
            bios_relpath: None,
            required_system_libs: &[],
        };

        let mut child = open_settings(&dir, emulator).unwrap();
        let status = child.wait().unwrap();

        assert!(status.success());
    }

    /// FBNeo's binary refuses to start with zero arguments — it just
    /// prints its usage text and exits (verified for real: this is the
    /// exact failure the user hit before `settings_args` existed). It only
    /// opens its own config UI when given `-menu` with no romname, so
    /// `open_settings` must actually pass that flag through rather than
    /// launching bare the way gopher64/DuckStation do. Proven with a real
    /// script that inspects its own argv and only exits successfully when
    /// `-menu` is present — not just asserting the field's value.
    #[test]
    fn fbneo_settings_are_opened_with_the_menu_flag() {
        let dir = scratch_dir("open-settings-fbneo");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("assert-menu-flag.sh");
        std::fs::write(&script, "#!/bin/sh\n[ \"$1\" = \"-menu\" ]\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let script_path: &'static str =
            Box::leak(script.to_string_lossy().into_owned().into_boxed_str());

        let mut fbneo = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .next()
            .unwrap();
        fbneo.launcher = Launcher::Binary(script_path);

        let mut child = open_settings(&dir, fbneo).unwrap();
        let status = child.wait().unwrap();

        assert!(
            status.success(),
            "open_settings must pass -menu through to FBNeo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_argv_when_nothing_is_enabled() {
        let dir = scratch_dir("argv-plain");

        let argv = build_argv(
            &dir,
            sample_emulator(),
            Path::new("/roms/game.z64"),
            &dir,
            "test-platform",
        );

        assert_eq!(
            argv,
            vec!["fake-emulator", "--fullscreen", "/roms/game.z64"]
        );
    }

    #[test]
    fn a_managed_emulator_is_launched_via_its_absolute_installed_path() {
        let dir = scratch_dir("argv-managed");
        let gopher = native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .next()
            .unwrap();

        let argv = build_argv(
            &dir,
            gopher,
            Path::new("/roms/game.z64"),
            &dir,
            "nintendo_64",
        );

        assert_eq!(
            argv,
            vec![
                managed_executable_path(&dir, "gopher64")
                    .to_string_lossy()
                    .to_string(),
                "--fullscreen".to_string(),
                "/roms/game.z64".to_string()
            ],
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// DuckStation uses its own flag style (single-dash `-batch -fullscreen`,
    /// verified against its real command-line parser source) — never
    /// assumed to match gopher64's `--fullscreen`.
    #[test]
    fn duckstation_is_launched_with_its_own_batch_and_fullscreen_flags() {
        let dir = scratch_dir("argv-duckstation");
        let duckstation = native_emulators_for(Platform::PlayStation)
            .into_iter()
            .next()
            .unwrap();

        let argv = build_argv(
            &dir,
            duckstation,
            Path::new("/roms/game.cue"),
            &dir,
            "play_station",
        );

        assert_eq!(
            argv,
            vec![
                managed_executable_path(&dir, "duckstation")
                    .to_string_lossy()
                    .to_string(),
                "-batch".to_string(),
                "-fullscreen".to_string(),
                "/roms/game.cue".to_string(),
            ],
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// FBNeo's Neo Geo CD mode needs a fixed system name *before* the ROM
    /// path (passed via `-cd`), unlike gopher64/DuckStation, which just
    /// take the path last — verified against FBNeo's own real `-help`
    /// usage text and command-line parser source.
    #[test]
    fn fbneo_places_the_rom_path_after_cd_not_trailing() {
        let dir = scratch_dir("argv-fbneo");
        let fbneo = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .next()
            .unwrap();

        let argv = build_argv(&dir, fbneo, Path::new("/roms/game.cue"), &dir, "neo_geo_cd");

        assert_eq!(
            argv,
            vec![
                managed_executable_path(&dir, "fbneo")
                    .to_string_lossy()
                    .to_string(),
                "neocdz".to_string(),
                "-cd".to_string(),
                "/roms/game.cue".to_string(),
                "-fullscreen".to_string(),
            ],
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A managed zipped-binary emulator's executable path points into its
    /// own extracted directory (see `install`'s `ManagedKind::ZippedBinary`
    /// handling) — this is what `is_available` checks for FBNeo.
    #[test]
    fn a_managed_zipped_binary_emulators_executable_lives_in_its_extracted_directory() {
        let dir = scratch_dir("availability-zipped");
        let mut fbneo = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .next()
            .unwrap();
        // Whether `libSDL2_image` happens to already be installed on the
        // machine running this test is real ambient system state — this
        // test is about the zip-extraction path shape, not that (see
        // `is_not_available_when_a_required_system_lib_is_missing` for the
        // required-lib behavior, exercised against a name guaranteed never
        // to exist instead).
        fbneo.required_system_libs = &[];

        let path = managed_executable_path(&dir, "fbneo");
        assert!(path.ends_with("fbneo-files/fbneo"));
        assert!(!is_available(&dir, &fbneo));

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fake fbneo binary").unwrap();
        assert!(is_available(&dir, &fbneo));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `is_available` must check `required_system_libs` in addition to the
    /// executable existing — otherwise Wervik would tell the user FBNeo is
    /// "Installed" when it would actually fail to launch for missing
    /// `libSDL2_image`. Uses a name guaranteed never to be a real shared
    /// library (mirrors `sysutil::tests::a_made_up_library_name_is_not_found`)
    /// rather than depending on whether the real dependency happens to be
    /// installed on whatever machine runs this test.
    #[test]
    fn is_not_available_when_a_required_system_lib_is_missing() {
        let dir = scratch_dir("availability-missing-lib");
        let mut fbneo = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .next()
            .unwrap();
        fbneo.required_system_libs = &["libDefinitelyNotARealWervikTestLibrary"];

        let path = managed_executable_path(&dir, "fbneo");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fake fbneo binary").unwrap();

        assert!(
            !is_available(&dir, &fbneo),
            "must not claim available without its required shared library"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_bios_copies_the_file_to_where_the_emulator_expects_it() {
        let dir = scratch_dir("install-bios");
        let fbneo = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .next()
            .unwrap();
        let executable = managed_executable_path(&dir, "fbneo");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"fake fbneo binary").unwrap();

        let bios_source = dir.join("user-provided-bios.zip");
        std::fs::write(&bios_source, b"fake bios contents").unwrap();

        install_bios(&dir, &fbneo, &bios_source).unwrap();

        let expected_dest = executable.parent().unwrap().join("roms/neocdz.zip");
        assert_eq!(
            std::fs::read(&expected_dest).unwrap(),
            b"fake bios contents"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_bios_is_a_no_op_for_an_emulator_that_does_not_need_one() {
        let dir = scratch_dir("install-bios-noop");
        let gopher = native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .next()
            .unwrap();
        let bios_source = dir.join("irrelevant.bin");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&bios_source, b"irrelevant").unwrap();

        assert!(install_bios(&dir, &gopher, &bios_source).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mirrors `game_setup::regenerated_wrapper_composes_env_vars_gamemode_and_mangohud_in_order`:
    /// gamescope outermost, then gamemode, then mangohud, then the
    /// emulator — proven here by building the real argv against real
    /// on-disk marker files/config, not by asserting against a hand-typed
    /// expectation of what the code "should" do.
    #[test]
    fn composes_gamescope_gamemode_and_mangohud_in_order_around_the_emulator() {
        let dir = scratch_dir("argv-full");
        std::fs::create_dir_all(&dir).unwrap();
        let key = "test-platform";

        mangohud::set_enabled(&dir, key, true).unwrap();
        game_setup::set_gamemode_enabled(&dir, key, true).unwrap();
        game_setup::set_gamescope_enabled(&dir, key, true).unwrap();
        game_setup::write_gamescope_config(
            &dir,
            key,
            &game_setup::GamescopeConfig {
                internal_width: 1280,
                internal_height: 800,
                output_width: 1920,
                output_height: 1200,
                upscaler: "fsr".into(),
            },
        )
        .unwrap();

        let argv = build_argv(
            &dir,
            sample_emulator(),
            Path::new("/roms/game.z64"),
            &dir,
            key,
        );

        assert_eq!(
            argv,
            vec![
                "gamescope",
                "-w",
                "1280",
                "-h",
                "800",
                "-W",
                "1920",
                "-H",
                "1200",
                "-F",
                "fsr",
                "-S",
                "fit",
                "--",
                "gamemoderun",
                "mangohud",
                "fake-emulator",
                "--fullscreen",
                "/roms/game.z64",
            ],
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hits the real GitHub API and downloads the real release binary —
    /// the whole point of `install` is that it talks to a real, public
    /// source, so this is worth verifying for real rather than only
    /// against a mock.
    #[test]
    #[ignore = "requires network access"]
    fn installs_the_real_gopher64_release_binary() {
        let dir = scratch_dir("install-real-gopher64");
        install(&dir, Platform::Nintendo64, "gopher64").unwrap();

        let gopher = native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .next()
            .unwrap();
        assert!(is_available(&dir, &gopher));

        let path = managed_executable_path(&dir, "gopher64");
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(
            size > 10_000_000,
            "expected a real multi-megabyte binary, got {size} bytes"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "installed binary must be executable");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same as the gopher64 install test, but for DuckStation's AppImage
    /// release — proves the whole `--appimage-extract` dance actually
    /// works against the real, current release, not just against a
    /// hand-built fixture.
    #[test]
    #[ignore = "requires network access"]
    fn installs_the_real_duckstation_release_and_extracts_the_appimage() {
        let dir = scratch_dir("install-real-duckstation");
        install(&dir, Platform::PlayStation, "duckstation").unwrap();

        let duckstation = native_emulators_for(Platform::PlayStation)
            .into_iter()
            .next()
            .unwrap();
        assert!(is_available(&dir, &duckstation));

        let apprun = managed_executable_path(&dir, "duckstation");
        assert!(apprun.is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&apprun).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "extracted AppRun must be executable");
        }

        // The downloaded AppImage itself is cleaned up after extraction —
        // only the extracted contents should remain.
        assert!(!dir
            .join("emulators/duckstation-appimage/download.AppImage")
            .exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same idea again, but for FBNeo's zipped release — proves the whole
    /// zip-extraction dance works against the real, current release.
    /// Checks the extracted file directly rather than through
    /// `is_available`, since that also requires `libSDL2_image` to be
    /// installed on whatever machine runs this test, which this one
    /// doesn't assert either way.
    #[test]
    #[ignore = "requires network access"]
    fn installs_the_real_fbneo_release_and_extracts_the_zip() {
        let dir = scratch_dir("install-real-fbneo");
        install(&dir, Platform::NeoGeoCd, "fbneo").unwrap();

        let path = managed_executable_path(&dir, "fbneo");
        assert!(path.is_file());
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(
            size > 10_000_000,
            "expected a real multi-megabyte binary, got {size} bytes"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o111,
                0o111,
                "extracted fbneo binary must be executable"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_an_unknown_emulator_id_is_an_error() {
        let dir = scratch_dir("install-unknown");
        let err = install(&dir, Platform::Nintendo64, "not-a-real-emulator").unwrap_err();
        assert!(err.contains("not-a-real-emulator"));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn exposes_retroarch_cores_for_arcade_and_neo_geo_cd() {
        let n64 = native_emulators_for(Platform::Nintendo64);
        assert_eq!(n64.len(), 1);
        assert_eq!(n64[0].id, "retroarch-mupen64plus-next");
        assert!(n64[0].recommended);

        let arcade = native_emulators_for(Platform::Arcade);
        assert_eq!(arcade.len(), 1);
        assert_eq!(arcade[0].id, "retroarch-fbneo");
        assert!(arcade[0].recommended);

        let neo_geo_cd = native_emulators_for(Platform::NeoGeoCd);
        assert_eq!(neo_geo_cd.len(), 3);
        assert_eq!(neo_geo_cd[0].id, "retroarch-neocd");
        assert_eq!(neo_geo_cd[1].id, "retroarch-geolith");
        assert_eq!(neo_geo_cd[2].id, "retroarch-fbneo");
        assert!(neo_geo_cd[0].recommended);
    }

    #[test]
    fn builds_the_retroarch_core_path_and_cli_arguments() {
        let app_data_dir = Path::new(r"C:\Users\tester\AppData\Roaming\Wervik");
        let emulator = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .find(|emulator| emulator.id == "retroarch-neocd")
            .unwrap();
        let core = managed_core_path(app_data_dir, emulator.id).unwrap();
        assert!(core.ends_with(r"emulators\retroarch\cores\neocd_libretro.dll"));

        let argv = build_argv(
            app_data_dir,
            emulator,
            Path::new(r"D:\Games\Metal Slug.cue"),
            app_data_dir,
            "neo_geo_cd",
        );
        assert_eq!(
            argv[0],
            r"C:\Users\tester\AppData\Roaming\Wervik\emulators\retroarch\retroarch.exe"
        );
        assert_eq!(argv[1], "-L");
        assert_eq!(argv[2], core.to_string_lossy());
        assert_eq!(argv[3], r"D:\Games\Metal Slug.cue");
    }

    #[test]
    fn builds_the_mupen64plus_next_core_path_and_cli_arguments() {
        let app_data_dir = Path::new(r"C:\Users\tester\AppData\Roaming\Wervik");
        let emulator = native_emulators_for(Platform::Nintendo64)
            .into_iter()
            .next()
            .unwrap();
        let core = managed_core_path(app_data_dir, emulator.id).unwrap();
        assert!(core.ends_with(r"emulators\retroarch\cores\mupen64plus_next_libretro.dll"));

        let argv = build_argv(
            app_data_dir,
            emulator,
            Path::new(r"D:\Games\Super Mario 64.z64"),
            app_data_dir,
            "nintendo_64",
        );
        assert_eq!(argv[1], "-L");
        assert_eq!(argv[2], core.to_string_lossy());
        assert_eq!(argv[3], r"D:\Games\Super Mario 64.z64");
    }

    #[test]
    fn builds_the_pcsx2_executable_path_and_cli_arguments() {
        let app_data_dir = Path::new(r"C:\Users\tester\AppData\Roaming\Wervik");
        let emulator = native_emulators_for(Platform::PlayStation2)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(emulator.id, "pcsx2");
        assert!(emulator.recommended);

        let executable = managed_executable_path(app_data_dir, emulator.id);
        assert!(executable.ends_with(r"emulators\pcsx2-files\pcsx2-qt.exe"));

        let argv = build_argv(
            app_data_dir,
            emulator,
            Path::new(r"D:\Games\Some Game.iso"),
            app_data_dir,
            "play_station_2",
        );
        assert_eq!(argv[0], executable.to_string_lossy());
        assert_eq!(&argv[1..], &["-portable", "-batch", "-fullscreen", r"D:\Games\Some Game.iso"]);
    }

    #[test]
    fn finds_retroarch_executable_inside_the_portable_package_directory() {
        let root =
            std::env::temp_dir().join(format!("wervik-retroarch-layout-{}", std::process::id()));
        let executable = managed_dir(&root)
            .join("retroarch")
            .join("RetroArch-Win64")
            .join("retroarch.exe");
        let parent = executable.parent().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(&executable, b"fake executable").unwrap();

        assert_eq!(
            managed_executable_path(&root, "retroarch-fbneo"),
            executable
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalizes_the_common_geolith_bios_member_name() {
        let root = std::env::temp_dir().join(format!("wervik-geolith-bios-{}", std::process::id()));
        let executable = managed_executable_path(&root, "retroarch-geolith");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"fake executable").unwrap();

        let source = root.join("neocdz.zip");
        {
            let file = std::fs::File::create(&source).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            writer
                .start_file("neocdz.bin", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, b"valid bios").unwrap();
            writer
                .start_file("000-lo.lo", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, b"logo rom").unwrap();
            writer.finish().unwrap();
        }

        let geolith = native_emulators_for(Platform::NeoGeoCd)
            .into_iter()
            .find(|emulator| emulator.id == "retroarch-geolith")
            .unwrap();
        install_bios(&root, &geolith, &source).unwrap();

        let destination = executable.parent().unwrap().join("system/neocdz.zip");
        let mut archive = zip::ZipArchive::new(std::fs::File::open(destination).unwrap()).unwrap();
        assert_eq!(archive.by_name("neocd.bin").unwrap().size(), 10);
        assert_eq!(archive.by_name("000-lo.lo").unwrap().size(), 8);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_a_retroarch_core_from_its_zip() {
        let root =
            std::env::temp_dir().join(format!("wervik-retroarch-core-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let archive_path = root.join("fbneo.zip");
        std::fs::create_dir_all(&root).unwrap();
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("fbneo_libretro.dll", options).unwrap();
            std::io::Write::write_all(&mut writer, b"fake core").unwrap();
            writer.finish().unwrap();
        }

        let bytes = std::fs::read(&archive_path).unwrap();
        let cores_dir = root.join("cores");
        install_retroarch_core(bytes, &cores_dir, "fbneo").unwrap();
        assert_eq!(
            std::fs::read(cores_dir.join("fbneo_libretro.dll")).unwrap(),
            b"fake core"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}

// Only meaningful on Windows itself: the Linux test module above exercises
// the `not(target_os = "windows")` variant of `native_emulators_for`, which
// is a different function entirely (see the `#[cfg]` on each).
#[cfg(all(test, target_os = "windows"))]
mod windows_release_tests {
    use super::*;

    #[test]
    fn every_managed_native_has_a_release_source() {
        for platform in Platform::ALL {
            for emulator in native_emulators_for(platform) {
                if emulator.is_managed() {
                    assert!(
                        managed_release_for(emulator.id).is_some(),
                        "{} ({platform:?}) has no managed_release_for entry",
                        emulator.id,
                    );
                }
            }
        }
    }

    /// Regression test: `ensure_retroarch_defaults` used to run for any
    /// `Launcher::Managed` emulator, which was safe only because every
    /// managed Windows emulator used to be RetroArch-based — PCSX2 broke
    /// that assumption, and calling it for PCSX2 would spuriously create an
    /// empty `emulators/retroarch/` directory that was never installed.
    #[test]
    fn only_retroarch_based_emulators_trigger_retroarch_defaults() {
        assert!(is_retroarch_based("retroarch-neocd"));
        assert!(is_retroarch_based("retroarch-mupen64plus-next"));
        assert!(!is_retroarch_based("pcsx2"));
    }

    #[test]
    fn every_platform_has_at_least_one_native_option() {
        for platform in Platform::ALL {
            assert!(
                !native_emulators_for(platform).is_empty(),
                "{platform:?} has no native emulator option at all"
            );
        }
    }
}
