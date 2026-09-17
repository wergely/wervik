use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use ureq::Agent;
use url::Url;

use super::native::retroarch_runtime_dir;
use crate::commands::AppStateHandle;

const WERVIK_WEB_URL: &str = "https://wervik.wergely.com";
const MAX_SAVE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SCAN_DEPTH: usize = 4;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteSave {
    kind: String,
    file_name: String,
    sha256: String,
    byte_size: u64,
    modified_at: i64,
}

#[derive(Debug, Deserialize)]
struct RemoteSaveResponse {
    saves: Vec<RemoteSave>,
}

fn new_agent() -> Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .into()
}

fn web_url(path: &str, query: &[(&str, &str)]) -> Result<Url, String> {
    let configured = std::env::var("WERVIK_WEB_URL").unwrap_or_else(|_| WERVIK_WEB_URL.to_string());
    let mut url =
        Url::parse(&configured).map_err(|error| format!("Invalid Wervik web URL: {error}"))?;
    url.set_path(path);
    {
        let mut pairs = url.query_pairs_mut();
        pairs.clear();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    Ok(url)
}

fn save_directories(app_data_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let runtime_dir = retroarch_runtime_dir(app_data_dir);
    let saves_dir = runtime_dir.join("saves");
    let states_dir = runtime_dir.join("states");
    fs::create_dir_all(&saves_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&states_dir).map_err(|error| error.to_string())?;
    Ok((saves_dir, states_dir))
}

fn valid_file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value == value.trim()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn modified_at(path: &Path) -> Result<i64, String> {
    Ok(fs::metadata(path)
        .map_err(|error| error.to_string())?
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64)
}

fn content_stem(rom_path: &Path) -> Result<String, String> {
    rom_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Could not determine the RetroArch save name for this ROM.".to_string())
}

fn collect_files(
    directory: &Path,
    stem: &str,
    depth: usize,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > MAX_SCAN_DEPTH {
        return Ok(());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let prefix = format!("{}.", stem.to_ascii_lowercase());
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            collect_files(&path, stem, depth + 1, files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.to_ascii_lowercase().starts_with(&prefix) && valid_file_name(file_name) {
            files.push(path);
        }
    }
    Ok(())
}

fn read_save(path: &Path) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size == 0 || size > MAX_SAVE_BYTES {
        return Err(format!(
            "RetroArch save is outside the supported size limit: {}",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| error.to_string())
}

fn remote_saves(
    agent: &Agent,
    token: &str,
    source_game_id: &str,
    platform_key: &str,
) -> Result<Vec<RemoteSave>, String> {
    let url = web_url(
        "/api/sync/retro-saves",
        &[
            ("sourceGameId", source_game_id),
            ("platformKey", platform_key),
        ],
    )?;
    let mut response = agent
        .get(url.as_str())
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|error| format!("Wervik save manifest fetch failed: {error}"))?;
    response
        .body_mut()
        .read_json::<RemoteSaveResponse>()
        .map(|body| body.saves)
        .map_err(|error| format!("Could not read Wervik save manifest: {error}"))
}

fn download_save(
    agent: &Agent,
    token: &str,
    source_game_id: &str,
    platform_key: &str,
    remote: &RemoteSave,
) -> Result<Vec<u8>, String> {
    let url = web_url(
        "/api/sync/retro-saves/file",
        &[
            ("sourceGameId", source_game_id),
            ("platformKey", platform_key),
            ("kind", &remote.kind),
            ("fileName", &remote.file_name),
        ],
    )?;
    let mut response = agent
        .get(url.as_str())
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|error| format!("Wervik save download failed: {error}"))?;
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(|error| format!("Could not read Wervik save download: {error}"))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_SAVE_BYTES {
        return Err("Wervik returned a save outside the supported size limit.".to_string());
    }
    if sha256_hex(&bytes) != remote.sha256 {
        return Err(format!(
            "Wervik save hash mismatch for {}.",
            remote.file_name
        ));
    }
    Ok(bytes)
}

fn restore_file(
    agent: &Agent,
    token: &str,
    source_game_id: &str,
    platform_key: &str,
    directory: &Path,
    remote: &RemoteSave,
) -> Result<bool, String> {
    if !valid_file_name(&remote.file_name)
        || !matches!(remote.kind.as_str(), "save" | "state")
        || remote.byte_size == 0
        || remote.byte_size > MAX_SAVE_BYTES
        || remote.sha256.len() != 64
    {
        return Ok(false);
    }
    let destination = directory.join(&remote.file_name);
    if let Ok(local_bytes) = read_save(&destination) {
        if sha256_hex(&local_bytes) == remote.sha256 {
            return Ok(false);
        }
    }
    if destination.is_file() && modified_at(&destination)? > remote.modified_at {
        return Ok(false);
    }

    let bytes = download_save(agent, token, source_game_id, platform_key, remote)?;
    if bytes.len() as u64 != remote.byte_size {
        return Err(format!(
            "Wervik save size mismatch for {}.",
            remote.file_name
        ));
    }
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    fs::write(destination, bytes).map_err(|error| error.to_string())?;
    Ok(true)
}

/// Restores the saves belonging to a ROM before RetroArch starts it. A local
/// file with newer contents wins, so a stale machine cannot overwrite a more
/// recent save from another installation.
pub fn restore_game(
    app_data_dir: &Path,
    token: &str,
    source_game_id: &str,
    platform_key: &str,
    rom_path: &Path,
) -> Result<usize, String> {
    if token.trim().is_empty() {
        return Ok(0);
    }
    let stem = content_stem(rom_path)?;
    let (saves_dir, states_dir) = save_directories(app_data_dir)?;
    let agent = new_agent();
    let remote = remote_saves(&agent, token, source_game_id, platform_key)?;
    let mut restored = 0;
    for save in remote {
        let directory = match save.kind.as_str() {
            "save" => &saves_dir,
            "state" => &states_dir,
            _ => continue,
        };
        if !save
            .file_name
            .to_ascii_lowercase()
            .starts_with(&format!("{}.", stem.to_ascii_lowercase()))
        {
            continue;
        }
        if restore_file(
            &agent,
            token,
            source_game_id,
            platform_key,
            directory,
            &save,
        )? {
            restored += 1;
        }
    }
    Ok(restored)
}

fn upload_file(
    agent: &Agent,
    token: &str,
    source_game_id: &str,
    platform_key: &str,
    kind: &str,
    path: &Path,
) -> Result<bool, String> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if !valid_file_name(file_name) {
        return Ok(false);
    }
    let bytes = read_save(path)?;
    let digest = sha256_hex(&bytes);
    let modified = modified_at(path)?;
    let url = web_url("/api/sync/retro-saves", &[])?;
    agent
        .put(url.as_str())
        .header("Authorization", &format!("Bearer {token}"))
        .header("X-Wervik-Source-Game-Id", source_game_id)
        .header("X-Wervik-Platform-Key", platform_key)
        .header("X-Wervik-Save-Kind", kind)
        .header("X-Wervik-File-Name", file_name)
        .header("X-Wervik-Sha256", &digest)
        .header("X-Wervik-Modified-At", &modified.to_string())
        .header("Content-Type", "application/octet-stream")
        .send(&bytes)
        .map_err(|error| format!("Wervik save upload failed: {error}"))?;
    Ok(true)
}

