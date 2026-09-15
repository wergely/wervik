//! Recursive ROM discovery: scans configured directories, identifies each
//! ROM's platform, collapses multi-file PlayStation discs into one game per
//! `.cue`, and assigns each a stable id — using only cheap filesystem
//! metadata for loose files (a `.zip`'s central directory listing is the
//! one exception — see `platform_for_zip`), and without ever touching the
//! ROM directories themselves other than reading.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::naming::{clean_display_name, region_label, region_rank};
use super::platform::Platform;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomGame {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub rom_path: String,
    pub size_bytes: u64,
    pub modified_at: i64,
    /// Other regional dumps of this same game that `dedupe_by_name` found
    /// and set aside rather than discarded — e.g. a "(USA)" dump wins as
    /// the default (see `naming::region_rank`), but its "(Europe)"/"(Japan)"
    /// siblings stay reachable so the user can still pick one to play
    /// instead (see `commands::launch_retro_game`'s `variant_id`).
    #[serde(default)]
    pub variants: Vec<RomVariant>,
}

/// One alternate regional dump of a `RomGame`, set aside by `dedupe_by_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RomVariant {
    pub id: String,
    pub rom_path: String,
    /// Original display title, useful when IGDB groups localized titles that
    /// do not share the same filename stem.
    #[serde(default)]
    pub name: String,
    /// Human label for the picker (e.g. "USA", "Europe", "Japan") — derived
    /// from the dump's own filename tag (see `naming::region_label`);
    /// "Original" when none was recognized.
    pub region: String,
}

impl RomVariant {
    fn from_game(game: &RomGame) -> Self {
        RomVariant {
            id: game.id.clone(),
            rom_path: game.rom_path.clone(),
            name: game.name.clone(),
            region: region_label(&game.rom_path),
        }
    }
}

const EXTRA_EXTENSIONS: &[(&str, Platform)] = &[("bin", Platform::Genesis)];

/// Picks the same entry out of a ROM zip's file listing that both
/// `platform_for_zip` (to identify the platform) and
/// `extract_rom_from_zip` (to actually get a real file gopher64-style
/// native emulators can open — unlike EmulatorJS, they can't read zips
/// themselves) care about: prefers a `.cue`/`.chd` over a `.bin`, mirroring
/// the "the disc sheet represents the whole game" preference `walk`
/// applies to loose PlayStation dumps.
fn best_zip_entry_name<R: std::io::Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
) -> Option<String> {
    let mut bin_fallback = None;
    for name in archive.file_names() {
        let Some(ext) = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        if Platform::from_unambiguous_extension(&ext).is_some() {
            return Some(name.to_string());
        }
        if ext == "bin" && bin_fallback.is_none() {
            bin_fallback = Some(name.to_string());
        }
    }
    bin_fallback
}

/// A `.zip`'s own extension can't tell us its platform the way a loose
/// ROM's can, so this peeks at the archive's central directory — cheap,
/// no decompression — to find an inner file with a recognizable extension.
/// Returns `None` for anything that isn't a valid zip, or has no
/// recognizable ROM inside — Wervik skips it rather than guessing.
///
/// A `.cue` entry additionally has to pass one of `disc`'s real
/// disc-content checks before being accepted as PlayStation *or* Neo Geo
/// CD: `.cue` is also used by several CD-based systems Wervik doesn't
/// support at all (Sega CD, PC-FX, 3DO...), and the extension alone can't
/// tell any of them apart — verified directly against a real Neo Geo CD
/// zip that this exact ambiguity misclassified as PlayStation before this
/// check existed.
fn platform_for_zip(path: &Path) -> Option<Platform> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let name = best_zip_entry_name(&archive)?;
    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)?;
    let platform = Platform::from_unambiguous_extension(&ext)
        .or_else(|| (ext == "bin").then_some(Platform::Genesis))?;

    if matches!(platform, Platform::PlayStation | Platform::PlayStation2) {
        return match ext.as_str() {
            "cue" if super::disc::zip_cue_references_a_playstation_disc(&mut archive, &name) => {
                Some(Platform::PlayStation)
            }
            "cue" if super::disc::zip_cue_references_a_neo_geo_cd_disc(&mut archive, &name) => {
                Some(Platform::NeoGeoCd)
            }
            // A zipped `.chd` isn't cheaply verifiable (CHD needs to seek
            // within the stream; a zip entry reader doesn't support that)
            // — and zipping an already-compressed CHD is unusual enough in
            // practice that skipping it rather than guessing costs nothing real.
            "iso" if super::disc::zip_iso_is_a_playstation2_disc(&mut archive, &name) => {
                Some(Platform::PlayStation2)
            }
            "iso" if super::disc::zip_iso_is_a_playstation_disc(&mut archive, &name) => {
                Some(Platform::PlayStation)
            }
            _ => None,
        };
    }
    Some(platform)
}

