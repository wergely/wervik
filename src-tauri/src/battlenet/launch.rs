//! Launches an installed Battle.net game through the Battle.net client.

#[cfg(windows)]
pub fn launch(launch_code: &str) -> Result<(), String> {
    use std::path::PathBuf;
    use std::process::Command;

    let mut candidates = Vec::new();
    for variable in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(root).join("Battle.net/Battle.net.exe"));
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local_app_data).join("Battle.net/Battle.net.exe"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        candidates.push(PathBuf::from(program_data).join("Battle.net/Battle.net.exe"));
    }

    let Some(executable) = candidates.into_iter().find(|path| path.is_file()) else {
        return Err("Battle.net.exe was not found. Open Battle.net once and rescan.".to_string());
    };

    // Battle.net expects the launch instruction as one argument, including
    // the quoted space between `launch` and the product code.
    let argument = format!(r#"--exec="launch {launch_code}""#);
    Command::new(executable)
        .arg(argument)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not launch Battle.net game: {e}"))
}

#[cfg(not(windows))]
pub fn launch(_launch_code: &str) -> Result<(), String> {
    Err("Battle.net games can only be launched on Windows.".to_string())
}
