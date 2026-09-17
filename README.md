# Wervik

Your games. One library.

Wervik is a fast, controller-friendly desktop library for browsing
and launching your installed Steam and retro games on macOS and Windows. On
Windows it also supports Xbox App/Game Pass, Battle.net, and EA App. It
reads local library data directly and can optionally enrich the selected game's
details through IGDB. Connecting a Wervik account optionally syncs the local
library, playtime and cached achievement progress to the web app; the desktop
library remains usable without an account.

Wervik — by Wergely.

## Development

```sh
npm install
npm run tauri dev
```

On macOS, Wervik scans the standard Steam location at
`~/Library/Application Support/Steam` and launches games through Steam's
`steam://` protocol. Xbox App/Game Pass, Battle.net, and EA App are not offered
there, and MangoHud/GameMode/Gamescope are Windows/Linux-only integrations.
Retro platforms use native emulators; on macOS the Platform Setup screen can
download the official universal RetroArch app plus architecture-matched
Libretro cores for native launching.

## Testing on Windows

Copy the project to a normal Windows folder such as `C:\dev\wervik` before
running it. From PowerShell, with Steam installed and open:

```powershell
cd C:\dev\wervik
npm ci
npm run tauri dev
```

The first run should find Steam from the Windows registry, locally installed
Xbox App/Game Pass titles from Windows app registration, and installed EA App
games from the Windows registry. Test launching a Steam game, an Xbox game,
a Battle.net game and an EA App game, opening their install folders
and the Steam store page where applicable, then add a ROM folder in Settings,
install the platform's native emulator from Platforms, and launch a retro game.
If Steam is installed in a portable or non-standard location, start PowerShell
with an override before launching Wervik:

```powershell
$env:STEAM_PATH = "D:\Games\Steam"
npm run tauri dev
```

To enable IGDB metadata while developing, set the credentials from your Twitch
Developer application in the same PowerShell session before starting Tauri:

```powershell
$env:WERVIK_IGDB_CLIENT_ID = "your-client-id"
$env:WERVIK_IGDB_CLIENT_SECRET = "your-client-secret"
npm run tauri dev
```

The OAuth redirect URL used when registering the Twitch application can be
`http://localhost`; IGDB uses the client-credentials flow and does not use that
redirect. Metadata and downloaded IGDB artwork are cached in
`wervik-metadata.json` and `igdb-artwork` under Wervik's application data.

When a supported PC game is opened, Wervik also loads its trending Nexus Mods
feed on demand. This does not run during library startup; the result is cached
in `wervik-nexus.json` under the same application data directory. The first
version recognizes common Nexus game domains and quietly skips titles without
a matching Nexus page.

## Building

```powershell
npm run tauri build
```

On macOS, `npm run tauri build` produces an `.app` and `.dmg`. The release
workflow builds Windows installers plus macOS artifacts for Apple Silicon and
Intel.

## Recommended IDE Setup

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with Desktop development with C++
- [Node.js LTS](https://nodejs.org/)
- [Rust MSVC](https://www.rust-lang.org/tools/install)
- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

The Windows build uses the Microsoft Edge WebView2 runtime. Windows 10
1803 and later already include it; the NSIS installer is configured to
download it when necessary.

The Windows baseline uses the official Windows portable RetroArch package and
the selected Libretro core for console and arcade games. Wervik downloads both
into its own application data directory without requiring an elevated Windows
install. The macOS build uses
the official universal RetroArch Metal app and downloads matching `.dylib`
cores for the current Apple Silicon or Intel architecture.
FBNeo is used for CPS and Neo Geo arcade ZIP sets; NeoCD is the recommended
Neo Geo CD core, with Geolith and FBNeo available as alternatives. Geolith
requires a complete MAME BIOS pack. No ROM or BIOS files are downloaded.

Arcade ZIP sets are discovered when they are inside a directory named
`arcade`, `cps`, `cps2`, `cps3`, `neogeo` or `fbneo`, including nested
directories. Neo Geo CD images continue to use the existing `.cue`/`.chd`
disc detection.

Wervik keeps local playtime sessions in `wervik-playtime.sqlite3` under its
application data directory. Existing retro and Steam playtime is imported as
an initial baseline; new launches are tracked by monitoring the installed game
process and are retained locally across restarts.

When a Wervik account is connected, games launched through the managed
RetroArch cores also synchronize their normal saves and save states after the
emulator exits, and restore them before the next launch. The files stay in
RetroArch's managed `saves` and `states` directories; a local file newer than
the cloud copy is kept. The same ROM filename and size are required on the
other installation for the save to be associated with the game.

## Credits

- Platform/console icons: [KyleBing/retro-game-console-icons](https://github.com/KyleBing/retro-game-console-icons)
  (GPL-3.0).
- Steam/Battle.net/EA brand icons: [Simple Icons](https://simpleicons.org/) (CC0-1.0).
