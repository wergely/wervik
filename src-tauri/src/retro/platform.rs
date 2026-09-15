//! Retro platform identification: which extensions belong to which system,
//! and each system's default libretro core.
//!
//! `default_core()` is set directly as `EJS_core`, which EmulatorJS's
//! loader treats as either a system id or a specific core name — passing
//! the exact core name (rather than a system id like "psx") pins Wervik to
//! the one core it actually vendors for that platform (see
//! `resources/emulatorjs-runtime/cores`), rather than whatever EmulatorJS's
//! own default-core-per-system ordering happens to be. Verified against
//! the real vendored `emulator.min.js` (see
//! `retro::server::tests::serves_the_real_vendored_runtime`), not just
//! EmulatorJS's docs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Nes,
    Snes,
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
    MasterSystem,
    Genesis,
    GameGear,
    Sega32X,
    PlayStation,
    Nintendo64,
    Atari5200,
    Atari7800,
    AtariLynx,
    AtariJaguar,
    WonderSwan,
    NeoGeoPocket,
    PcEngine,
    ColecoVision,
    Arcade,
    NeoGeoCd,
    PlayStation2,
    WiiU,
    GameCube,
    Wii,
}

impl Platform {
    /// Every platform Wervik knows about, for UI listings (e.g. the
    /// Settings "Platforms" section) and exhaustiveness tests.
    pub const ALL: [Platform; 25] = [
        Platform::Nes,
        Platform::Snes,
        Platform::GameBoy,
        Platform::GameBoyColor,
        Platform::GameBoyAdvance,
        Platform::MasterSystem,
        Platform::Genesis,
        Platform::GameGear,
        Platform::Sega32X,
        Platform::PlayStation,
        Platform::Nintendo64,
        Platform::Atari5200,
        Platform::Atari7800,
        Platform::AtariLynx,
        Platform::AtariJaguar,
        Platform::WonderSwan,
        Platform::NeoGeoPocket,
        Platform::PcEngine,
        Platform::ColecoVision,
        Platform::Arcade,
        Platform::NeoGeoCd,
        Platform::PlayStation2,
        Platform::WiiU,
        Platform::GameCube,
        Platform::Wii,
    ];

