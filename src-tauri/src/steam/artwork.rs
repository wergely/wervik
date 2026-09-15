//! Resolves locally cached Steam artwork for a given app id.
//!
//! Steam caches images under `<steam_root>/appcache/librarycache`. Older
//! clients store them as flat files named `<appid>_<kind>.<ext>`; current
//! clients store them under a per-app directory, one level deeper, inside a
//! content-hashed subdirectory whose name we can't predict — so we search
//! for the known filename instead of guessing the hash.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    /// Portrait grid cover (600x900), used for library cards.
    pub cover: Option<PathBuf>,
    /// Wide hero/banner image, used as the detail-view background.
    pub hero: Option<PathBuf>,
    /// Transparent logo, optionally overlaid on the hero image.
    pub logo: Option<PathBuf>,
}

pub fn resolve(librarycache_dir: &Path, app_id: u32) -> Artwork {
    Artwork {
        cover: find_variant(librarycache_dir, app_id, "library_600x900.jpg")
            .or_else(|| find_variant(librarycache_dir, app_id, "library_600x900.png"))
            // Some titles only ever got the newer, smaller vertical capsule
            // instead of a dedicated library grid image; same 2:3 aspect.
            .or_else(|| find_variant(librarycache_dir, app_id, "library_capsule.jpg"))
            .or_else(|| find_variant(librarycache_dir, app_id, "library_capsule.png")),
        hero: find_variant(librarycache_dir, app_id, "library_hero.jpg")
            .or_else(|| find_variant(librarycache_dir, app_id, "library_hero.png")),
        logo: find_variant(librarycache_dir, app_id, "logo.png"),
    }
}

fn find_variant(librarycache_dir: &Path, app_id: u32, filename: &str) -> Option<PathBuf> {
    // Legacy flat layout: librarycache/<appid>_<filename>
    let flat = librarycache_dir.join(format!("{app_id}_{filename}"));
    if flat.is_file() {
        return Some(flat);
    }

    let app_dir = librarycache_dir.join(app_id.to_string());

    // Common layout: librarycache/<appid>/<filename>
    let direct = app_dir.join(filename);
    if direct.is_file() {
        return Some(direct);
    }

    // Older/alternate layout: librarycache/<appid>/<hash>/<filename>
    if let Ok(entries) = std::fs::read_dir(&app_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join(filename);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}
