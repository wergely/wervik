//! Read-only lookup of a single app's `LaunchOptions` value from
//! `localconfig.vdf`.
//!
//! Wervik never writes to this file: Steam is known to silently overwrite
//! externally-made changes to it while running, so — same as GOverlay —
//! any Launch Options change is left for the user to paste into Steam's own
//! Properties dialog, where Steam itself is the sole writer. This module
//! only ever needs to check what's currently there.

use std::path::Path;

use super::{discovery, vdf};

const PATH_TO_APPS: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

pub fn get_launch_options(steam_root: &Path, app_id: u32) -> Option<String> {
    let userdata_dir = discovery::find_active_userdata_dir(steam_root)?;
    let text = std::fs::read_to_string(userdata_dir.join("config/localconfig.vdf")).ok()?;
    let root = vdf::parse(&text).ok()?;
    let apps = root.path(&PATH_TO_APPS)?;
    let app = &apps
        .entries()
        .find(|(k, _)| k.parse::<u32>() == Ok(app_id))?
        .1;
    app.get_str("LaunchOptions").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_launch_options_for_the_right_app_among_siblings() {
        // Includes a decoy top-level "apps" block with the same app id
        // nested under a different key, mirroring a real localconfig.vdf,
        // to make sure the nested-path lookup doesn't get confused by it.
        let vdf = "\"UserLocalConfigStore\"\n{\n\t\"apps\"\n\t{\n\t\t\"949230\"\n\t\t{\n\t\t\t\"DefaultLaunchOption\"\n\t\t\t{\n\t\t\t\t\"ac9fe2d5\"\t\t\"0\"\n\t\t\t}\n\t\t}\n\t}\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"949230\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"gamemoderun %command%\"\n\t\t\t\t\t}\n\t\t\t\t\t\"750920\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

        let dir =
            std::env::temp_dir().join(format!("wervik-launch-opts-test-{}", std::process::id()));
        let userdata = dir.join("userdata/1/config");
        fs::create_dir_all(&userdata).unwrap();
        fs::write(userdata.join("localconfig.vdf"), vdf).unwrap();

        assert_eq!(
            get_launch_options(&dir, 949230),
            Some("gamemoderun %command%".to_string())
        );
        assert_eq!(get_launch_options(&dir, 750920), None);
        assert_eq!(get_launch_options(&dir, 99999), None);

        let _ = fs::remove_dir_all(&dir);
    }
}