/// Extracts the ROM `best_zip_entry_name` identifies out of `zip_path` into
/// `dest_dir`, returning its path — needed only for native emulators that
/// (unlike EmulatorJS's own in-browser decompression) can't read a zipped
/// ROM directly. The caller is responsible for cleaning up `dest_dir` once
/// done with it (see `commands::launch_retro_game`).
///
/// A `.cue` sheet (PlayStation or Neo Geo CD) references its `.bin` track
/// files by name, expecting them to sit right next to it on disk —
/// extracting only the `.cue` itself leaves every track missing and the
/// native emulator fails to parse the disc. Verified for real: FBNeo's
/// "Couldn't parse CD image" against an actual multi-track Neo Geo CD dump
/// whose `.cue` had been extracted alone. So for a `.cue`, every entry in
/// the zip is extracted alongside it instead of just the one file.
pub fn extract_rom_from_zip(zip_path: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let name = best_zip_entry_name(&archive)
        .ok_or_else(|| "This zip has no recognizable ROM inside it".to_string())?;
    let file_name = Path::new(&name)
        .file_name()
        .ok_or_else(|| "ROM entry has no file name".to_string())?
        .to_owned();

    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;

    let is_disc_sheet = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cue"));
    if is_disc_sheet {
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let Some(entry_file_name) = Path::new(entry.name()).file_name() else {
                continue;
            };
            let mut out =
                std::fs::File::create(dest_dir.join(entry_file_name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
        return Ok(dest_dir.join(file_name));
    }

    let mut entry = archive.by_name(&name).map_err(|e| e.to_string())?;
    let dest_path = dest_dir.join(file_name);
    let mut out = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    Ok(dest_path)
}

/// Recursively scans every directory in `dirs` and returns every ROM found.
/// This does real filesystem walking (`read_dir`/`metadata` calls) — safe
/// to call from a background task, but callers on the UI thread should
/// prefer the cached results from a previous scan (see `retro::cache`) and
/// only call this for an explicit user-triggered rescan or first run.
pub fn scan_directories(dirs: &[String]) -> Vec<RomGame> {
    let mut games = Vec::new();
    let mut seen_ids = HashSet::new();
    for dir in dirs {
        let root = Path::new(dir);
        let mut cue_dirs = HashSet::new();
        let mut arcade_dirs = HashSet::new();
        collect_cue_dirs(root, &mut cue_dirs);
        collect_arcade_dirs(root, &mut arcade_dirs);
        walk(root, &cue_dirs, &arcade_dirs, &mut games, &mut seen_ids);
    }
    dedupe_by_name(games)
}

/// Collapses the same game appearing more than once in the library — e.g.
/// a ROM set's main folder and its "Extras"/rerelease subfolder both
/// having their own dump of "Super Mario World" — down to one entry per
/// (platform, cleaned display name). Picks the best of each group by
/// region first (USA/World over anything else — see `naming::region_rank`
/// — since e.g. some N64 PAL releases genuinely run slower than their
/// NTSC counterpart, not just a settings difference), then, among dumps
/// with the same region rank, the largest file (in practice the
/// least-compressed/most complete dump; ties keep whichever was found
/// first). This is a heuristic on the display name, not the file's
/// contents: two genuinely different games that happen to share a cleaned
/// title would also collapse into one. That's an accepted trade-off — the
/// alternative is a many-thousand-game library never getting deduplicated
/// at all, which is the actual problem this solves.
fn dedupe_by_name(games: Vec<RomGame>) -> Vec<RomGame> {
    let mut index_by_key: HashMap<(Platform, String), usize> = HashMap::new();
    let mut result: Vec<RomGame> = Vec::with_capacity(games.len());
    for mut game in games {
        let key = (game.platform, game.name.clone());
        match index_by_key.get(&key) {
            Some(&index) => {
                let (new_rank, current_rank) = (
                    region_rank(&game.rom_path),
                    region_rank(&result[index].rom_path),
                );
                let is_better = new_rank < current_rank
                    || (new_rank == current_rank && game.size_bytes > result[index].size_bytes);
                if is_better {
                    // The previous winner doesn't disappear — it becomes an
                    // alternate variant of the new one, same as any variant
                    // already set aside underneath it.
                    let outgoing = RomVariant::from_game(&result[index]);
                    let mut variants = std::mem::take(&mut result[index].variants);
                    variants.push(outgoing);
                    game.variants = variants;
                    result[index] = game;
                } else {
                    result[index].variants.push(RomVariant::from_game(&game));
                }
            }
            None => {
                index_by_key.insert(key, result.len());
                result.push(game);
            }
        }
    }
    result
}

/// Collapses localized or provider-specific names that point to the same
/// IGDB game, while keeping each ROM playable as a variant. The platform is
/// part of the key because an IGDB game can legitimately have releases on
/// more than one platform.
pub(crate) fn dedupe_by_igdb_id(
    games: Vec<RomGame>,
    igdb_ids: &HashMap<String, u64>,
) -> (Vec<RomGame>, bool) {
    let mut index_by_key: HashMap<(Platform, u64), usize> = HashMap::new();
    let mut result: Vec<RomGame> = Vec::with_capacity(games.len());
    let mut changed = false;

    for mut game in games {
        let Some(&igdb_id) = igdb_ids.get(&game.id) else {
            result.push(game);
            continue;
        };
        let key = (game.platform, igdb_id);
        let Some(&index) = index_by_key.get(&key) else {
            index_by_key.insert(key, result.len());
            result.push(game);
            continue;
        };

        changed = true;
        let (new_rank, current_rank) = (
            region_rank(&game.rom_path),
            region_rank(&result[index].rom_path),
        );
        let is_better = new_rank < current_rank
            || (new_rank == current_rank && game.size_bytes > result[index].size_bytes);
        if is_better {
            // Replace the visible title with the preferred regional dump,
            // but preserve both the old winner and any variants it already
            // carried.
            let mut old_winner = std::mem::replace(&mut result[index], game);
            let new_winner = &mut result[index];
            let mut variants = std::mem::take(&mut old_winner.variants);
            variants.extend(std::mem::take(&mut new_winner.variants));
            variants.push(RomVariant::from_game(&old_winner));
            new_winner.variants = variants;
        } else {
            let incoming_variants = std::mem::take(&mut game.variants);
            result[index].variants.extend(incoming_variants);
            result[index].variants.push(RomVariant::from_game(&game));
        }
    }

    (result, changed)
}

fn collect_cue_dirs(dir: &Path, cue_dirs: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_cue_dirs(&path, cue_dirs);
        } else if is_extension(&path, "cue") {
            if let Some(parent) = path.parent() {
                cue_dirs.insert(parent.to_path_buf());
            }
        }
    }
}

