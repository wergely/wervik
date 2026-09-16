//! Small local JSON store for Wervik's own state: favorites, settings, and ROM
//! library configuration. Playtime sessions and totals live in the separate
//! SQLite store from `playtime.rs`; this file keeps the old retro fields for
//! migration and compatibility. It never touches Steam's own configuration.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WervikAccount {
    pub name: String,
    pub email: String,
    pub connected_at: i64,
    pub installation_id: String,
    /// Opaque bearer credential issued during desktop account handoff. Older
    /// local accounts may not have one and must reconnect before syncing.
    #[serde(default)]
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingWervikConnect {
    pub state: String,
    pub code_verifier: String,
    pub redirect_scheme: String,
    pub installation_id: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    /// The Library grid's own sort/filter selections (see
    /// `LibrarySection` on the frontend) — persisted here, alongside the
    /// rest of Wervik's settings, so closing and reopening the app (or
    /// just navigating away and back to Home) doesn't reset them.
    #[serde(default = "default_library_sort_mode")]
    pub library_sort_mode: String,
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_source_filter: Vec<String>,
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_platform_filter: Vec<String>,
    #[serde(default)]
    pub library_favorites_only: bool,
    /// Advanced filters for the locally cached IGDB metadata attached to
    /// library games. Empty lists and `None` mean no restriction.
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_igdb_genre_filter: Vec<String>,
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_igdb_platform_filter: Vec<String>,
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_igdb_developer_filter: Vec<String>,
    #[serde(
        default = "default_all_filters",
        deserialize_with = "deserialize_filter_list"
    )]
    pub library_igdb_publisher_filter: Vec<String>,
    #[serde(default)]
    pub library_igdb_min_score: Option<u8>,
    #[serde(default)]
    pub library_igdb_year_from: Option<u16>,
    #[serde(default)]
    pub library_igdb_year_to: Option<u16>,
    #[serde(default)]
    pub library_igdb_metadata_only: bool,
    /// Background chiptune player heard while browsing Home (see
    /// `App.tsx`'s `useChiptunePlayer`) — generated locally, no audio
    /// files or network involved.
    #[serde(default = "default_music_enabled")]
    pub music_enabled: bool,
    #[serde(default = "default_music_volume")]
    pub music_volume: f32,
    /// IGDB credentials entered in Settings, as an alternative to the
    /// `WERVIK_IGDB_CLIENT_ID`/`WERVIK_IGDB_CLIENT_SECRET` environment
    /// variables (see `metadata::credentials`, which prefers these when
    /// both are set). Stored in this same local, unsynced file — never
    /// sent anywhere except to IGDB's own token endpoint.
    #[serde(default)]
    pub igdb_client_id: String,
    #[serde(default)]
    pub igdb_client_secret: String,
    /// RetroAchievements username and Web API key (from the site's own
    /// "Keys" control panel page) for the read-only achievements viewer —
    /// see `retroachievements`. Unrelated to the account password used to
    /// enable RetroArch's own live achievements (never persisted here).
    #[serde(default)]
    pub ra_username: String,
    #[serde(default)]
    pub ra_api_key: String,
    /// Steam Web API key (from <https://steamcommunity.com/dev/apikey>)
    /// for the same read-only achievements viewer, but for Steam games —
    /// see `steam::achievements`. The account's SteamID64 is detected
    /// automatically from the local Steam install and never stored here;
    /// `steam_id64_override` is only a manual escape hatch for a machine
    /// with more than one local account, if auto-detection ever picks the
    /// wrong one.
    #[serde(default)]
    pub steam_api_key: String,
    #[serde(default)]
    pub steam_id64_override: String,
}

fn default_ui_scale() -> f32 {
    1.0
}

// Playtime is the most useful default ordering — "what do I actually
// play" — rather than an alphabetical list that says nothing about that.
fn default_library_sort_mode() -> String {
    "playtime".to_string()
}

fn default_all_filters() -> Vec<String> {
    Vec::new()
}

fn default_music_enabled() -> bool {
    true
}

fn default_music_volume() -> f32 {
    0.35
}

