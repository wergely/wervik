//! Small read-only integration with Nexus Mods.
//!
//! The trending feed is queried only when a game detail page is opened. The
//! result is cached locally so navigating away and back does not repeatedly
//! hit the public endpoint.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ureq::Agent;

const CACHE_MAX_AGE_SECONDS: i64 = 60 * 60 * 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusMod {
    pub name: String,
    pub author: Option<String>,
    pub summary: Option<String>,
    #[serde(alias = "picture_url")]
    pub picture_url: Option<String>,
    #[serde(alias = "mod_page_url")]
    pub mod_page_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NexusCacheEntry {
    mods: Vec<NexusMod>,
    fetched_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct NexusCacheFile {
    #[serde(default)]
    entries: HashMap<String, NexusCacheEntry>,
}

#[derive(Debug)]
pub struct NexusCache {
    path: PathBuf,
    entries: HashMap<String, NexusCacheEntry>,
}

impl NexusCache {
    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str::<NexusCacheFile>(&contents).ok())
            .map(|file| file.entries)
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            entries,
        }
    }

    pub fn get_fresh(&self, domain: &str) -> Option<Vec<NexusMod>> {
        let entry = self.entries.get(domain)?;
        if now_unix().saturating_sub(entry.fetched_at) < CACHE_MAX_AGE_SECONDS {
            Some(entry.mods.clone())
        } else {
            None
        }
    }

    pub fn get(&self, domain: &str) -> Option<Vec<NexusMod>> {
        self.entries.get(domain).map(|entry| entry.mods.clone())
    }

    pub fn put(&mut self, domain: String, mods: Vec<NexusMod>) -> Result<(), String> {
        self.entries.insert(
            domain,
            NexusCacheEntry {
                mods,
                fetched_at: now_unix(),
            },
        );
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&NexusCacheFile {
            entries: self.entries.clone(),
        })
        .map_err(|error| error.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&self.path, json).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct TrendingResponse {
    data: TrendingData,
}

#[derive(Debug, Deserialize)]
struct TrendingData {
    mods: Vec<NexusMod>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .into()
}

/// Resolves common installed game titles to the URL domain used by Nexus.
/// Nexus domains are not derivable reliably from an IGDB title because
/// editions can have separate pages, so unknown titles are left unmapped.
pub fn domain_for_game(name: &str) -> Option<&'static str> {
    let mut normalized: String = name
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect();
    if normalized.ends_with("windows") {
        normalized.truncate(normalized.len() - "windows".len());
    }

    match normalized.as_str() {
        "skyrim" | "theelderscrollsvskyrim" => Some("skyrim"),
        "skyrimspecialedition" | "theelderscrollsvskyrimspecialedition" => {
            Some("skyrimspecialedition")
        }
        "fallout3" => Some("fallout3"),
        "fallout4" => Some("fallout4"),
        "fallout76" => Some("fallout76"),
        "falloutnewvegas" => Some("falloutnewvegas"),
        "oblivion" | "theelderscrollsivoblivion" => Some("oblivion"),
        "morrowind" | "theelderscrollsiimorrowind" => Some("morrowind"),
        "cyberpunk2077" => Some("cyberpunk2077"),
        "eldenring" => Some("eldenring"),
        "baldursgate3" => Some("baldursgate3"),
        "stardewvalley" => Some("stardewvalley"),
        "starfield" => Some("starfield"),
        "thewitcher3wildhunt" | "witcher3" => Some("witcher3"),
        "hogwartslegacy" => Some("hogwartslegacy"),
        "monsterhunterworld" => Some("monsterhunterworld"),
        "monsterhunterrise" => Some("monsterhunterrise"),
        "masseffectlegendaryedition" => Some("masseffectlegendaryedition"),
        "dragonageorigins" => Some("dragonage"),
        "dragonageinquisition" => Some("dragonageinquisition"),
        "kingdomcomedeliverance" => Some("kingdomcomedeliverance"),
        "kingdomcomedeliverance2" => Some("kingdomcomedeliverance2"),
        "reddeadredemption2" => Some("reddeadredemption2"),
        "residentevilrequiem" => Some("residentevilrequiem"),
        "residentevil4" => Some("residentevil4"),
        "rimworld" => Some("rimworld"),
        _ => None,
    }
}

pub fn fetch_trending(domain: &str) -> Result<Vec<NexusMod>, String> {
    let url = format!("https://api.nexusmods.com/v3/games/{domain}/trending-mods");
    let mut response = new_agent()
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|error| format!("Nexus Mods request failed: {error}"))?;
    response
        .body_mut()
        .read_json::<TrendingResponse>()
        .map(|response| response.data.mods)
        .map_err(|error| format!("Could not read Nexus Mods response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_game_editions() {
        assert_eq!(
            domain_for_game("The Elder Scrolls V: Skyrim Special Edition"),
            Some("skyrimspecialedition")
        );
        assert_eq!(
            domain_for_game("The Witcher 3: Wild Hunt"),
            Some("witcher3")
        );
        assert_eq!(
            domain_for_game("Cyberpunk 2077 - Windows"),
            Some("cyberpunk2077")
        );
        assert_eq!(
            domain_for_game("Resident Evil Requiem - Windows"),
            Some("residentevilrequiem")
        );
    }

    #[test]
    fn leaves_unknown_titles_unmapped() {
        assert_eq!(domain_for_game("A Plague Tale: Requiem - Windows"), None);
    }

    #[test]
    fn parses_trending_response_fields() {
        let response: TrendingResponse = serde_json::from_str(
            r#"{"data":{"mods":[{"name":"Example Mod","author":"Author","summary":"A summary","picture_url":"https://example.com/mod.jpg","mod_page_url":"https://www.nexusmods.com/fallout4/mods/1"}]}}"#,
        )
        .unwrap();
        assert_eq!(response.data.mods[0].name, "Example Mod");
        assert_eq!(
            response.data.mods[0].picture_url.as_deref(),
            Some("https://example.com/mod.jpg")
        );
    }
}
