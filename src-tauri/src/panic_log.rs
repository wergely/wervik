//! Writes every panic to a file in the app's data directory, in addition to
//! the normal stderr output.
//!
//! Without this, a panic in a background thread (a metadata/artwork sweep,
//! an achievements fetch, save sync, ...) leaves no trace at all: a GUI app
//! launched normally has no visible console, and — unlike an actual OS-level
//! crash (an access violation, a stack overflow) — a plain Rust panic
//! doesn't show up in the Windows Event Log either. If it happens on a
//! thread whose failure the user can *see* (e.g. the whole app appearing to
//! "just close"), there would otherwise be nothing to go on afterward.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Installs the panic hook. Call once, as early as possible (right after
/// `app_data_dir` is known) — a panic before this runs still goes to stderr
/// via Rust's own default hook, just without the file record.
pub fn install(app_data_dir: &Path) {
    let log_path = app_data_dir.join("wervik-panic.log");
    let log_path = Mutex::new(log_path);
    let previous_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        previous_hook(info);

        let thread_name = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        // Captured unconditionally (not gated on RUST_BACKTRACE, which a
        // normally-launched GUI app never has set) — this is the whole
        // point: finding out what actually happened without asking anyone
        // to run the app from a terminal with extra environment variables
        // set first.
        let backtrace = std::backtrace::Backtrace::force_capture();

        let path = log_path.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = write_panic(&path, &thread_name, &location, &message, &backtrace) {
            eprintln!("Could not write panic log to {}: {error}", path.display());
        }
    }));
}

fn write_panic(
    path: &PathBuf,
    thread_name: &str,
    location: &str,
    message: &str,
    backtrace: &std::backtrace::Backtrace,
) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    writeln!(
        file,
        "[{timestamp}] thread '{thread_name}' panicked at {location}:\n{message}\n{backtrace}\n"
    )
}