/// Arcade ROM sets are usually ZIP files whose members have chip-style names
/// such as `sfa3.03` rather than a console extension. Wervik only classifies
/// those archives as Arcade inside an explicitly named arcade directory, so
/// an unrelated ZIP in a general ROM folder is never mistaken for a game.
fn collect_arcade_dirs(dir: &Path, arcade_dirs: &mut HashSet<PathBuf>) {
    let inherited = dir
        .parent()
        .is_some_and(|parent| arcade_dirs.contains(parent));
    let named_arcade = dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_arcade_directory_name);
    if inherited || named_arcade {
        arcade_dirs.insert(dir.to_path_buf());
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            collect_arcade_dirs(&path, arcade_dirs);
        }
    }
}

/// ROM-set collections frequently use descriptive names instead of a plain
/// `Arcade` folder — for example `CapcomCPS2ByDataghost` and
/// `neo-geo-aes-romset`. Their ZIP members have names such as `kof97.p1`
/// or `sfa3.03`, so the normal ZIP extension probe falls back to `.bin` and
/// would otherwise label the whole set as Genesis. Match the well-known
/// arcade family tokens in the directory name while keeping generic ZIP
/// folders conservative.
pub(super) fn is_arcade_directory_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();

    matches!(
        compact.as_str(),
        "arcade" | "arcades" | "cps" | "cps1" | "cps2" | "cps3" | "neogeo" | "fbneo"
    ) || compact.contains("arcade")
        || compact.contains("cps")
        || compact.contains("neogeo")
        || compact.contains("fbneo")
        || compact.contains("mame")
}

