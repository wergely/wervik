//! Per-game MangoHud overlay configuration.
//!
//! This module owns MangoHud's own config file format and its "should the
//! wrapper activate MangoHud" marker. The wrapper script itself (which also
//! composes GameMode and custom environment variables) lives in
//! `game_setup.rs`, since it isn't MangoHud-specific.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sysutil;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MangoHudConfig {
    pub fps: bool,
    pub frame_timing: bool,
    pub gpu_stats: bool,
    pub cpu_stats: bool,
    pub ram: bool,
    pub vram: bool,
    pub position: String,
    pub fps_limit: u32,
    pub toggle_hud: String,
    /// MangoHud's own `no_display` flag: keep the library active (so
    /// `fps_limit` still enforces a cap) without drawing the overlay.
    pub no_display: bool,
}

impl Default for MangoHudConfig {
    fn default() -> Self {
        Self {
            fps: true,
            frame_timing: true,
            gpu_stats: true,
            cpu_stats: true,
            ram: false,
            vram: false,
            position: "top-left".into(),
            fps_limit: 0,
            toggle_hud: "Shift_R+F12".into(),
            no_display: false,
        }
    }
}

fn app_dir(mangohud_dir: &Path, key: &str) -> PathBuf {
    mangohud_dir.join(key)
}

pub fn config_path(mangohud_dir: &Path, key: &str) -> PathBuf {
    app_dir(mangohud_dir, key).join("config.conf")
}

pub(crate) fn enabled_marker_path(mangohud_dir: &Path, key: &str) -> PathBuf {
    app_dir(mangohud_dir, key).join("enabled")
}

pub fn is_enabled(mangohud_dir: &Path, key: &str) -> bool {
    enabled_marker_path(mangohud_dir, key).is_file()
}

pub fn set_enabled(mangohud_dir: &Path, key: &str, enabled: bool) -> Result<(), String> {
    let marker = enabled_marker_path(mangohud_dir, key);
    if enabled {
        std::fs::create_dir_all(app_dir(mangohud_dir, key)).map_err(|e| e.to_string())?;
        std::fs::write(&marker, b"").map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&marker) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn read_config(mangohud_dir: &Path, key: &str) -> MangoHudConfig {
    std::fs::read_to_string(config_path(mangohud_dir, key))
        .map(|text| from_conf_text(&text))
        .unwrap_or_default()
}

pub fn write_config(mangohud_dir: &Path, key: &str, config: &MangoHudConfig) -> Result<(), String> {
    std::fs::create_dir_all(app_dir(mangohud_dir, key)).map_err(|e| e.to_string())?;
    std::fs::write(config_path(mangohud_dir, key), to_conf_text(config)).map_err(|e| e.to_string())
}

fn to_conf_text(cfg: &MangoHudConfig) -> String {
    let mut lines: Vec<String> = Vec::new();
    if cfg.fps {
        lines.push("fps".into());
    }
    if cfg.frame_timing {
        lines.push("frame_timing".into());
    }
    if cfg.gpu_stats {
        lines.push("gpu_stats".into());
    }
    if cfg.cpu_stats {
        lines.push("cpu_stats".into());
    }
    if cfg.ram {
        lines.push("ram".into());
    }
    if cfg.vram {
        lines.push("vram".into());
    }
    if cfg.no_display {
        lines.push("no_display".into());
    }
    lines.push(format!("position={}", cfg.position));
    lines.push(format!("fps_limit={}", cfg.fps_limit));
    lines.push(format!("toggle_hud={}", cfg.toggle_hud));
    lines.join("\n") + "\n"
}

fn from_conf_text(text: &str) -> MangoHudConfig {
    let mut cfg = MangoHudConfig {
        fps: false,
        frame_timing: false,
        gpu_stats: false,
        cpu_stats: false,
        ram: false,
        vram: false,
        no_display: false,
        ..MangoHudConfig::default()
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "position" => cfg.position = value.trim().to_string(),
                "fps_limit" => cfg.fps_limit = value.trim().parse().unwrap_or(0),
                "toggle_hud" => cfg.toggle_hud = value.trim().to_string(),
                _ => {}
            }
        } else {
            match line {
                "fps" => cfg.fps = true,
                "frame_timing" => cfg.frame_timing = true,
                "gpu_stats" => cfg.gpu_stats = true,
                "cpu_stats" => cfg.cpu_stats = true,
                "ram" => cfg.ram = true,
                "vram" => cfg.vram = true,
                "no_display" => cfg.no_display = true,
                _ => {}
            }
        }
    }
    cfg
}

pub fn is_installed() -> bool {
    sysutil::binary_installed("mangohud")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_round_trips_through_text_format() {
        let mut cfg = MangoHudConfig::default();
        cfg.gpu_stats = false;
        cfg.position = "bottom-right".into();
        cfg.fps_limit = 60;
        cfg.no_display = true;

        let text = to_conf_text(&cfg);
        let parsed = from_conf_text(&text);

        assert!(parsed.fps);
        assert!(!parsed.gpu_stats);
        assert_eq!(parsed.position, "bottom-right");
        assert_eq!(parsed.fps_limit, 60);
        assert!(parsed.no_display);
    }
}