/// Accepts both the old single-value filter (`"retro"`) and the new array
/// representation (`["steam", "retro"]`) while users' saved state migrates
/// naturally the next time settings are persisted.
fn deserialize_filter_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(value) => {
            if value == "all" || value.trim().is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![value])
            }
        }
        serde_json::Value::Array(values) => Ok(values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .filter(|value| value != "all" && !value.trim().is_empty())
            .collect()),
        _ => Err(serde::de::Error::custom("expected a filter string or list")),
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fullscreen: false,
            launch_on_startup: false,
            ui_scale: default_ui_scale(),
            library_sort_mode: default_library_sort_mode(),
            library_source_filter: default_all_filters(),
            library_platform_filter: default_all_filters(),
            library_favorites_only: false,
            library_igdb_genre_filter: default_all_filters(),
            library_igdb_platform_filter: default_all_filters(),
            library_igdb_developer_filter: default_all_filters(),
            library_igdb_publisher_filter: default_all_filters(),
            library_igdb_min_score: None,
            library_igdb_year_from: None,
            library_igdb_year_to: None,
            library_igdb_metadata_only: false,
            music_enabled: default_music_enabled(),
            music_volume: default_music_volume(),
            igdb_client_id: String::new(),
            igdb_client_secret: String::new(),
            ra_username: String::new(),
            ra_api_key: String::new(),
            steam_api_key: String::new(),
            steam_id64_override: String::new(),
        }
    }
}

/// A configured ROM folder, with an optional explicit platform override.
///
/// Left as `None` (the default — every directory added before this field
/// existed migrates to this), a directory keeps today's behavior: `walk`
/// figures out each file's platform itself from its extension/content (see
/// `retro::discovery`, `retro::disc`). Set, Wervik trusts it completely
/// instead — no extension or content check at all, and (see
/// `retro::discovery::scan_tagged_directory`) each immediate entry under
/// the directory becomes one game whether it's a file *or a folder*, which
/// is what actually lets Wervik support a platform whose own game dumps
/// aren't single files (e.g. a decrypted Wii U title's
/// `code`/`content`/`meta` layout) without teaching `disc.rs` a new
/// content-sniffing check for every such case.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RomDirectoryConfig {
    pub path: String,
    #[serde(default)]
    pub platform_key: Option<String>,
}

/// Old saves have `romDirectories` as a plain list of path strings; this
/// accepts either that or the new `{path, platformKey}` shape so opening
/// an existing library never loses configured ROM folders.
fn deserialize_rom_directories<'de, D>(deserializer: D) -> Result<Vec<RomDirectoryConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry {
        Legacy(String),
        Config(RomDirectoryConfig),
    }
    let entries = Vec::<Entry>::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            Entry::Legacy(path) => RomDirectoryConfig {
                path,
                platform_key: None,
            },
            Entry::Config(config) => config,
        })
        .collect())
}

/// Legacy accumulated playtime for a retro game. New sessions are written to
/// SQLite, but this representation remains readable for migration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetroPlaytime {
    pub total_minutes: u64,
    pub last_played: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    /// Stable opaque identifier for this local Wervik installation. It is
    /// created once and sent only as part of the desktop account handoff.
    #[serde(default)]
    pub installation_id: String,
    #[serde(default)]
    pub wervik_account: Option<WervikAccount>,
    #[serde(default)]
    pub pending_wervik_connect: Option<PendingWervikConnect>,
    /// Keyed by the unified game id (Steam app ids as strings, or a local
    /// ROM's stable id) — favorites apply across both kinds of game.
    #[serde(default)]
    pub favorite_game_ids: HashSet<String>,
    /// Local like/dislike preference keyed by the unified game id.
    #[serde(default)]
    pub game_reactions: HashMap<String, String>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default, deserialize_with = "deserialize_rom_directories")]
    pub rom_directories: Vec<RomDirectoryConfig>,
    #[serde(default)]
    pub retro_playtime: HashMap<String, RetroPlaytime>,
    /// Legacy last launch time for sources that do not expose their own
    /// playtime history (Xbox App / PC Game Pass and Battle.net).
    #[serde(default)]
    pub local_last_played: HashMap<String, i64>,
    /// Legacy game id -> path map from the first BIOS implementation. It is
    /// kept so old save files deserialize and is drained into
    /// `platform_bios_paths` once the ROM cache tells us each game's
    /// platform. Never populated by Wervik itself downloading anything.
    #[serde(default)]
    pub bios_paths: HashMap<String, String>,
    /// Platform key -> path of the user-supplied BIOS file Wervik copied into
    /// its own app data directory (see `retro::bios`). A platform has one
    /// shared BIOS configuration, so every ROM on that platform sees the
    /// same file.
    #[serde(default)]
    pub platform_bios_paths: HashMap<String, String>,
    /// Platform key -> the user's explicit "Default Emulator" choice for
    /// that platform: either `"emulatorjs"` (the built-in runtime) or a
    /// native emulator's id (see `retro::native::NativeEmulator::id`).
    /// Absent means no explicit choice has been made yet — Wervik picks
    /// the best available native emulator, falling back to EmulatorJS
    /// (see `commands::selected_emulator_id`).
    #[serde(default)]
    pub platform_default_emulator: HashMap<String, String>,
    /// Superseded by `platform_default_emulator` above (see `Store::load`'s
    /// one-time migration) — kept only so a save file from before that
    /// change still deserializes; always empty again immediately after.
    #[serde(default)]
    platform_prefer_native: HashMap<String, bool>,
    #[serde(default)]
    platform_native_emulator: HashMap<String, String>,
    /// Platform key -> `"original"` or `"optimized"` graphics preset (see
    /// `retro::native::apply_graphics_preset`). Absent means `"original"` —
    /// Wervik never applies a resolution bump the user didn't ask for.
    #[serde(default)]
    pub platform_graphics_preset: HashMap<String, String>,
}

