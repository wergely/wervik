//! Wervik never ships or downloads BIOS files — some retro platforms
//! (notably PlayStation) need one to boot at all, so the user points at
//! their own dump and Wervik copies it into its own app data directory,
//! next to everything else it manages.

use std::path::{Path, PathBuf};

fn bios_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("bios")
}

/// Copies `source_path` into Wervik's BIOS directory and returns the new
/// path. Copies rather than moves — the user's original dump is left
/// untouched wherever they keep it.
pub fn install(app_data_dir: &Path, source_path: &Path) -> Result<PathBuf, String> {
    let file_name = source_path
        .file_name()
        .ok_or_else(|| "Selected BIOS path has no file name".to_string())?;
    let dir = bios_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(file_name);
    std::fs::copy(source_path, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}
