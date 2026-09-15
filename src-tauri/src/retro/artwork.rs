//! Best-effort automatic box art lookup against the public
//! [libretro-thumbnails](https://github.com/libretro-thumbnails) dataset —
//! the same source RetroArch itself pulls thumbnails from. No API key, no
//! account, no scraping of a site that doesn't want it: it's a plain,
//! public, static file host. If nothing matches, Wervik just shows no
//! cover art for that game rather than guessing or fabricating one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ureq::Agent;

use super::discovery::RomGame;
use super::naming::parenthesized_tags;
use super::platform::Platform;

fn repo_for(platform: Platform) -> Option<&'static str> {
    Some(match platform {
        Platform::Nes => "Nintendo_-_Nintendo_Entertainment_System",
        Platform::Snes => "Nintendo_-_Super_Nintendo_Entertainment_System",
        Platform::GameBoy => "Nintendo_-_Game_Boy",
        Platform::GameBoyColor => "Nintendo_-_Game_Boy_Color",
        Platform::GameBoyAdvance => "Nintendo_-_Game_Boy_Advance",
        Platform::MasterSystem => "Sega_-_Master_System_-_Mark_III",
        Platform::Genesis => "Sega_-_Mega_Drive_-_Genesis",
        Platform::GameGear => "Sega_-_Game_Gear",
        Platform::Sega32X => "Sega_-_32X",
        Platform::PlayStation => "Sony_-_PlayStation",
        Platform::Nintendo64 => "Nintendo_-_Nintendo_64",
        Platform::Atari5200 => "Atari_-_5200",
        Platform::Atari7800 => "Atari_-_7800",
        Platform::AtariLynx => "Atari_-_Lynx",
        Platform::AtariJaguar => "Atari_-_Jaguar",
        // libretro-thumbnails splits WonderSwan/Neo Geo Pocket into
        // separate mono vs. Color repos, but EmulatorJS's own core
        // grouping (and Wervik's `Platform` enum, matching it) treats each
        // pair as one system — this defaults to the mono repo, which is a
        // best-effort choice for the Color variant, not a guaranteed hit.
        Platform::WonderSwan => "Bandai_-_WonderSwan",
        Platform::NeoGeoPocket => "SNK_-_Neo_Geo_Pocket",
        Platform::PcEngine => "NEC_-_PC_Engine_-_TurboGrafx_16",
        Platform::ColecoVision => "Coleco_-_ColecoVision",
        Platform::PlayStation2 => "Sony_-_PlayStation_2",
        Platform::WiiU => "Nintendo_-_Wii_U",
        Platform::Arcade | Platform::NeoGeoCd => return None,
    })
}

fn cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("retro-artwork")
}

fn cache_path(app_data_dir: &Path, game_id: &str) -> PathBuf {
    cache_dir(app_data_dir).join(format!("{game_id}.png"))
}

/// Marks a game we've already searched libretro-thumbnails for and found
/// nothing — distinct from a cached cover, but just as final: without it,
/// the fetch pass that runs on every app startup would hit the network for
/// every game that will never have art (an obscure ROM hack, a title that
/// just isn't in the dataset) again and again, forever, since a miss never
/// produces a file to check for.
fn miss_marker_path(app_data_dir: &Path, game_id: &str) -> PathBuf {
    cache_dir(app_data_dir).join(format!("{game_id}.nomatch"))
}

/// The 8-byte signature every real PNG starts with — used to self-heal
/// from a cache entry that isn't actually a valid image. Real, found case:
/// `download`'s git-symlink-target bug (see its doc comment) saved a
/// GitHub symlink's raw *target path text* as if it were the cover itself
/// for any re-release symlinked to its original release's art — 237 of
/// them in one real library, all a few dozen bytes of plain text instead
/// of an image.
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

fn is_valid_cached_image(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic).is_ok() && magic == PNG_MAGIC
}

/// Returns a previously-fetched cover's path, if there is one. Never
/// touches the network — safe to call on every library snapshot. A cached
/// file that isn't actually a valid image (see `PNG_MAGIC`) is treated as
/// if it were never fetched at all, and removed so the next background
/// pass fetches it for real instead of leaving it permanently "resolved".
pub fn cached_cover(app_data_dir: &Path, game_id: &str) -> Option<PathBuf> {
    let path = cache_path(app_data_dir, game_id);
    if !path.exists() {
        return None;
    }
    if is_valid_cached_image(&path) {
        return Some(path);
    }
    let _ = std::fs::remove_file(&path);
    None
}