fn walk(
    dir: &Path,
    cue_dirs: &HashSet<PathBuf>,
    arcade_dirs: &HashSet<PathBuf>,
    games: &mut Vec<RomGame>,
    seen_ids: &mut HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, cue_dirs, arcade_dirs, games, seen_ids);
            continue;
        }

        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };

        // A standalone .bin sitting beside a .cue is a disc track, not a
        // game on its own — the .cue represents the whole game.
        if ext == "bin" && cue_dirs.contains(dir) {
            continue;
        }

        let platform = if ext == "zip" {
            let platform = match platform_for_zip(&path) {
                Some(platform) => platform,
                None if arcade_dirs.contains(dir) => Platform::Arcade,
                None => continue,
            };
            // A CPS/Neo Geo archive often contains only `.bin` members;
            // `platform_for_zip` deliberately treats a loose `.bin` as
            // Genesis, which is correct for console dumps but ambiguous for
            // arcade chip archives. The folder context resolves that one
            // ambiguity without weakening the general Genesis rule.
            if arcade_dirs.contains(dir) && platform == Platform::Genesis {
                Platform::Arcade
            } else {
                platform
            }
        } else {
            let Some(platform) = Platform::from_unambiguous_extension(&ext).or_else(|| {
                EXTRA_EXTENSIONS
                    .iter()
                    .find(|(e, _)| *e == ext)
                    .map(|(_, p)| *p)
            }) else {
                continue;
            };
            // `.cue`/`.chd` alone don't unambiguously mean PlayStation —
            // several CD-based systems Wervik doesn't support at all (Sega
            // CD, PC-FX, 3DO...) use the exact same formats, and Neo Geo
            // CD (which Wervik does support) uses them too. Resolved
            // against the disc's own data before accepting either one —
            // real Sega CD `.chd` dumps and Neo Geo CD zips were found
            // silently misclassified as PlayStation without this check.
            // `.iso` is similarly ambiguous, but only between PS1 and PS2
            // (see `disc::iso_is_a_playstation2_disc`'s doc comment).
            let platform = if matches!(platform, Platform::PlayStation | Platform::PlayStation2) {
                let resolved = match ext.as_str() {
                    "cue" if super::disc::cue_references_a_playstation_disc(&path) => {
                        Some(Platform::PlayStation)
                    }
                    "cue" if super::disc::cue_references_a_neo_geo_cd_disc(&path) => {
                        Some(Platform::NeoGeoCd)
                    }
                    "chd" if super::disc::chd_is_a_playstation_disc(&path) => {
                        Some(Platform::PlayStation)
                    }
                    "chd" if super::disc::chd_is_a_neo_geo_cd_disc(&path) => {
                        Some(Platform::NeoGeoCd)
                    }
                    "iso" if super::disc::iso_is_a_playstation2_disc(&path) => {
                        Some(Platform::PlayStation2)
                    }
                    "iso" if super::disc::iso_is_a_playstation_disc(&path) => {
                        Some(Platform::PlayStation)
                    }
                    _ => None,
                };
                let Some(resolved) = resolved else { continue };
                resolved
            } else {
                platform
            };
            platform
        };

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let size_bytes = metadata.len();
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let id = stable_id(file_name, size_bytes);
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        games.push(RomGame {
            id,
            name: clean_display_name(file_name),
            platform,
            rom_path: path.to_string_lossy().to_string(),
            size_bytes,
            modified_at,
            variants: Vec::new(),
        });
    }
}