pub struct Store {
    path: PathBuf,
    state: StoredState,
}

impl Store {
    pub fn load(app_data_dir: &Path) -> Self {
        let path = app_data_dir.join("wervik-state.json");
        let mut state: StoredState = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        // One-time migration from the old two-field "prefer native, and
        // which one" model to a single explicit per-platform "Default
        // Emulator" choice (see the Platform Setup UX refinement). Without
        // this, a real already-made choice — e.g. picking gopher64 over
        // mupen64plus for N64 — would silently revert to whatever the
        // fallback logic picks first the next time Wervik starts.
        let mut migrated = false;
        for (platform_key, emulator_id) in state.platform_native_emulator.drain() {
            state
                .platform_default_emulator
                .entry(platform_key)
                .or_insert(emulator_id);
            migrated = true;
        }
        for (platform_key, prefer) in state.platform_prefer_native.drain() {
            if !prefer {
                state
                    .platform_default_emulator
                    .insert(platform_key, "emulatorjs".to_string());
                migrated = true;
            }
        }

        if state.installation_id.is_empty() {
            state.installation_id = format!("{:032x}", rand::random::<u128>());
            migrated = true;
        }

        let store = Self { path, state };
        if migrated {
            store.persist();
        }
        store
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.state) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    pub fn is_favorite(&self, game_id: &str) -> bool {
        self.state.favorite_game_ids.contains(game_id)
    }

    pub fn set_favorite(&mut self, game_id: &str, favorite: bool) {
        if favorite {
            self.state.favorite_game_ids.insert(game_id.to_string());
        } else {
            self.state.favorite_game_ids.remove(game_id);
        }
        self.persist();
    }

    pub fn game_reaction(&self, game_id: &str) -> Option<String> {
        self.state.game_reactions.get(game_id).cloned()
    }

    pub fn set_game_reaction(&mut self, game_id: &str, reaction: Option<String>) {
        match reaction {
            Some(reaction) => {
                self.state
                    .game_reactions
                    .insert(game_id.to_string(), reaction);
            }
            None => {
                self.state.game_reactions.remove(game_id);
            }
        }
        self.persist();
    }

    pub fn settings(&self) -> Settings {
        self.state.settings.clone()
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.state.settings = settings;
        self.persist();
    }

    pub fn installation_id(&self) -> String {
        self.state.installation_id.clone()
    }

    pub fn wervik_account(&self) -> Option<WervikAccount> {
        self.state.wervik_account.clone()
    }

    pub fn set_wervik_account(&mut self, account: WervikAccount) {
        self.state.wervik_account = Some(account);
        self.state.pending_wervik_connect = None;
        self.persist();
    }

    pub fn clear_wervik_account(&mut self) {
        self.state.wervik_account = None;
        self.state.pending_wervik_connect = None;
        self.persist();
    }

    pub fn pending_wervik_connect(&self) -> Option<PendingWervikConnect> {
        self.state.pending_wervik_connect.clone()
    }

    pub fn set_pending_wervik_connect(&mut self, pending: PendingWervikConnect) {
        self.state.pending_wervik_connect = Some(pending);
        self.persist();
    }

    pub fn clear_pending_wervik_connect(&mut self) {
        self.state.pending_wervik_connect = None;
        self.persist();
    }

    pub fn rom_directories(&self) -> Vec<RomDirectoryConfig> {
        self.state.rom_directories.clone()
    }

    pub fn add_rom_directory(&mut self, path: String, platform_key: Option<String>) {
        if !self.state.rom_directories.iter().any(|dir| dir.path == path) {
            self.state
                .rom_directories
                .push(RomDirectoryConfig { path, platform_key });
            self.persist();
        }
    }

