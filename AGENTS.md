# AGENTS.md

Guidance for coding agents working in this repository.

## What this is

Wervik is a Tauri 2 desktop app: a fast, controller-friendly Windows games
library that lists and launches installed Steam, Xbox App/Game Pass,
Battle.net, and retro (emulated) games, with optional IGDB metadata
enrichment. No account or cloud library — everything is read from the local
machine. See `README.md` for the full feature/user-facing description.

- Frontend: React 19 + TypeScript, plain CSS Modules (no Tailwind/UI kit),
  built with Vite. Entry point `src/main.tsx` → `src/App.tsx`.
- Backend: Rust, using Tauri's `invoke_handler` command pattern. Entry point
  `src-tauri/src/lib.rs` (`run()`), commands registered there and implemented
  in `src-tauri/src/commands.rs` (~1900 lines — the hub) plus per-source
  modules.
- The two sides only talk through Tauri commands: React calls typed wrappers
  in `src/api/tauri.ts`, which `invoke()` into the Rust command handlers.
  `src/types.ts` mirrors the Rust `serde` structs by hand — when you change a
  struct's shape on one side, update the other.

## Layout

```
src/                  React UI
  api/tauri.ts        Typed invoke() wrappers — the only place invoke() is called
  components/         One .tsx + matching .module.css per component
  hooks/               use* data-fetching/state hooks (library, artwork, settings, progress)
  navigation/          Controller/gamepad spatial navigation
  utils/               Pure helpers (e.g. recommendations.ts — has unit tests)
  types.ts             Shared TS types, hand-mirrored from Rust structs

src-tauri/src/
  lib.rs               App bootstrap: plugins, AppState setup, command registration
  commands.rs          Most #[tauri::command] handlers + AppState
  metadata.rs          IGDB metadata fetch/cache
  nexus.rs             Nexus Mods trending feed fetch/cache
  storage.rs           Settings/favorites persistence
  playtime.rs          SQLite-backed playtime tracking
  game_setup.rs         Per-game MangoHud/gamemode/gamescope wrapper setup
  mangohud.rs, sysutil.rs
  steam/, xbox/, battlenet/   Per-source discovery, launch, artwork
  retro/                ROM discovery, BIOS, disc handling, EmulatorJS server, native emulator sessions
  platform/             Cross-source platform abstraction

tests/                 Node's built-in test runner, run directly against .ts sources
```

## Building and running

Full app dev/build requires Windows (Steam/Xbox/Battle.net discovery is
Windows-specific) — see `README.md` for the Windows dev loop
(`npm run tauri dev`, `npm run tauri build`) and environment variables
(`STEAM_PATH`, `WERVIK_IGDB_CLIENT_ID`/`SECRET`).

On a non-Windows dev box (e.g. this environment) you can still:

- Type-check + build the frontend: `npm run build` (runs `tsc` then
  `vite build`).
- Run frontend unit tests: `node --test tests/*.test.mjs` (Node's native TS
  stripping runs the `.ts` imports directly — no test script is wired into
  `package.json` yet, no separate test runner/config to install).
- Check the Rust side compiles: `cd src-tauri && cargo check`. Full builds
  and anything touching `winreg`/Windows registry code paths won't run here.

There is no lint script configured. Match existing style in the file you're
editing.

## Conventions worth knowing

- Comments are rare and deliberate — they explain *why*, not *what* (see
  `lib.rs`'s comment about the `mangohud` directory name, or `types.ts`'s
  doc comments on non-obvious field semantics). Keep that bar when adding
  comments; don't add narrative ones.
- CSS is per-component CSS Modules (`Component.module.css`), not a shared
  design system file — check a sibling component for existing tokens/patterns
  before introducing new ones.
- `types.ts` fields commonly have doc comments spelling out *which source*
  a field applies to (e.g. "Present only when `source === \"steam\"\"") —
  preserve that pattern for new optional/union-shaped fields.
- Background work (artwork fetch, metadata fetch, Nexus fetch) runs as
  spawned tasks guarded by `Arc<AtomicBool>` "running" flags and reports
  progress via polled `*Progress` state + commands — follow this pattern
  rather than adding new event/channel mechanisms.
- Caches are flat JSON files or SQLite under the Tauri app data dir
  (`wervik-metadata.json`, `wervik-nexus.json`, `wervik-playtime.sqlite3`);
  settings/favorites go through `storage::Store`.

## Git

Repo is young (linear history). No CI config present. Commit messages are
short, imperative, capitalized (e.g. "Improve game source badges").