/// True once a game either has a cached cover or has already been searched
/// for and confirmed to have none (see `miss_marker_path`) — either way,
/// there's nothing left to fetch for it. This is what the background pass
/// uses to decide what still needs a network round trip, so it doesn't
/// redo the same futile lookups every time Wervik starts.
pub fn already_resolved(app_data_dir: &Path, game_id: &str) -> bool {
    cached_cover(app_data_dir, game_id).is_some()
        || miss_marker_path(app_data_dir, game_id).exists()
}

/// Tries to download box art for `rom` and cache it locally, trying the
/// ROM's original filename first — libretro-thumbnails names files after
/// No-Intro conventions, full region words and all (e.g. "(USA)") — then,
/// since a lot of ROM sets instead use older GoodTools-style abbreviated
/// region codes (e.g. "(U)", "[!]"), Wervik's cleaned-up title combined
/// with the equivalent No-Intro region name, then the cleaned title alone
/// as a last resort. Returns `Ok(None)` when nothing matched; that's an
/// expected, non-error outcome, not a failure.
pub fn fetch_cover(
    agent: &Agent,
    app_data_dir: &Path,
    rom: &RomGame,
) -> Result<Option<PathBuf>, String> {
    let dest = cache_path(app_data_dir, &rom.id);
    if dest.exists() {
        return Ok(Some(dest));
    }
    if miss_marker_path(app_data_dir, &rom.id).exists() {
        return Ok(None);
    }

    let original_stem = Path::new(&rom.rom_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&rom.name)
        .to_string();

    let mut candidates = vec![original_stem.clone()];
    for region in region_candidates(&original_stem) {
        candidates.push(format!("{} ({region})", rom.name));
    }
    if rom.name != original_stem {
        candidates.push(rom.name.clone());
    }

    let Some(repo) = repo_for(rom.platform) else {
        let dir = cache_dir(app_data_dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(miss_marker_path(app_data_dir, &rom.id), b"").map_err(|e| e.to_string())?;
        return Ok(None);
    };
    for name in candidates {
        // libretro-thumbnails' own file-sanitization for names that would
        // otherwise be invalid in a path.
        let sanitized = name.replace('&', "_");
        let url = format!(
            "https://raw.githubusercontent.com/libretro-thumbnails/{repo}/master/Named_Boxarts/{}.png",
            encode_path_segment(&sanitized)
        );

        match download(agent, &url) {
            Ok(Some(bytes)) => {
                let dir = cache_dir(app_data_dir);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
                return Ok(Some(dest));
            }
            Ok(None) => continue,
            Err(e) => return Err(e),
        }
    }
    let dir = cache_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(miss_marker_path(app_data_dir, &rom.id), b"").map_err(|e| e.to_string())?;
    Ok(None)
}

pub fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

/// GitHub raw content doesn't resolve git symlinks: for a path that's
/// actually a symlink (libretro-thumbnails uses these for re-releases
/// that share art with their original, e.g. a Wii Virtual Console port
/// pointing at the same box art as the original cartridge release), it
/// serves the *target path* as `text/plain`, not the real image bytes —
/// found for real via `Mario Party 2 (USA) (Wii Virtual Console).png`,
/// which resolved to a 23-byte text file, not a cover. Follows one level
/// of that before giving up, rather than caching the target's filename
/// as if it were a real image.
fn download(agent: &Agent, url: &str) -> Result<Option<Vec<u8>>, String> {
    download_following_symlinks(agent, url, 1)
}

fn download_following_symlinks(
    agent: &Agent,
    url: &str,
    remaining_redirects: u8,
) -> Result<Option<Vec<u8>>, String> {
    match agent.get(url).call() {
        Ok(mut response) => {
            let is_plain_text = response
                .body()
                .mime_type()
                .is_some_and(|mime| mime == "text/plain");
            let bytes = response
                .body_mut()
                .read_to_vec()
                .map_err(|e| e.to_string())?;

            if is_plain_text {
                if remaining_redirects > 0 {
                    if let Some(target) = symlink_target(&bytes) {
                        if let Some((dir, _)) = url.rsplit_once('/') {
                            let resolved_url = format!("{dir}/{}", encode_path_segment(target));
                            return download_following_symlinks(
                                agent,
                                &resolved_url,
                                remaining_redirects - 1,
                            );
                        }
                    }
                }
                // A `text/plain` response for what should be a `.png` isn't
                // a real cover, resolvable symlink or not — treat it as a
                // miss rather than caching whatever this is.
                return Ok(None);
            }
            Ok(Some(bytes))
        }
        Err(ureq::Error::StatusCode(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// A git symlink's raw content, when GitHub serves it unresolved, is just
/// its target path as plain UTF-8 text — recognized here by being short,
/// having no path separators (thumbnails are never symlinked across
/// directories) or newlines, and ending in a plausible image extension.
fn symlink_target(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > 256 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if text.contains('/') || text.contains('\n') || text.contains('\r') {
        return None;
    }
    let ext = Path::new(text).extension()?.to_str()?.to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg").then_some(text)
}

/// Maps GoodTools-style abbreviated region codes to the full No-Intro
/// region name libretro-thumbnails' own filenames use, for every `(...)`
/// tag on the ROM's original filename that looks like one.
fn region_candidates(original_stem: &str) -> Vec<&'static str> {
    parenthesized_tags(original_stem)
        .iter()
        .filter_map(|tag| region_full_name(tag))
        .collect()
}

fn region_full_name(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "U" => "USA",
        "E" => "Europe",
        "J" => "Japan",
        "W" => "World",
        "A" => "Australia",
        "B" => "Brazil",
        "C" => "China",
        "F" => "France",
        "G" => "Germany",
        "I" => "Italy",
        "K" => "Korea",
        "S" => "Spain",
        "Sw" => "Sweden",
        "Nl" => "Netherlands",
        _ => return None,
    })
}

fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_spaces_and_punctuation_for_a_url_path_segment() {
        assert_eq!(
            encode_path_segment("Super Mario World (USA)"),
            "Super%20Mario%20World%20%28USA%29"
        );
    }

    #[test]
    fn maps_goodtools_style_region_codes_to_no_intro_region_names() {
        assert_eq!(region_candidates("Super Mario World (U) [!]"), vec!["USA"]);
        assert_eq!(region_candidates("Sonic (E)"), vec!["Europe"]);
        assert_eq!(region_candidates("Some Game (USA)"), Vec::<&str>::new());
        assert_eq!(region_candidates("Plain Name"), Vec::<&str>::new());
    }

    /// The exact real response GitHub gave for a git-symlinked thumbnail:
    /// `Mario Party 2 (USA) (Wii Virtual Console).png` (a re-release
    /// symlinked to its original release's art) resolved to this 23-byte
    /// plain-text body instead of real image bytes.
    #[test]
    fn recognizes_a_real_git_symlink_target_as_such() {
        assert_eq!(
            symlink_target(b"Mario Party 2 (USA).png"),
            Some("Mario Party 2 (USA).png")
        );
    }

    #[test]
    fn a_real_image_body_is_never_mistaken_for_a_symlink_target() {
        // Too long, and doesn't end in a plausible image extension either.
        let mut fake_image_bytes = PNG_MAGIC.to_vec();
        fake_image_bytes.extend(std::iter::repeat(0u8).take(2000));
        assert_eq!(symlink_target(&fake_image_bytes), None);
    }

    #[test]
    fn a_path_with_a_directory_separator_is_not_treated_as_a_symlink_target() {
        // Thumbnails are never symlinked across directories in this
        // dataset — treat that as not a symlink stub, rather than
        // constructing a URL to somewhere unexpected.
        assert_eq!(symlink_target(b"../Other_System/Game.png"), None);
    }

    #[test]
    fn cached_cover_is_none_when_nothing_has_been_fetched_yet() {
        let dir = std::env::temp_dir().join(format!("wervik-artwork-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(cached_cover(&dir, "rom-doesnotexist"), None);
    }

    #[test]
    fn a_real_png_is_a_valid_cached_cover() {
        let dir = std::env::temp_dir().join(format!("wervik-artwork-valid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(cache_dir(&dir)).unwrap();
        let mut fake_png = PNG_MAGIC.to_vec();
        fake_png.extend_from_slice(b"not real pixel data, but a real header");
        std::fs::write(cache_path(&dir, "rom-real-cover"), &fake_png).unwrap();

        assert_eq!(
            cached_cover(&dir, "rom-real-cover"),
            Some(cache_path(&dir, "rom-real-cover"))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact real bug this guards against: a cache entry that's
    /// actually a git symlink's target path saved as text (see
    /// `download`'s doc comment) must not be treated as a permanently
    /// resolved cover — it gets removed so the next fetch pass tries
    /// again for real, instead of leaving 237 games (as found in one real
    /// library) stuck showing a broken image forever.
    #[test]
    fn a_cached_file_that_is_not_actually_a_png_is_treated_as_unresolved_and_removed() {
        let dir =
            std::env::temp_dir().join(format!("wervik-artwork-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(cache_dir(&dir)).unwrap();
        let bad_path = cache_path(&dir, "rom-corrupt-cover");
        std::fs::write(&bad_path, b"Mario Party 2 (USA).png").unwrap();

        assert_eq!(cached_cover(&dir, "rom-corrupt-cover"), None);
        assert!(
            !bad_path.exists(),
            "the corrupt cache file should be removed, not left behind"
        );
        assert!(
            !already_resolved(&dir, "rom-corrupt-cover"),
            "it must be retried, not treated as done"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_resolved_before_a_cover_or_a_miss_marker_exists() {
        let dir =
            std::env::temp_dir().join(format!("wervik-artwork-unresolved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!already_resolved(&dir, "rom-untouched"));
    }

    /// The bug this guards against: without a persisted "we already looked,
    /// there's nothing" marker, a game libretro-thumbnails simply has no art
    /// for would look "pending" again on every single app startup forever —
    /// re-fetched, missed, forgotten, re-fetched — since a miss produces no
    /// cover file to check for next time.
    #[test]
    fn a_confirmed_miss_counts_as_resolved_even_though_no_cover_was_ever_cached() {
        let dir = std::env::temp_dir().join(format!("wervik-artwork-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(cache_dir(&dir)).unwrap();
        std::fs::write(miss_marker_path(&dir, "rom-nomatch"), b"").unwrap();

        assert!(already_resolved(&dir, "rom-nomatch"));
        assert!(cached_cover(&dir, "rom-nomatch").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hits the real libretro-thumbnails host for a title that has existed
    /// there for years — the whole point of this feature is that it talks
    /// to a real, public source, so this is worth verifying for real rather
    /// than only against a mock.
    #[test]
    #[ignore = "requires network access"]
    fn fetches_a_real_known_cover_from_libretro_thumbnails() {
        let dir = std::env::temp_dir().join(format!("wervik-artwork-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rom = RomGame {
            id: "rom-test-mario".into(),
            name: "Super Mario World".into(),
            platform: Platform::Snes,
            rom_path: "/roms/Super Mario World (USA).sfc".into(),
            size_bytes: 0,
            modified_at: 0,
            variants: Vec::new(),
        };
        let agent = new_agent();
        let result = fetch_cover(&agent, &dir, &rom).unwrap();
        assert!(
            result.is_some(),
            "expected a real cover to be found for a well-known SNES title"
        );
        assert!(result.unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact real game that surfaced this bug: its libretro-thumbnails
    /// entry is a git symlink to the original release's art. Before the
    /// `download` fix, this cached a 23-byte text file (the symlink's raw
    /// target path) instead of a real cover.
    #[test]
    #[ignore = "requires network access"]
    fn follows_a_real_git_symlinked_thumbnail_to_its_actual_cover() {
        let dir =
            std::env::temp_dir().join(format!("wervik-artwork-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rom = RomGame {
            id: "rom-test-mario-party-2-vc".into(),
            name: "Mario Party 2".into(),
            platform: Platform::Nintendo64,
            rom_path: "/roms/Mario Party 2 (USA) (Wii Virtual Console).zip".into(),
            size_bytes: 0,
            modified_at: 0,
            variants: Vec::new(),
        };
        let agent = new_agent();
        let result = fetch_cover(&agent, &dir, &rom).unwrap();

        let path = result.expect("expected the symlink to resolve to a real cover");
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(&PNG_MAGIC),
            "expected real PNG bytes, not the symlink's raw target text"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact case that motivated `region_candidates`: an older
    /// GoodTools-style dump name ("(U)", "[!]") that doesn't appear
    /// verbatim or in Wervik's tag-stripped form in libretro-thumbnails,
    /// but does resolve once "(U)" is expanded to "(USA)".
    #[test]
    #[ignore = "requires network access"]
    fn fetches_a_cover_for_a_goodtools_style_dump_name() {
        let dir =
            std::env::temp_dir().join(format!("wervik-artwork-goodtools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let rom = RomGame {
            id: "rom-test-mario-goodtools".into(),
            name: "Super Mario World".into(),
            platform: Platform::Snes,
            rom_path: "/roms/Super Mario World (U) [!].smc".into(),
            size_bytes: 0,
            modified_at: 0,
            variants: Vec::new(),
        };
        let agent = new_agent();
        let result = fetch_cover(&agent, &dir, &rom).unwrap();
        assert!(
            result.is_some(),
            "expected the (U) -> (USA) region mapping to find a real cover"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