    /// Changes an already-added directory's platform override — `None`
    /// reverts it to auto-detection. A no-op (returns `false`) if `path`
    /// isn't a configured directory, so the caller can tell "nothing to
    /// persist" apart from a real change.
    pub fn set_rom_directory_platform(&mut self, path: &str, platform_key: Option<String>) -> bool {
        let Some(dir) = self.state.rom_directories.iter_mut().find(|dir| dir.path == path) else {
            return false;
        };
        dir.platform_key = platform_key;
        self.persist();
        true
    }

    pub fn remove_rom_directory(&mut self, path: &str) {
        self.state.rom_directories.retain(|dir| dir.path != path);
        self.persist();
    }

    pub fn retro_playtime(&self, game_id: &str) -> RetroPlaytime {
        self.state
            .retro_playtime
            .get(game_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the old JSON playtime entries so the first SQLite startup can
    /// import them without exposing the rest of the store's representation.
    pub fn legacy_retro_playtime(&self) -> Vec<(String, RetroPlaytime)> {
        self.state
            .retro_playtime
            .iter()
            .map(|(game_id, playtime)| (game_id.clone(), playtime.clone()))
            .collect()
    }

    pub fn local_last_played(&self, game_id: &str) -> Option<i64> {
        self.state.local_last_played.get(game_id).copied()
    }

    pub fn record_local_launch(&mut self, game_id: &str, played_at: i64) {
        self.state
            .local_last_played
            .insert(game_id.to_string(), played_at);
        self.persist();
    }

    /// Keeps the legacy JSON retro total in sync with the SQLite playtime
    /// store while older installations migrate.
    pub fn record_retro_session(&mut self, game_id: &str, minutes: u64, played_at: i64) {
        let entry = self
            .state
            .retro_playtime
            .entry(game_id.to_string())
            .or_default();
        entry.total_minutes += minutes;
        entry.last_played = Some(played_at);
        self.persist();
    }

    pub fn bios_path(&self, platform_key: &str) -> Option<String> {
        self.state.platform_bios_paths.get(platform_key).cloned()
    }

    pub fn set_bios_path(&mut self, platform_key: &str, path: Option<String>) {
        match path {
            Some(path) => {
                self.state
                    .platform_bios_paths
                    .insert(platform_key.to_string(), path);
            }
            None => {
                self.state.platform_bios_paths.remove(platform_key);
            }
        }
        self.persist();
    }

    /// Moves BIOS choices from the old per-ROM representation to the shared
    /// per-platform representation. Existing platform choices win when an
    /// old save contains more than one ROM from the same platform.
    pub fn migrate_legacy_bios_paths(&mut self, mappings: Vec<(String, String)>) -> bool {
        let mut migrated = false;
        for (game_id, platform_key) in mappings {
            let Some(path) = self.state.bios_paths.remove(&game_id) else {
                continue;
            };
            self.state
                .platform_bios_paths
                .entry(platform_key)
                .or_insert(path);
            migrated = true;
        }
        if migrated {
            self.persist();
        }
        migrated
    }

    /// The user's explicit "Default Emulator" choice for this platform
    /// (`"emulatorjs"` or a native emulator id) — `None` if they haven't
    /// picked one yet, in which case the caller falls back to the best
    /// available native emulator (see `commands::selected_emulator_id`).
    pub fn default_emulator(&self, platform_key: &str) -> Option<String> {
        self.state
            .platform_default_emulator
            .get(platform_key)
            .cloned()
    }

    pub fn set_default_emulator(&mut self, platform_key: &str, emulator_id: &str) {
        self.state
            .platform_default_emulator
            .insert(platform_key.to_string(), emulator_id.to_string());
        self.persist();
    }

    /// `"original"` unless the user has explicitly chosen `"optimized"` for
    /// this platform.
    pub fn graphics_preset(&self, platform_key: &str) -> String {
        self.state
            .platform_graphics_preset
            .get(platform_key)
            .cloned()
            .unwrap_or_else(|| "original".to_string())
    }

    pub fn set_graphics_preset(&mut self, platform_key: &str, preset: &str) {
        self.state
            .platform_graphics_preset
            .insert(platform_key.to_string(), preset.to_string());
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wervik-storage-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_fresh_store_has_no_default_emulator_choice() {
        let dir = scratch_dir("fresh");
        let store = Store::load(&dir);
        assert_eq!(store.default_emulator("nintendo_64"), None);
    }

    #[test]
    fn migrates_legacy_single_library_filters_to_lists() {
        let dir = scratch_dir("migrate-library-filters");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"settings":{"librarySourceFilter":"retro","libraryPlatformFilter":"SNES"}}"#,
        )
        .unwrap();

        let store = Store::load(&dir);
        let settings = store.settings();

        assert_eq!(settings.library_source_filter, vec!["retro"]);
        assert_eq!(settings.library_platform_filter, vec!["SNES"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_plain_string_rom_directories_to_untagged_configs() {
        let dir = scratch_dir("migrate-rom-directories");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"romDirectories":["D:\\Roms\\SNES","D:\\Roms\\Misc"]}"#,
        )
        .unwrap();

        let store = Store::load(&dir);

        assert_eq!(
            store.rom_directories(),
            vec![
                RomDirectoryConfig {
                    path: r"D:\Roms\SNES".to_string(),
                    platform_key: None,
                },
                RomDirectoryConfig {
                    path: r"D:\Roms\Misc".to_string(),
                    platform_key: None,
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rom_directory_platform_override_round_trips() {
        let dir = scratch_dir("rom-directory-platform");
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::load(&dir);

        store.add_rom_directory("D:\\Roms\\Arcade2".to_string(), None);
        assert!(store.set_rom_directory_platform("D:\\Roms\\Arcade2", Some("arcade".to_string())));
        assert_eq!(
            store.rom_directories(),
            vec![RomDirectoryConfig {
                path: "D:\\Roms\\Arcade2".to_string(),
                platform_key: Some("arcade".to_string()),
            }]
        );

        // Persisted, not just in memory — a fresh load sees the same override.
        let reloaded = Store::load(&dir);
        assert_eq!(reloaded.rom_directories(), store.rom_directories());

        assert!(!store.set_rom_directory_platform("D:\\Not\\Configured", Some("nes".to_string())));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact real-world case that motivated migrating rather than just
    /// dropping the old fields: a save file from before the "Default
    /// Emulator" model existed, where the user had already picked gopher64
    /// over mupen64plus for N64.
    #[test]
    fn migrates_a_saved_native_emulator_choice_into_the_new_default_emulator_field() {
        let dir = scratch_dir("migrate-native");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"platformNativeEmulator": {"nintendo_64": "gopher64"}}"#,
        )
        .unwrap();

        let store = Store::load(&dir);

        assert_eq!(
            store.default_emulator("nintendo_64"),
            Some("gopher64".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mirrors old `resolve_native_emulator`'s check order: `prefer_native
    /// == false` gated everything else, so it must win over any remembered
    /// native emulator id during migration too.
    #[test]
    fn an_explicit_prefer_native_false_migrates_to_an_emulatorjs_default() {
        let dir = scratch_dir("migrate-prefer-false");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"platformPreferNative": {"snes": false}, "platformNativeEmulator": {"snes": "some-emulator"}}"#,
        )
        .unwrap();

        let store = Store::load(&dir);

        assert_eq!(
            store.default_emulator("snes"),
            Some("emulatorjs".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_persists_so_the_old_fields_are_not_reprocessed_on_the_next_load() {
        let dir = scratch_dir("migrate-persist");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"platformNativeEmulator": {"nintendo_64": "gopher64"}}"#,
        )
        .unwrap();

        let _ = Store::load(&dir);
        let saved = std::fs::read_to_string(dir.join("wervik-state.json")).unwrap();

        assert!(saved.contains("\"nintendo_64\": \"gopher64\""));
        assert!(
            !saved.contains("platformNativeEmulator")
                || saved.contains("\"platformNativeEmulator\": {}")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_legacy_bios_choices_to_the_platform_and_preserves_existing_choices() {
        let dir = scratch_dir("migrate-bios");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wervik-state.json"),
            r#"{"biosPaths":{"rom-nes":"old-nes.zip","rom-psx":"old-psx.bin"},"platformBiosPaths":{"play_station":"existing.bin"}}"#,
        )
        .unwrap();

        let mut store = Store::load(&dir);
        assert!(store.migrate_legacy_bios_paths(vec![
            ("rom-nes".to_string(), "nes".to_string()),
            ("rom-psx".to_string(), "play_station".to_string()),
        ]));

        assert_eq!(store.bios_path("nes"), Some("old-nes.zip".to_string()));
        assert_eq!(
            store.bios_path("play_station"),
            Some("existing.bin".to_string())
        );
        let saved = std::fs::read_to_string(dir.join("wervik-state.json")).unwrap();
        assert!(saved.contains("platformBiosPaths"));
        assert!(!saved.contains("rom-nes"));
        assert!(!saved.contains("rom-psx"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
