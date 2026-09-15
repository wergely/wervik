//! Wervik's local playtime database and process-backed session tracker.
//!
//! Provider metadata is useful as an initial baseline, but it is not
//! available consistently across Steam, Xbox App, Battle.net, and retro
//! sessions. This module keeps a small SQLite database with one row per
//! launch and an aggregate table for fast library sorting.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use sysinfo::{Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppStateHandle;

const STARTUP_GRACE: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct PlaytimeSummary {
    pub total_seconds: u64,
    pub last_played: Option<i64>,
}

impl PlaytimeSummary {
    pub fn total_minutes(&self) -> u64 {
        if self.total_seconds == 0 {
            0
        } else {
            self.total_seconds.div_ceil(60)
        }
    }
}

#[derive(Debug)]
pub struct FinishedSession {
    pub game_id: String,
    pub source: String,
    pub duration_seconds: u64,
    pub ended_at: i64,
}

pub struct PlaytimeDb {
    connection: Connection,
}

impl PlaytimeDb {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;

                CREATE TABLE IF NOT EXISTS playtime_totals (
                    game_id TEXT PRIMARY KEY NOT NULL,
                    total_seconds INTEGER NOT NULL DEFAULT 0,
                    last_played INTEGER
                );

                CREATE TABLE IF NOT EXISTS playtime_sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    game_id TEXT NOT NULL,
                    source TEXT NOT NULL,
                    started_at INTEGER NOT NULL,
                    last_seen_at INTEGER NOT NULL,
                    ended_at INTEGER,
                    duration_seconds INTEGER
                );

                CREATE INDEX IF NOT EXISTS idx_playtime_sessions_game
                    ON playtime_sessions(game_id, started_at);
                CREATE INDEX IF NOT EXISTS idx_playtime_sessions_active
                    ON playtime_sessions(ended_at);
                ",
            )
            .map_err(|e| e.to_string())?;

        Ok(Self { connection })
    }

    /// Imports an existing provider value only once. This gives the internal
    /// database a useful starting point without adding the same Steam value on
    /// every scan.
    pub fn seed_if_missing(
        &self,
        game_id: &str,
        total_seconds: u64,
        last_played: Option<i64>,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO playtime_totals(game_id, total_seconds, last_played)
                 VALUES (?1, ?2, ?3)",
                params![game_id, total_seconds as i64, last_played],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn summary(&self, game_id: &str) -> Result<Option<PlaytimeSummary>, String> {
        self.connection
            .query_row(
                "SELECT total_seconds, last_played FROM playtime_totals WHERE game_id = ?1",
                params![game_id],
                |row| {
                    Ok(PlaytimeSummary {
                        total_seconds: row.get::<_, i64>(0)?.max(0) as u64,
                        last_played: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Starts a session and updates `last_played` immediately. The aggregate
    /// total is changed only when the session is completed.
    pub fn start_session(
        &self,
        game_id: &str,
        source: &str,
        started_at: i64,
    ) -> Result<i64, String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO playtime_totals(game_id, total_seconds, last_played)
                 VALUES (?1, 0, ?2)
                 ON CONFLICT(game_id) DO UPDATE SET last_played = excluded.last_played",
                params![game_id, started_at],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO playtime_sessions(game_id, source, started_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?3)",
                params![game_id, source, started_at],
            )
            .map_err(|e| e.to_string())?;
        let id = transaction.last_insert_rowid();
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }

    pub fn heartbeat(&self, session_id: i64, seen_at: i64) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE playtime_sessions
                 SET last_seen_at = CASE WHEN last_seen_at < ?2 THEN ?2 ELSE last_seen_at END
                 WHERE id = ?1 AND ended_at IS NULL",
                params![session_id, seen_at],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn finish_session(
        &self,
        session_id: i64,
        ended_at: i64,
    ) -> Result<Option<FinishedSession>, String> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let Some((game_id, source, started_at)) = transaction
            .query_row(
                "SELECT game_id, source, started_at FROM playtime_sessions
                 WHERE id = ?1 AND ended_at IS NULL",
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())?
        else {
            transaction.commit().map_err(|e| e.to_string())?;
            return Ok(None);
        };

        let ended_at = ended_at.max(started_at);
        let duration_seconds = ended_at.saturating_sub(started_at) as u64;
        transaction
            .execute(
                "UPDATE playtime_sessions
                 SET ended_at = ?2, duration_seconds = ?3, last_seen_at = ?2
                 WHERE id = ?1 AND ended_at IS NULL",
                params![session_id, ended_at, duration_seconds as i64],
            )
            .map_err(|e| e.to_string())?;
        transaction
            .execute(
                "INSERT INTO playtime_totals(game_id, total_seconds, last_played)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(game_id) DO UPDATE SET
                   total_seconds = playtime_totals.total_seconds + excluded.total_seconds,
                   last_played = excluded.last_played",
                params![game_id, duration_seconds as i64, ended_at],
            )
            .map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;

        Ok(Some(FinishedSession {
            game_id,
            source,
            duration_seconds,
            ended_at,
        }))
    }

    /// A session that never found its game process should not add phantom
    /// minutes, but it is still closed so it cannot be recovered forever.
    pub fn cancel_session(&self, session_id: i64, ended_at: i64) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE playtime_sessions
                 SET ended_at = ?2, duration_seconds = 0, last_seen_at = ?2
                 WHERE id = ?1 AND ended_at IS NULL",
                params![session_id, ended_at],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Closes sessions left active by a previous Wervik crash. The last
    /// heartbeat is used as the end time so the app does not count time while
    /// it was already offline.
    pub fn recover_interrupted_sessions(&self) -> Result<(), String> {
        let ids: Vec<(i64, i64)> = {
            let mut statement = self
                .connection
                .prepare("SELECT id, last_seen_at FROM playtime_sessions WHERE ended_at IS NULL")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (id, last_seen_at) in ids {
            let _ = self.finish_session(id, last_seen_at)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProcessTarget {
    root: PathBuf,
    executable_names: HashSet<String>,
}

impl ProcessTarget {
    pub fn from_install_dir(root: &Path) -> Self {
        let mut executable_names = HashSet::new();
        collect_executable_names(root, 2, &mut executable_names);
        Self {
            root: root.to_path_buf(),
            executable_names,
        }
    }
}

fn collect_executable_names(dir: &Path, depth: u8, names: &mut HashSet<String>) {
    if names.len() >= 128 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_file() {
            let is_executable = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
            if is_executable {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    let lowered = name.to_ascii_lowercase();
                    if !lowered.starts_with("unins")
                        && !lowered.contains("crash")
                        && !lowered.contains("report")
                        && !lowered.contains("redist")
                        && !lowered.contains("setup")
                        && !lowered.contains("install")
                    {
                        names.insert(lowered);
                    }
                }
            }
        } else if depth > 0 && path.is_dir() {
            collect_executable_names(&path, depth - 1, names);
        }
        if names.len() >= 128 {
            return;
        }
    }
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn process_is_in_target(process: &Process, target: &ProcessTarget) -> bool {
    if let Some(executable) = process.exe() {
        let executable = normalized_path(executable);
        let root = normalized_path(&target.root);
        if executable == root || executable.starts_with(&(root.clone() + "\\")) {
            return true;
        }
        // We have a concrete executable path and it belongs elsewhere. Do
        // not fall back to a same-named process from another installation.
        return false;
    }

    // Some protected Windows processes do not expose their executable path;
    // only those processes use the conservative filename fallback.
    let process_name = process.name().to_string_lossy().to_ascii_lowercase();
    target.executable_names.contains(&process_name)
}

fn game_process_is_running(system: &System, target: &ProcessTarget) -> bool {
    system
        .processes()
        .values()
        .any(|process| process_is_in_target(process, target))
}

/// `game_id` is only used to tell the frontend which game's "Playing"
/// status just ended (see the `playtime-updated` emit below and in
/// `commands::finish_playtime_session`) — the session itself is tracked by
/// `session_id` alone, same as before.
pub fn spawn_process_monitor(app_handle: AppHandle, session_id: i64, game_id: String, target: ProcessTarget) {
    std::thread::spawn(move || {
        let mut system = System::new();
        let deadline = Instant::now() + STARTUP_GRACE;
        let mut seen_process = false;
        let mut missing_polls = 0u8;

        loop {
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
            );
            let running = game_process_is_running(&system, &target);

            if running {
                seen_process = true;
                missing_polls = 0;
                if !heartbeat_session(&app_handle, session_id) {
                    return;
                }
            } else if seen_process {
                missing_polls = missing_polls.saturating_add(1);
                if missing_polls >= 2 {
                    finish_session(&app_handle, session_id);
                    return;
                }
            } else if Instant::now() >= deadline {
                cancel_session(&app_handle, session_id);
                // The process never actually appeared (finish_session, which
                // emits this same event with the real game id, never runs
                // for a cancelled session) — the frontend still needs to
                // know to stop showing this game as "Playing".
                let _ = app_handle.emit("playtime-updated", &game_id);
                return;
            }

            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn heartbeat_session(app_handle: &AppHandle, session_id: i64) -> bool {
    let Some(state) = app_handle.try_state::<AppStateHandle>() else {
        return false;
    };
    let state = state.lock().unwrap_or_else(|e| e.into_inner());
    state
        .playtime
        .heartbeat(session_id, crate::commands::now_unix())
        .is_ok()
}

fn finish_session(app_handle: &AppHandle, session_id: i64) {
    crate::commands::finish_playtime_session(app_handle, session_id);
}

fn cancel_session(app_handle: &AppHandle, session_id: i64) {
    if let Some(state) = app_handle.try_state::<AppStateHandle>() {
        let state = state.lock().unwrap_or_else(|e| e.into_inner());
        let _ = state
            .playtime
            .cancel_session(session_id, crate::commands::now_unix());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(name: &str) -> (PlaytimeDb, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "wervik-playtime-{name}-{}.sqlite3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        (PlaytimeDb::open(&path).unwrap(), path)
    }

    #[test]
    fn completed_sessions_accumulate_and_update_last_played() {
        let (db, path) = database("accumulate");
        let first = db.start_session("steam:10", "steam", 100).unwrap();
        db.heartbeat(first, 130).unwrap();
        let finished = db.finish_session(first, 160).unwrap().unwrap();
        assert_eq!(finished.duration_seconds, 60);

        let second = db.start_session("steam:10", "steam", 200).unwrap();
        db.finish_session(second, 245).unwrap();

        let summary = db.summary("steam:10").unwrap().unwrap();
        assert_eq!(summary.total_seconds, 105);
        assert_eq!(summary.last_played, Some(245));
        assert_eq!(summary.total_minutes(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn recovery_uses_last_heartbeat_instead_of_current_time() {
        let (db, path) = database("recovery");
        let session = db.start_session("retro:10", "retro", 100).unwrap();
        db.heartbeat(session, 125).unwrap();
        db.recover_interrupted_sessions().unwrap();

        let summary = db.summary("retro:10").unwrap().unwrap();
        assert_eq!(summary.total_seconds, 25);
        assert_eq!(summary.last_played, Some(125));
        let _ = std::fs::remove_file(path);
    }
}