/// Uploads the saves RetroArch left behind after the emulator exits. The
/// account is read at upload time so reconnecting during gameplay cannot make
/// the exit hook reuse the previous account's token.
pub fn sync_game_from_runtime(
    app_handle: &AppHandle,
    app_data_dir: &Path,
    source_game_id: &str,
    platform_key: &str,
    rom_path: &Path,
) -> Result<usize, String> {
    let token = app_handle
        .try_state::<AppStateHandle>()
        .and_then(|state| {
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .store
                .wervik_account()
        })
        .map(|account| account.access_token)
        .filter(|token| !token.trim().is_empty());
    let Some(token) = token else {
        return Ok(0);
    };

    let stem = content_stem(rom_path)?;
    let (saves_dir, states_dir) = save_directories(app_data_dir)?;
    let mut files = Vec::new();
    collect_files(&saves_dir, &stem, 0, &mut files)?;
    let mut state_files = Vec::new();
    collect_files(&states_dir, &stem, 0, &mut state_files)?;
    let agent = new_agent();
    let mut uploaded = 0;
    for path in files {
        if upload_file(&agent, &token, source_game_id, platform_key, "save", &path)? {
            uploaded += 1;
        }
    }
    for path in state_files {
        if upload_file(&agent, &token, source_game_id, platform_key, "state", &path)? {
            uploaded += 1;
        }
    }
    Ok(uploaded)
}