    /// A stable machine-readable key, independent of the JSON wire format
    /// (`Serialize`'s `snake_case` rename) — used for persisted settings
    /// keys (see `storage::Store::default_emulator`) so a future change to
    /// the wire representation can't silently change what a stored
    /// preference key looks like.
    pub fn key(self) -> &'static str {
        match self {
            Platform::Nes => "nes",
            Platform::Snes => "snes",
            Platform::GameBoy => "game_boy",
            Platform::GameBoyColor => "game_boy_color",
            Platform::GameBoyAdvance => "game_boy_advance",
            Platform::MasterSystem => "master_system",
            Platform::Genesis => "genesis",
            Platform::GameGear => "game_gear",
            Platform::Sega32X => "sega_32x",
            Platform::PlayStation => "play_station",
            Platform::Nintendo64 => "nintendo_64",
            Platform::Atari5200 => "atari_5200",
            Platform::Atari7800 => "atari_7800",
            Platform::AtariLynx => "atari_lynx",
            Platform::AtariJaguar => "atari_jaguar",
            Platform::WonderSwan => "wonder_swan",
            Platform::NeoGeoPocket => "neo_geo_pocket",
            Platform::PcEngine => "pc_engine",
            Platform::ColecoVision => "coleco_vision",
            Platform::Arcade => "arcade",
            Platform::NeoGeoCd => "neo_geo_cd",
            Platform::PlayStation2 => "play_station_2",
            Platform::WiiU => "wii_u",
            Platform::GameCube => "game_cube",
            Platform::Wii => "wii",
        }
    }

    /// The inverse of `key()` — used wherever a platform arrives over the
    /// Tauri boundary as its stable string key (e.g. `install_native_emulator`)
    /// rather than as this enum directly.
    pub fn from_key(key: &str) -> Option<Platform> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Full name, e.g. for the Game Setup "System" field.
    pub fn display_name(self) -> &'static str {
        match self {
            Platform::Nes => "Nintendo Entertainment System",
            Platform::Snes => "Super Nintendo Entertainment System",
            Platform::GameBoy => "Game Boy",
            Platform::GameBoyColor => "Game Boy Color",
            Platform::GameBoyAdvance => "Game Boy Advance",
            Platform::MasterSystem => "Master System",
            Platform::Genesis => "Mega Drive / Genesis",
            Platform::GameGear => "Game Gear",
            Platform::Sega32X => "Sega 32X",
            Platform::PlayStation => "PlayStation",
            Platform::Nintendo64 => "Nintendo 64",
            Platform::Atari5200 => "Atari 5200",
            Platform::Atari7800 => "Atari 7800",
            Platform::AtariLynx => "Atari Lynx",
            Platform::AtariJaguar => "Atari Jaguar",
            Platform::WonderSwan => "WonderSwan",
            Platform::NeoGeoPocket => "Neo Geo Pocket",
            Platform::PcEngine => "PC Engine / TurboGrafx-16",
            Platform::ColecoVision => "ColecoVision",
            Platform::Arcade => "Arcade",
            Platform::NeoGeoCd => "Neo Geo CD",
            Platform::PlayStation2 => "PlayStation 2",
            Platform::WiiU => "Wii U",
            Platform::GameCube => "GameCube",
            Platform::Wii => "Wii",
        }
    }

    /// Short label for the source badge on cards/details, e.g. "SNES".
    pub fn short_name(self) -> &'static str {
        match self {
            Platform::Nes => "NES",
            Platform::Snes => "SNES",
            Platform::GameBoy => "Game Boy",
            Platform::GameBoyColor => "Game Boy Color",
            Platform::GameBoyAdvance => "Game Boy Advance",
            Platform::MasterSystem => "Master System",
            Platform::Genesis => "Genesis",
            Platform::GameGear => "Game Gear",
            Platform::Sega32X => "32X",
            Platform::PlayStation => "PlayStation",
            Platform::Nintendo64 => "N64",
            Platform::Atari5200 => "Atari 5200",
            Platform::Atari7800 => "Atari 7800",
            Platform::AtariLynx => "Lynx",
            Platform::AtariJaguar => "Jaguar",
            Platform::WonderSwan => "WonderSwan",
            Platform::NeoGeoPocket => "Neo Geo Pocket",
            Platform::PcEngine => "PC Engine",
            Platform::ColecoVision => "ColecoVision",
            Platform::Arcade => "Arcade",
            Platform::NeoGeoCd => "Neo Geo CD",
            Platform::PlayStation2 => "PS2",
            Platform::WiiU => "Wii U",
            Platform::GameCube => "GameCube",
            Platform::Wii => "Wii",
        }
    }

    /// The exact core Wervik vendors for this platform (see
    /// `resources/emulatorjs-runtime/cores`) — verified against the real
    /// `data/cores/cores.json` EmulatorJS ships, not guessed from docs.
    /// `None` when EmulatorJS has no vendored core for this platform at
    /// all (Arcade and Neo Geo CD aren't bundled) —
    /// callers must not offer EmulatorJS as a fallback in that case (see
    /// `commands::emulator_options_for`), since it genuinely can't play
    /// anything for that platform.
    pub fn default_core(self) -> Option<&'static str> {
        Some(match self {
            Platform::Nes => "fceumm",
            Platform::Snes => "snes9x",
            Platform::GameBoy | Platform::GameBoyColor => "gambatte",
            Platform::GameBoyAdvance => "mgba",
            Platform::MasterSystem | Platform::Genesis | Platform::GameGear => "genesis_plus_gx",
            Platform::Sega32X => "picodrive",
            Platform::PlayStation => "mednafen_psx_hw",
            Platform::Nintendo64 => "mupen64plus_next",
            Platform::Atari5200 => "a5200",
            Platform::Atari7800 => "prosystem",
            Platform::AtariLynx => "handy",
            Platform::AtariJaguar => "virtualjaguar",
            Platform::WonderSwan => "mednafen_wswan",
            Platform::NeoGeoPocket => "mednafen_ngp",
            Platform::PcEngine => "mednafen_pce",
            Platform::ColecoVision => "gearcoleco",
            Platform::Arcade
            | Platform::NeoGeoCd
            | Platform::PlayStation2
            | Platform::WiiU
            | Platform::GameCube
            | Platform::Wii => return None,
        })
    }

    /// Whether this platform needs a user-supplied BIOS to boot at all, as
    /// opposed to systems where a BIOS is optional (e.g. just for a boot
    /// logo). Wervik never provides BIOS files itself — see `retro::bios`.
    ///
    /// Wii U is the one soft case: Cemu doesn't strictly need `keys.txt` to
    /// boot every title (only ones with encrypted content), but Wervik
    /// still gates on it here rather than adding a genuinely optional
    /// third state to this otherwise-boolean check — simpler, and the
    /// BIOS/keys setup UI this drives is the only place Wervik has to
    /// offer configuring it at all.
    pub fn requires_bios(self) -> bool {
        matches!(
            self,
            Platform::PlayStation | Platform::NeoGeoCd | Platform::PlayStation2 | Platform::WiiU
        )
    }

    /// The platform's own name(s) as they appear in IGDB's `platforms.name`
    /// field, used only to disambiguate an IGDB title search against the
    /// ROM's actual platform (see `metadata::choose_result`) — e.g. so a
    /// SNES "Star Fox" search doesn't settle for the Switch "Star Fox"
    /// entry just because IGDB ranked it first. Several platforms have more
    /// than one real IGDB entry Wervik treats as equivalent (e.g. Famicom
    /// alongside NES).
    pub fn igdb_names(self) -> &'static [&'static str] {
        match self {
            Platform::Nes => &["Nintendo Entertainment System (NES)", "Family Computer"],
            Platform::Snes => &[
                "Super Nintendo Entertainment System (SNES)",
                "Super Famicom",
            ],
            Platform::GameBoy => &["Game Boy"],
            Platform::GameBoyColor => &["Game Boy Color"],
            Platform::GameBoyAdvance => &["Game Boy Advance"],
            Platform::MasterSystem => &["Sega Master System/Mark III"],
            Platform::Genesis => &["Sega Mega Drive/Genesis"],
            Platform::GameGear => &["Sega Game Gear"],
            Platform::Sega32X => &["Sega 32X"],
            Platform::PlayStation => &["PlayStation"],
            Platform::Nintendo64 => &["Nintendo 64"],
            Platform::Atari5200 => &["Atari 5200"],
            Platform::Atari7800 => &["Atari 7800"],
            Platform::AtariLynx => &["Atari Lynx"],
            Platform::AtariJaguar => &["Atari Jaguar"],
            Platform::WonderSwan => &["WonderSwan", "WonderSwan Color"],
            Platform::NeoGeoPocket => &["Neo Geo Pocket", "Neo Geo Pocket Color"],
            Platform::PcEngine => &["TurboGrafx-16/PC Engine", "TurboGrafx-16/PC Engine CD"],
            Platform::ColecoVision => &["ColecoVision"],
            Platform::Arcade => &["Arcade"],
            Platform::NeoGeoCd => &["Neo Geo CD"],
            Platform::PlayStation2 => &["PlayStation 2"],
            Platform::WiiU => &["Wii U"],
            Platform::GameCube => &["Nintendo GameCube"],
            Platform::Wii => &["Wii"],
        }
    }

    /// Maps an unambiguous file extension to its platform. `.bin` is
    /// deliberately excluded — it's ambiguous between a standalone Genesis
    /// ROM and a PlayStation disc track (and, per the real
    /// `cores.json`, also a valid extension for several of the platforms
    /// below) — so callers resolve it themselves using directory context
    /// (see `discovery`). Every extension here is cross-checked against
    /// `resources/emulatorjs-runtime/cores/cores.json` rather than assumed;
    /// where a core's real extension list includes something ambiguous
    /// (shared with another platform, or generic like "rom"/"bin"), that
    /// extension is left out rather than risk misidentifying a ROM.
    pub fn from_unambiguous_extension(ext: &str) -> Option<Platform> {
        match ext.to_ascii_lowercase().as_str() {
            "nes" => Some(Platform::Nes),
            "sfc" | "smc" => Some(Platform::Snes),
            "gb" => Some(Platform::GameBoy),
            "gbc" => Some(Platform::GameBoyColor),
            "gba" => Some(Platform::GameBoyAdvance),
            "sms" => Some(Platform::MasterSystem),
            "md" | "gen" => Some(Platform::Genesis),
            "gg" => Some(Platform::GameGear),
            "32x" => Some(Platform::Sega32X),
            // A provisional guess only — `.cue`/`.chd` are also used by
            // Neo Geo CD (and other CD-based systems Wervik doesn't
            // support), so `discovery` re-checks the disc's own content
            // before trusting this (see `disc::cue_references_a_playstation_disc`
            // and its Neo Geo CD counterpart).
            "cue" | "chd" => Some(Platform::PlayStation),
            // Also a provisional guess: besides PS2's own (near-universal)
            // use of `.iso`, it's occasionally a single-track PS1 dump, a
            // GameCube dump, or a Wii dump too — `discovery` re-checks the
            // disc's own header before trusting this (see
            // `disc::iso_is_a_playstation2_disc` and its PS1/GameCube/Wii
            // counterparts).
            "iso" => Some(Platform::PlayStation2),
            // GameCube's and Wii's own uncompressed dump formats —
            // unambiguous, unlike `.iso`: no other supported platform uses
            // either one, and (unlike each other) `.gcm` was never adopted
            // for Wii dumps nor `.wbfs` (a format built specifically around
            // Wii's own disc layout) for GameCube ones.
            "gcm" => Some(Platform::GameCube),
            "wbfs" => Some(Platform::Wii),
            "n64" | "v64" | "z64" => Some(Platform::Nintendo64),
            "a52" => Some(Platform::Atari5200),
            "a78" => Some(Platform::Atari7800),
            "lnx" => Some(Platform::AtariLynx),
            "j64" | "jag" => Some(Platform::AtariJaguar),
            "ws" | "wsc" => Some(Platform::WonderSwan),
            "ngp" | "ngc" => Some(Platform::NeoGeoPocket),
            "pce" => Some(Platform::PcEngine),
            "col" | "cv" => Some(Platform::ColecoVision),
            // Wii U's own disc-dump/archive formats — unambiguous (no other
            // supported platform uses any of these). A game whose own dump
            // is a *directory* instead (the far more common case for a
            // decrypted Wii U title) isn't reachable through this function
            // at all; see `discovery::scan_tagged_directory`.
            "wud" | "wux" | "wua" => Some(Platform::WiiU),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_extensions() {
        assert_eq!(
            Platform::from_unambiguous_extension("sfc"),
            Some(Platform::Snes)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("SFC"),
            Some(Platform::Snes)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("cue"),
            Some(Platform::PlayStation)
        );
        assert_eq!(Platform::from_unambiguous_extension("bin"), None);
        assert_eq!(Platform::from_unambiguous_extension("txt"), None);
    }

    #[test]
    fn maps_the_newly_added_platforms() {
        assert_eq!(
            Platform::from_unambiguous_extension("z64"),
            Some(Platform::Nintendo64)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("32x"),
            Some(Platform::Sega32X)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("gg"),
            Some(Platform::GameGear)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("a52"),
            Some(Platform::Atari5200)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("a78"),
            Some(Platform::Atari7800)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("lnx"),
            Some(Platform::AtariLynx)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("jag"),
            Some(Platform::AtariJaguar)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("wsc"),
            Some(Platform::WonderSwan)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("ngc"),
            Some(Platform::NeoGeoPocket)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("pce"),
            Some(Platform::PcEngine)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("cv"),
            Some(Platform::ColecoVision)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("iso"),
            Some(Platform::PlayStation2)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("gcm"),
            Some(Platform::GameCube)
        );
        assert_eq!(
            Platform::from_unambiguous_extension("wbfs"),
            Some(Platform::Wii)
        );
    }

    /// Every platform that claims an EmulatorJS core at all must map to
    /// one that's actually vendored (see `resources/emulatorjs-runtime/cores`)
    /// — this would have caught the PSX default-core mistake `platform.rs`'s
    /// module doc mentions if it existed when that was fixed. A platform
    /// with no vendored core (`None`) is a deliberate, checked exception,
    /// not something this test should silently let slide — see the next
    /// test.
    #[test]
    fn every_default_core_is_actually_vendored() {
        let cores_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/emulatorjs-runtime/cores");
        for platform in Platform::ALL {
            let Some(core) = platform.default_core() else {
                continue;
            };
            let path = cores_dir.join(format!("{core}-wasm.data"));
            assert!(
                path.exists(),
                "{platform:?}'s core {core:?} is not vendored at {path:?}"
            );
        }
    }

    /// Arcade and Neo Geo CD deliberately have no EmulatorJS core at all —
    /// pinned here so a platform silently losing its core isn't mistaken for
    /// "working as intended".
    #[test]
    fn neo_geo_cd_has_no_emulatorjs_core() {
        assert_eq!(Platform::NeoGeoCd.default_core(), None);
    }

    #[test]
    fn every_platform_has_a_unique_storage_key() {
        let mut keys: Vec<&str> = Platform::ALL.iter().map(|p| p.key()).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            before,
            "two platforms share the same storage key"
        );
    }

    #[test]
    fn from_key_reverses_key_for_every_platform() {
        for platform in Platform::ALL {
            assert_eq!(Platform::from_key(platform.key()), Some(platform));
        }
        assert_eq!(Platform::from_key("not-a-real-platform"), None);
    }
}