fn is_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// A stable, cheap identifier derived only from the file name and size
/// (both free from filesystem metadata) — never the file's contents, since
/// hashing multi-gigabyte ROMs on every scan would be far too slow. Stable
/// across rescans and across moving a ROM between configured directories,
/// as long as the file itself isn't renamed or resized.
fn stable_id(file_name: &str, size_bytes: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file_name.hash(&mut hasher);
    size_bytes.hash(&mut hasher);
    format!("rom-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str, contents: &[u8]) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    /// Fake but realistic-enough disc-track bytes to pass
    /// `disc::looks_like_playstation_disc`'s real signature check: an
    /// ISO9660 marker ("CD001") plus the PlayStation-specific volume label
    /// at the same offsets a real Mode 1/2352 disc image has them.
    fn fake_playstation_track_bytes() -> Vec<u8> {
        let mut track = vec![0u8; 40_000];
        track[37649..37649 + 5].copy_from_slice(b"CD001");
        track[37700..37700 + 11].copy_from_slice(b"PLAYSTATION");
        track
    }

    #[test]
    fn discovers_supported_extensions_and_skips_unknown_ones() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Super Mario World (USA) (Rev 1).sfc", b"snes-rom");
        touch(&dir, "readme.txt", b"not a rom");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Super Mario World");
        assert_eq!(games[0].platform, Platform::Snes);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collapses_multi_track_playstation_discs_into_one_game() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-cue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(
            &dir,
            "Final Fantasy VII (USA) (Disc 1).cue",
            b"FILE \"track01.bin\" BINARY\n",
        );
        touch(&dir, "track01.bin", &fake_playstation_track_bytes());
        touch(&dir, "track02.bin", b"track-2-data");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(
            games.len(),
            1,
            "the .cue must represent the whole game, tracks must not appear separately"
        );
        assert_eq!(games[0].platform, Platform::PlayStation);
        assert!(games[0].rom_path.ends_with(".cue"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real bug this guards against: `.cue`/`.chd` are also used by
    /// CD-based systems Wervik doesn't support (Sega CD, Neo Geo CD...),
    /// and were found silently misclassifying every single one of them as
    /// PlayStation — an entire real library's "PlayStation" section (364
    /// games) turned out to be 100% misidentified Sega CD/Neo Geo CD discs.
    #[test]
    fn a_non_playstation_disc_with_a_cue_or_chd_extension_is_skipped_not_misclassified() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-not-ps1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // A real ISO9660 CD-ROM (like a genuine Neo Geo CD/Sega CD dump)
        // that just isn't PlayStation — has "CD001" but not "PLAYSTATION".
        let mut not_ps1_track = vec![0u8; 40_000];
        not_ps1_track[37649..37649 + 5].copy_from_slice(b"CD001");
        touch(
            &dir,
            "Some Other System Game.cue",
            b"FILE \"track01.bin\" BINARY\n",
        );
        touch(&dir, "track01.bin", &not_ps1_track);

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(
            games.len(),
            0,
            "a non-PlayStation disc must be skipped, not tagged as PlayStation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fake but realistic-enough disc bytes to pass
    /// `disc::iso_is_a_playstation2_disc`'s real signature check.
    fn fake_playstation2_iso_bytes() -> Vec<u8> {
        let mut image = vec![0u8; 40_000];
        image[32768..32768 + 5].copy_from_slice(b"CD001");
        image[33000..33000 + 5].copy_from_slice(b"BOOT2");
        image
    }

    #[test]
    fn a_loose_iso_with_a_boot2_line_is_playstation2() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-ps2-iso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Some PS2 Game.iso", &fake_playstation2_iso_bytes());

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::PlayStation2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `.iso` is occasionally a single-track PS1 dump too — it must resolve
    /// to PS1, not PS2, when the disc's own `SYSTEM.CNF` says so.
    #[test]
    fn a_loose_iso_without_boot2_but_with_a_playstation_label_is_playstation() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-ps1-iso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Some PS1 Game.iso", &fake_playstation_track_bytes());

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::PlayStation);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_loose_iso_matching_neither_signature_is_skipped_not_misclassified() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-not-ps-iso-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut not_ps_image = vec![0u8; 40_000];
        not_ps_image[32768..32768 + 5].copy_from_slice(b"CD001");
        touch(&dir, "Some Other System Game.iso", &not_ps_image);

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collapses_the_same_game_found_in_two_places_into_one_entry() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-dupe-{}", std::process::id()));
        let extras = dir.join("Extras");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&extras).unwrap();

        touch(
            &dir,
            "Super Mario World (USA).sfc",
            b"the-larger-more-complete-dump",
        );
        touch(&extras, "Super Mario World (Virtual Console).sfc", b"tiny");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(
            games.len(),
            1,
            "the same cleaned title in two folders must collapse to one game"
        );
        assert!(
            games[0].rom_path.ends_with("Super Mario World (USA).sfc"),
            "the larger dump should win"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Region beats file size: a smaller USA dump must still win over a
    /// larger Europe dump of the same game — some N64 (and other) PAL
    /// releases genuinely run slower, so "biggest file" alone would keep
    /// picking the wrong one.
    #[test]
    fn a_usa_dump_wins_over_a_larger_europe_dump_of_the_same_game() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-region-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Chrono Trigger (USA).sfc", b"small");
        touch(&dir, "Chrono Trigger (Europe).sfc", b"a much larger dump");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert!(
            games[0].rom_path.ends_with("Chrono Trigger (USA).sfc"),
            "the USA dump should win despite being smaller"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The runner-up dump doesn't just vanish — it's kept as a `RomVariant`
    /// the user can still choose to play instead (see
    /// `commands::launch_retro_game`'s `variant_id`).
    #[test]
    fn the_non_default_dump_is_kept_as_a_playable_variant_not_discarded() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-variants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Chrono Trigger (USA).sfc", b"small");
        touch(&dir, "Chrono Trigger (Europe).sfc", b"a much larger dump");
        touch(&dir, "Chrono Trigger (Japan).sfc", b"also a dump");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(
            games[0].variants.len(),
            2,
            "both runner-up dumps should be kept, not discarded"
        );
        let regions: std::collections::HashSet<_> = games[0]
            .variants
            .iter()
            .map(|v| v.region.as_str())
            .collect();
        assert_eq!(
            regions,
            std::collections::HashSet::from(["Europe", "Japan"])
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn collapses_localized_titles_with_the_same_igdb_id_into_variants() {
        let games = vec![
            RomGame {
                id: "japan".to_string(),
                name: "Banjo to Kazooie no Daibouken".to_string(),
                platform: Platform::Nintendo64,
                rom_path: "D:/Games/Banjo (Japan).zip".to_string(),
                size_bytes: 100,
                modified_at: 0,
                variants: Vec::new(),
            },
            RomGame {
                id: "usa".to_string(),
                name: "Banjo-Kazooie".to_string(),
                platform: Platform::Nintendo64,
                rom_path: "D:/Games/Banjo-Kazooie (USA).zip".to_string(),
                size_bytes: 90,
                modified_at: 0,
                variants: Vec::new(),
            },
        ];
        let igdb_ids = HashMap::from([("japan".to_string(), 2148), ("usa".to_string(), 2148)]);

        let (games, changed) = dedupe_by_igdb_id(games, &igdb_ids);

        assert!(changed);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Banjo-Kazooie");
        assert_eq!(games[0].variants.len(), 1);
        assert_eq!(games[0].variants[0].id, "japan");
        assert_eq!(games[0].variants[0].name, "Banjo to Kazooie no Daibouken");
        assert_eq!(games[0].variants[0].region, "Japan");
    }

    #[test]
    fn does_not_collapse_the_same_name_across_different_platforms() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-samename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Aladdin.sfc", b"snes-version");
        touch(&dir, "Aladdin.md", b"genesis-version-longer");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(
            games.len(),
            2,
            "the same title on two different platforms is not a duplicate"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn treats_standalone_bin_as_genesis() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        touch(&dir, "Sonic The Hedgehog (World).bin", b"genesis-rom");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::Genesis);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deflate, not the library default of Stored — real-world ROM zips
    /// are (near) always Deflate-compressed, and a real bug (missing the
    /// `zip` crate's `deflate` feature) only ever showed up against an
    /// actual Deflated zip, not an uncompressed one.
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            std::io::Write::write_all(&mut writer, contents).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn identifies_platform_by_peeking_inside_a_zip() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        write_zip(
            &dir.join("Chrono Trigger.zip"),
            &[("Chrono Trigger.sfc", b"snes-rom-bytes")],
        );

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Chrono Trigger");
        assert_eq!(games[0].platform, Platform::Snes);
        assert!(
            games[0].rom_path.ends_with(".zip"),
            "the zip itself is served, not an extracted file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identifies_a_zipped_playstation2_iso_by_peeking_inside_the_zip() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-zip-ps2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        write_zip(
            &dir.join("Some PS2 Game.zip"),
            &[("Some PS2 Game.iso", &fake_playstation2_iso_bytes())],
        );

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::PlayStation2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identifies_arcade_zip_sets_inside_an_arcade_directory() {
        let root = std::env::temp_dir().join(format!("wervik-rom-arcade-{}", std::process::id()));
        let arcade = root.join("Arcade").join("CPS2");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&arcade).unwrap();

        write_zip(
            &arcade.join("sfz3.zip"),
            &[("sfa3.03", b"program-rom"), ("sfa3.13m", b"sprite-rom")],
        );

        let games = scan_directories(&[root.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "sfz3");
        assert_eq!(games[0].platform, Platform::Arcade);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn identifies_common_named_cps_and_neo_geo_romset_directories_as_arcade() {
        let root =
            std::env::temp_dir().join(format!("wervik-rom-arcade-named-{}", std::process::id()));
        let cps = root.join("CapcomCPS2ByDataghost");
        let neo_geo = root.join("neo-geo-aes-romset");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&cps).unwrap();
        std::fs::create_dir_all(&neo_geo).unwrap();

        // `.bin` is the deliberately conservative fallback for an arcade
        // ZIP with chip-style members. The directory name supplies the
        // missing context and must promote it to Arcade.
        write_zip(
            &cps.join("sfz3.zip"),
            &[("sfa3.03", b"program-rom"), ("sfa3.13m", b"sprite-rom")],
        );
        write_zip(
            &neo_geo.join("kof97.zip"),
            &[("kof97.p1", b"program-rom"), ("kof97.c1", b"sprite-rom")],
        );

        let games = scan_directories(&[root.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 2);
        assert!(games.iter().all(|game| game.platform == Platform::Arcade));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prefers_cue_over_bin_inside_a_playstation_zip() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-zip-psx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        write_zip(
            &dir.join("Final Fantasy VII.zip"),
            &[
                ("game.cue", b"FILE \"track01.bin\" BINARY\n"),
                ("track01.bin", &fake_playstation_track_bytes()),
            ],
        );

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::PlayStation);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same real bug as the loose-file case, but for a zip whose `.cue`
    /// references a non-PlayStation CD track — a Neo Geo CD zip, in
    /// particular, is exactly this shape (multiple `.bin` tracks + one
    /// `.cue`), and was being silently misclassified as PlayStation before
    /// this check existed.
    #[test]
    fn a_zip_with_a_non_playstation_cue_is_skipped_not_misclassified() {
        let dir =
            std::env::temp_dir().join(format!("wervik-rom-zip-not-ps1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut not_ps1_track = vec![0u8; 40_000];
        not_ps1_track[37649..37649 + 5].copy_from_slice(b"CD001");
        write_zip(
            &dir.join("Some Neo Geo CD Game.zip"),
            &[
                ("game.cue", b"FILE \"track01.bin\" BINARY\n"),
                ("track01.bin", &not_ps1_track),
            ],
        );

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(
            games.len(),
            0,
            "a non-PlayStation disc inside a zip must be skipped, not tagged as PlayStation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact real-world case this whole feature is for: a Neo Geo CD
    /// zip (multiple `.bin` tracks + one `.cue`, just like a real dump)
    /// must be correctly identified as Neo Geo CD — not just rejected as
    /// "not PlayStation" and thrown away.
    #[test]
    fn a_zip_with_a_neo_geo_cd_cue_is_identified_as_neo_geo_cd() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-zip-ngcd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut ngcd_track = vec![0u8; 50_000];
        ngcd_track[37649..37649 + 5].copy_from_slice(b"CD001");
        ngcd_track[48483..48483 + 7].copy_from_slice(b"IPL.TXT");
        write_zip(
            &dir.join("Some Neo Geo CD Game.zip"),
            &[
                ("game.cue", b"FILE \"track01.bin\" BINARY\n"),
                ("track01.bin", &ngcd_track),
            ],
        );

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].platform, Platform::NeoGeoCd);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_a_zip_with_no_recognizable_rom_inside() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-zip-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        write_zip(&dir.join("Manual.zip"), &[("readme.txt", b"not a rom")]);

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);
        assert_eq!(games.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Native emulators (e.g. mupen64plus) can't read a ROM straight out of
    /// a zip the way EmulatorJS's own in-browser decompression can — this
    /// is what `commands::launch_retro_game` calls to get a real,
    /// standalone file for them first.
    #[test]
    fn extracts_the_rom_out_of_a_zip_for_native_emulators() {
        let dir =
            std::env::temp_dir().join(format!("wervik-rom-zip-extract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("Super Mario 64.zip");
        write_zip(
            &zip_path,
            &[("Super Mario 64 (USA).z64", b"real-n64-rom-bytes")],
        );

        let dest_dir = dir.join("extracted");
        let extracted = extract_rom_from_zip(&zip_path, &dest_dir).unwrap();

        assert_eq!(extracted.file_name().unwrap(), "Super Mario 64 (USA).z64");
        assert_eq!(std::fs::read(&extracted).unwrap(), b"real-n64-rom-bytes");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real bug this fixes: a multi-track CD dump (Neo Geo CD, and
    /// latently PlayStation too) has its `.cue` reference several `.bin`
    /// tracks by name, sitting next to it. Extracting only the `.cue` (the
    /// old behavior) left every track missing on disk and the native
    /// emulator failed to parse the disc — reproduced for real against
    /// FBNeo with an actual multi-track Neo Geo CD zip ("Couldn't parse CD
    /// image"). Every track must land next to the extracted `.cue`.
    #[test]
    fn extracting_a_cue_from_a_zip_also_extracts_every_referenced_track() {
        let dir =
            std::env::temp_dir().join(format!("wervik-rom-zip-extract-cue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("Power Spikes II.zip");
        write_zip(
            &zip_path,
            &[
                ("Power Spikes II.cue", b"FILE \"Power Spikes II (Track 01).bin\" BINARY\nFILE \"Power Spikes II (Track 02).bin\" BINARY\n"),
                ("Power Spikes II (Track 01).bin", b"track-one-data"),
                ("Power Spikes II (Track 02).bin", b"track-two-data"),
            ],
        );

        let dest_dir = dir.join("extracted");
        let extracted = extract_rom_from_zip(&zip_path, &dest_dir).unwrap();

        assert_eq!(extracted.file_name().unwrap(), "Power Spikes II.cue");
        assert_eq!(std::fs::read(&extracted).unwrap(), b"FILE \"Power Spikes II (Track 01).bin\" BINARY\nFILE \"Power Spikes II (Track 02).bin\" BINARY\n");
        assert_eq!(
            std::fs::read(dest_dir.join("Power Spikes II (Track 01).bin")).unwrap(),
            b"track-one-data"
        );
        assert_eq!(
            std::fs::read(dest_dir.join("Power Spikes II (Track 02).bin")).unwrap(),
            b"track-two-data"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stable_id_is_deterministic_and_distinguishes_different_files() {
        assert_eq!(stable_id("game.sfc", 100), stable_id("game.sfc", 100));
        assert_ne!(stable_id("game.sfc", 100), stable_id("game.sfc", 200));
        assert_ne!(stable_id("a.sfc", 100), stable_id("b.sfc", 100));
    }

    #[test]
    fn recurses_into_subdirectories() {
        let dir = std::env::temp_dir().join(format!("wervik-rom-nested-{}", std::process::id()));
        let sub = dir.join("SNES");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&sub).unwrap();

        touch(&sub, "Chrono Trigger.sfc", b"data");

        let games = scan_directories(&[dir.to_string_lossy().to_string()]);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Chrono Trigger");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
