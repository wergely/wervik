//! Launches an Xbox App / PC Game Pass title through Windows AppUserModelID.

#[cfg(windows)]
pub fn launch(aumid: &str) -> Result<(), String> {
    use std::process::Command;

    let shell_target = format!("shell:AppsFolder\\{aumid}");
    Command::new("explorer.exe")
        .arg(shell_target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not launch Xbox game: {e}"))
}

#[cfg(not(windows))]
pub fn launch(_aumid: &str) -> Result<(), String> {
    Err("Xbox games can only be launched on Windows.".to_string())
}
