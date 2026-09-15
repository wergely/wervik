import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  addRomDirectory,
  applyRetroArchAchievements,
  getRomDirectories,
  refreshAllMetadata,
  removeRomDirectory,
  rescanRetroLibrary,
} from "../api/tauri";
import { useMetadataProgress } from "../hooks/useMetadataProgress";
import type { LibrarySnapshot, Settings as SettingsType } from "../types";
import { Spinner } from "./Spinner";
import { Switch } from "./Switch";
import styles from "./Settings.module.css";

interface SettingsProps {
  snapshot: LibrarySnapshot | null;
  settings: SettingsType | null;
  rescanning: boolean;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
  onRescan: () => void;
  onLibraryChange: (snapshot: LibrarySnapshot) => void;
}

const SCALE_STEPS = [0.9, 1, 1.1, 1.25];

type Category =
  | "steam"
  | "battlenet"
  | "ea"
  | "xbox"
  | "retro"
  | "igdb"
  | "retroachievements"
  | "interface"
  | "audio";

const CATEGORIES: { id: Category; label: string }[] = [
  { id: "steam", label: "Steam" },
  { id: "battlenet", label: "Battle.net" },
  { id: "ea", label: "EA App" },
  { id: "xbox", label: "Xbox App & PC Game Pass" },
  { id: "retro", label: "Retro & Emulation" },
  { id: "igdb", label: "IGDB" },
  { id: "retroachievements", label: "RetroAchievements" },
  { id: "interface", label: "Interface" },
  { id: "audio", label: "Audio" },
];

/** Client ID/Secret are edited locally and only pushed to `onUpdateSettings`
 * on Save — unlike every other setting here, saving on each keystroke would
 * both write to disk constantly and (see `commands::set_settings`) fire off
 * an IGDB auth attempt with a half-typed credential on every character. */
function IgdbFields({
  settings,
  onUpdateSettings,
}: {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}) {
  const [clientId, setClientId] = useState(settings?.igdbClientId ?? "");
  const [clientSecret, setClientSecret] = useState(settings?.igdbClientSecret ?? "");
  const [revealSecret, setRevealSecret] = useState(false);

  useEffect(() => {
    setClientId(settings?.igdbClientId ?? "");
    setClientSecret(settings?.igdbClientSecret ?? "");
  }, [settings?.igdbClientId, settings?.igdbClientSecret]);

  const dirty =
    clientId.trim() !== (settings?.igdbClientId ?? "") || clientSecret.trim() !== (settings?.igdbClientSecret ?? "");
  const configured = !!settings?.igdbClientId && !!settings?.igdbClientSecret;

  const metadataProgress = useMetadataProgress();
  const fetchingMetadata =
    !!metadataProgress && metadataProgress.total > 0 && metadataProgress.done < metadataProgress.total;
  const [refreshRequested, setRefreshRequested] = useState(false);

  function save() {
    onUpdateSettings({ igdbClientId: clientId.trim(), igdbClientSecret: clientSecret.trim() });
  }

  async function refreshMetadata() {
    setRefreshRequested(true);
    try {
      await refreshAllMetadata();
    } finally {
      setRefreshRequested(false);
    }
  }

  return (
    <>
      <p className={styles.groupDescription}>
        Optional. Enriches your library with IGDB details, ratings, screenshots and box art. Get a Client ID and
        Secret from a{" "}
        <a href="https://dev.twitch.tv/console/apps" target="_blank" rel="noreferrer">
          Twitch Developer application
        </a>{" "}
        (IGDB uses Twitch's client-credentials flow) — the redirect URL can be <code>http://localhost</code>.
      </p>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Client ID</div>
        </div>
        <input
          className={styles.textInput}
          value={clientId}
          onChange={(e) => setClientId(e.target.value)}
          placeholder="Client ID"
          spellCheck={false}
          data-nav-item
        />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Client Secret</div>
          <div className={styles.rowDescription}>Stored locally — only ever sent to IGDB's own token endpoint.</div>
        </div>
        <div className={styles.secretField}>
          <input
            className={styles.textInput}
            type={revealSecret ? "text" : "password"}
            value={clientSecret}
            onChange={(e) => setClientSecret(e.target.value)}
            placeholder="Client Secret"
            spellCheck={false}
            data-nav-item
          />
          <button
            type="button"
            className={styles.revealButton}
            data-nav-item
            onClick={() => setRevealSecret((v) => !v)}
            aria-label={revealSecret ? "Hide client secret" : "Show client secret"}
            title={revealSecret ? "Hide client secret" : "Show client secret"}
          >
            {revealSecret ? "🙈" : "👁"}
          </button>
        </div>
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Status</div>
          <div className={styles.rowDescription}>
            {configured
              ? "Configured — details, ratings and artwork are fetched from IGDB."
              : "Not configured — Wervik works fine without it, just without IGDB-sourced details and artwork."}
          </div>
        </div>
        <button type="button" className={styles.rescanButton} data-nav-item onClick={save} disabled={!dirty}>
          Save
        </button>
      </div>
      {configured && (
        <div className={styles.row}>
          <div>
            <div className={styles.rowLabel}>Refresh metadata</div>
            <div className={styles.rowDescription}>
              Re-fetches every game from IGDB from scratch, including cover art. Use this if a game shows the wrong
              details or artwork — a cached bad match otherwise sticks around for up to 30 days.
            </div>
          </div>
          <button
            type="button"
            className={styles.rescanButton}
            data-nav-item
            onClick={refreshMetadata}
            disabled={refreshRequested || fetchingMetadata}
          >
            {(refreshRequested || fetchingMetadata) && <Spinner />}
            {fetchingMetadata
              ? `Updating ${metadataProgress!.done}/${metadataProgress!.total}…`
              : refreshRequested
                ? "Starting…"
                : "Refresh now"}
          </button>
        </div>
      )}
    </>
  );
}

/** Two unrelated things live here: the read-only achievements viewer
 * (persisted username + Web API key, like `IgdbFields` above) and a
 * fire-and-forget action that points RetroArch's own live achievements
 * engine at the user's account for the platforms Wervik launches through
 * it. The RetroArch password is local-only state — it's never part of
 * `settings` and is cleared right after being sent. */
function RetroAchievementsFields({
  settings,
  onUpdateSettings,
}: {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}) {
  const [username, setUsername] = useState(settings?.raUsername ?? "");
  const [apiKey, setApiKey] = useState(settings?.raApiKey ?? "");
  const [revealApiKey, setRevealApiKey] = useState(false);

  useEffect(() => {
    setUsername(settings?.raUsername ?? "");
    setApiKey(settings?.raApiKey ?? "");
  }, [settings?.raUsername, settings?.raApiKey]);

  const dirty = username.trim() !== (settings?.raUsername ?? "") || apiKey.trim() !== (settings?.raApiKey ?? "");
  const configured = !!settings?.raUsername && !!settings?.raApiKey;

  function save() {
    onUpdateSettings({ raUsername: username.trim(), raApiKey: apiKey.trim() });
  }

  const [nativePassword, setNativePassword] = useState("");
  const [nativeHardcore, setNativeHardcore] = useState(false);
  const [nativeBusy, setNativeBusy] = useState<"enable" | "disable" | null>(null);
  const [nativeMessage, setNativeMessage] = useState<{ error: boolean; text: string } | null>(null);

  async function handleEnableNative() {
    if (!username.trim() || !nativePassword.trim()) {
      setNativeMessage({ error: true, text: "Enter your username above and a password first." });
      return;
    }
    setNativeBusy("enable");
    setNativeMessage(null);
    try {
      await applyRetroArchAchievements(true, username.trim(), nativePassword, nativeHardcore);
      setNativeMessage({ error: false, text: "Enabled — RetroArch will sign in the next time it launches." });
      setNativePassword("");
    } catch (error) {
      setNativeMessage({ error: true, text: String(error) });
    } finally {
      setNativeBusy(null);
    }
  }

  async function handleDisableNative() {
    setNativeBusy("disable");
    setNativeMessage(null);
    try {
      await applyRetroArchAchievements(false, "", "", false);
      setNativeMessage({ error: false, text: "Disabled in RetroArch." });
    } catch (error) {
      setNativeMessage({ error: true, text: String(error) });
    } finally {
      setNativeBusy(null);
    }
  }

  return (
    <>
      <p className={styles.groupDescription}>
        Optional. Shows earned and locked achievements (with points) on a retro game's page, matched by title
        against RetroAchievements — not by ROM hash, so an unusual release may not match. Get a Web API key from
        your{" "}
        <a href="https://retroachievements.org/settings" target="_blank" rel="noreferrer">
          RetroAchievements control panel
        </a>{" "}
        ("Keys" section).
      </p>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Username</div>
        </div>
        <input
          className={styles.textInput}
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="Username"
          spellCheck={false}
          data-nav-item
        />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Web API Key</div>
          <div className={styles.rowDescription}>
            Stored locally — only ever sent to RetroAchievements' own API.
          </div>
        </div>
        <div className={styles.secretField}>
          <input
            className={styles.textInput}
            type={revealApiKey ? "text" : "password"}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="Web API Key"
            spellCheck={false}
            data-nav-item
          />
          <button
            type="button"
            className={styles.revealButton}
            data-nav-item
            onClick={() => setRevealApiKey((v) => !v)}
            aria-label={revealApiKey ? "Hide API key" : "Show API key"}
            title={revealApiKey ? "Hide API key" : "Show API key"}
          >
            {revealApiKey ? "🙈" : "👁"}
          </button>
        </div>
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Status</div>
          <div className={styles.rowDescription}>
            {configured
              ? "Configured — achievements appear on a retro game's page when matched."
              : "Not configured — Wervik works fine without it."}
          </div>
        </div>
        <button type="button" className={styles.rescanButton} data-nav-item onClick={save} disabled={!dirty}>
          Save
        </button>
      </div>

      <p className={styles.groupTitle} style={{ marginTop: 28 }}>
        Real achievements in RetroArch
      </p>
      <p className={styles.groupDescription}>
        Arcade, CPS and Neo Geo games launch through Wervik's own managed RetroArch, which has a real
        RetroAchievements engine — unlock popups and hardcore mode included, unlike the viewer above. This sends
        your login straight into RetroArch's own config; Wervik never stores the password itself, and RetroArch
        replaces it with a session token as soon as it signs in.
      </p>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Password</div>
          <div className={styles.rowDescription}>Your RetroAchievements account password — not the Web API key above.</div>
        </div>
        <input
          className={styles.textInput}
          type="password"
          value={nativePassword}
          onChange={(e) => setNativePassword(e.target.value)}
          placeholder="Password"
          spellCheck={false}
          data-nav-item
        />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Hardcore mode</div>
          <div className={styles.rowDescription}>
            Disables save states, rewind and cheats while playing — some achievements require it.
          </div>
        </div>
        <Switch checked={nativeHardcore} onChange={setNativeHardcore} label="Hardcore mode" />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Apply to RetroArch</div>
          {nativeMessage && (
            <div className={nativeMessage.error ? styles.statusMessageError : styles.statusMessage}>
              {nativeMessage.text}
            </div>
          )}
        </div>
        <div className={styles.nativeActions}>
          <button
            type="button"
            className={styles.rescanButton}
            data-nav-item
            onClick={handleEnableNative}
            disabled={nativeBusy !== null}
          >
            {nativeBusy === "enable" && <Spinner />}
            {nativeBusy === "enable" ? "Enabling…" : "Enable"}
          </button>
          <button
            type="button"
            className={styles.romRemoveButton}
            data-nav-item
            onClick={handleDisableNative}
            disabled={nativeBusy !== null}
          >
            {nativeBusy === "disable" && <Spinner />}
            {nativeBusy === "disable" ? "Disabling…" : "Disable"}
          </button>
        </div>
      </div>
    </>
  );
}

/** Steam's counterpart to `IgdbFields`/`RetroAchievementsFields` above —
 * matched by the exact Steam app id, so there's no fuzzy-match caveat to
 * mention here the way there is for RetroAchievements. */
function SteamAchievementsFields({
  settings,
  onUpdateSettings,
}: {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}) {
  const [apiKey, setApiKey] = useState(settings?.steamApiKey ?? "");
  const [steamId64Override, setSteamId64Override] = useState(settings?.steamId64Override ?? "");
  const [revealApiKey, setRevealApiKey] = useState(false);

  useEffect(() => {
    setApiKey(settings?.steamApiKey ?? "");
    setSteamId64Override(settings?.steamId64Override ?? "");
  }, [settings?.steamApiKey, settings?.steamId64Override]);

  const dirty =
    apiKey.trim() !== (settings?.steamApiKey ?? "") ||
    steamId64Override.trim() !== (settings?.steamId64Override ?? "");
  const configured = !!settings?.steamApiKey;

  function save() {
    onUpdateSettings({ steamApiKey: apiKey.trim(), steamId64Override: steamId64Override.trim() });
  }

  return (
    <>
      <p className={styles.groupDescription}>
        Optional. In addition to achievements, this lets Wervik show games owned by your Steam account even when
        they are not installed yet. Get a Web API key from your{" "}
        <a href="https://steamcommunity.com/dev/apikey" target="_blank" rel="noreferrer">
          Steam API key page
        </a>
        . Steam may return an empty owned-games list unless your profile's "Game details" privacy is Public — that's
        a Steam setting, not something Wervik can change for you.
      </p>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Web API Key</div>
          <div className={styles.rowDescription}>Stored locally — only ever sent to Steam's own API.</div>
        </div>
        <div className={styles.secretField}>
          <input
            className={styles.textInput}
            type={revealApiKey ? "text" : "password"}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="Web API Key"
            spellCheck={false}
            data-nav-item
          />
          <button
            type="button"
            className={styles.revealButton}
            data-nav-item
            onClick={() => setRevealApiKey((v) => !v)}
            aria-label={revealApiKey ? "Hide API key" : "Show API key"}
            title={revealApiKey ? "Hide API key" : "Show API key"}
          >
            {revealApiKey ? "🙈" : "👁"}
          </button>
        </div>
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>SteamID64 override</div>
          <div className={styles.rowDescription}>
            Detected automatically from your local Steam install — only set this if that ever picks the wrong
            account (e.g. more than one account has signed in on this machine).
          </div>
        </div>
        <input
          className={styles.textInput}
          value={steamId64Override}
          onChange={(e) => setSteamId64Override(e.target.value)}
          placeholder="Auto-detected"
          spellCheck={false}
          data-nav-item
        />
      </div>
      <div className={styles.row}>
        <div>
          <div className={styles.rowLabel}>Status</div>
          <div className={styles.rowDescription}>
            {configured
              ? "Configured — owned, non-installed Steam games and achievements are available."
              : "Not configured — Wervik works fine without it."}
          </div>
        </div>
        <button type="button" className={styles.rescanButton} data-nav-item onClick={save} disabled={!dirty}>
          Save
        </button>
      </div>
    </>
  );
}

export function Settings({
  snapshot,
  settings,
  rescanning,
  onUpdateSettings,
  onRescan,
  onLibraryChange,
}: SettingsProps) {
  const [category, setCategory] = useState<Category>("steam");
  const scaleIndex = settings ? SCALE_STEPS.indexOf(settings.uiScale) : -1;
  const [romDirectories, setRomDirectories] = useState<string[]>([]);
  // Which ROM-folder action is in flight, if any — lets each button show
  // its own "in progress" state instead of one generic flag for all three.
  const [pendingAction, setPendingAction] = useState<"add" | "rescan" | { remove: string } | null>(null);
  const romScanning = pendingAction !== null;

  useEffect(() => {
    getRomDirectories().then(setRomDirectories);
  }, []);

  async function handleAddRomDirectory() {
    const selected = await open({ directory: true, multiple: false, title: "Select a folder containing ROMs" });
    if (!selected || Array.isArray(selected)) return;
    setPendingAction("add");
    try {
      const snapshot = await addRomDirectory(selected);
      onLibraryChange(snapshot);
      setRomDirectories(await getRomDirectories());
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRemoveRomDirectory(path: string) {
    setPendingAction({ remove: path });
    try {
      const snapshot = await removeRomDirectory(path);
      onLibraryChange(snapshot);
      setRomDirectories(await getRomDirectories());
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRescanRoms() {
    setPendingAction("rescan");
    try {
      onLibraryChange(await rescanRetroLibrary());
    } finally {
      setPendingAction(null);
    }
  }

  function stepScale(delta: number) {
    if (!settings) return;
    const currentIndex = scaleIndex === -1 ? 1 : scaleIndex;
    const nextIndex = Math.min(SCALE_STEPS.length - 1, Math.max(0, currentIndex + delta));
    onUpdateSettings({ uiScale: SCALE_STEPS[nextIndex] });
  }

  function stepMusicVolume(delta: number) {
    if (!settings) return;
    const next = Math.min(1, Math.max(0, Math.round((settings.musicVolume + delta) * 10) / 10));
    onUpdateSettings({ musicVolume: next });
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Settings</h1>

      <div className={styles.layout}>
        <nav className={styles.sidebar} aria-label="Settings categories">
          {CATEGORIES.map((entry) => (
            <button
              key={entry.id}
              type="button"
              data-nav-item
              className={`${styles.sidebarButton} ${category === entry.id ? styles.sidebarButtonActive : ""}`}
              aria-current={category === entry.id}
              onClick={() => setCategory(entry.id)}
            >
              {entry.label}
            </button>
          ))}
        </nav>

        <div className={styles.content}>
          {category === "steam" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Steam</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Detected installation</div>
                  <div className={styles.rowDescription}>The local Steam install Wervik reads from.</div>
                </div>
                <div className={styles.rowValue}>{snapshot?.steamRoot ?? "Not found"}</div>
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Library folders</div>
                  <div className={styles.rowDescription}>Where installed games are discovered.</div>
                </div>
                <div className={styles.rowValue}>
                  {snapshot && snapshot.libraryFolders.length > 0
                    ? snapshot.libraryFolders.join(", ")
                    : "None found"}
                </div>
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Rescan library</div>
                  <div className={styles.rowDescription}>Look for newly installed or removed games.</div>
                </div>
                <button
                  type="button"
                  className={styles.rescanButton}
                  data-nav-item
                  onClick={onRescan}
                  disabled={rescanning}
                >
                  {rescanning && <Spinner />}
                  {rescanning ? "Scanning…" : "Rescan now"}
                </button>
              </div>

              <p className={styles.groupTitle} style={{ marginTop: 28 }}>
                Achievements
              </p>
              <SteamAchievementsFields settings={settings} onUpdateSettings={onUpdateSettings} />
            </div>
          )}

          {category === "battlenet" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Battle.net</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Installed games</div>
                  <div className={styles.rowDescription}>
                    Games registered in the local Battle.net Agent database.
                  </div>
                </div>
                <div className={styles.rowValue}>
                  {snapshot?.battlenetFound
                    ? `${snapshot.games.filter((game) => game.source === "battlenet").length} found`
                    : "None found"}
                </div>
              </div>
            </div>
          )}

          {category === "ea" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>EA App</p>
              <p className={styles.groupDescription}>
                Wervik detects installed games registered by EA App or the older Origin client on Windows. EA App
                itself handles launching and installation.
              </p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Installed games</div>
                  <div className={styles.rowDescription}>Games found in the local EA/Origin registry entries.</div>
                </div>
                <div className={styles.rowValue}>
                  {snapshot?.eaFound
                    ? `${snapshot.games.filter((game) => game.source === "ea").length} found`
                    : "None found"}
                </div>
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Rescan library</div>
                  <div className={styles.rowDescription}>Look for newly installed or removed EA App games.</div>
                </div>
                <button
                  type="button"
                  className={styles.rescanButton}
                  data-nav-item
                  onClick={onRescan}
                  disabled={rescanning}
                >
                  {rescanning && <Spinner />}
                  {rescanning ? "Scanning…" : "Rescan now"}
                </button>
              </div>
            </div>
          )}

          {category === "xbox" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Xbox App &amp; PC Game Pass</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Installed games</div>
                  <div className={styles.rowDescription}>Games registered for the current Windows user.</div>
                </div>
                <div className={styles.rowValue}>
                  {snapshot?.xboxFound
                    ? `${snapshot.games.filter((game) => game.source === "xbox").length} found`
                    : "None found"}
                </div>
              </div>
            </div>
          )}

          {category === "retro" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Retro &amp; emulation</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>ROM folders</div>
                  <div className={styles.rowDescription}>
                    Wervik scans these for supported ROMs and plays them with EmulatorJS.
                  </div>
                </div>
                <button
                  type="button"
                  className={styles.rescanButton}
                  data-nav-item
                  onClick={handleAddRomDirectory}
                  disabled={romScanning}
                >
                  {pendingAction === "add" && <Spinner />}
                  {pendingAction === "add" ? "Adding…" : "Add folder…"}
                </button>
              </div>
              {romDirectories.length > 0 && (
                <div className={styles.romList}>
                  {romDirectories.map((dir) => {
                    const removing =
                      typeof pendingAction === "object" && pendingAction !== null && pendingAction.remove === dir;
                    return (
                      <div key={dir} className={styles.romListItem}>
                        <span className={styles.romPath}>{dir}</span>
                        <button
                          type="button"
                          className={styles.romRemoveButton}
                          data-nav-item
                          onClick={() => handleRemoveRomDirectory(dir)}
                          disabled={romScanning}
                          aria-label={`Remove ${dir}`}
                        >
                          {removing && <Spinner />}
                          {removing ? "Removing…" : "Remove"}
                        </button>
                      </div>
                    );
                  })}
                </div>
              )}
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Rescan ROM library</div>
                  <div className={styles.rowDescription}>Look for newly added or removed ROMs.</div>
                </div>
                <button
                  type="button"
                  className={styles.rescanButton}
                  data-nav-item
                  onClick={handleRescanRoms}
                  disabled={romScanning}
                >
                  {pendingAction === "rescan" && <Spinner />}
                  {pendingAction === "rescan" ? "Scanning…" : "Rescan now"}
                </button>
              </div>
            </div>
          )}

          {category === "igdb" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>IGDB</p>
              <IgdbFields settings={settings} onUpdateSettings={onUpdateSettings} />
            </div>
          )}

          {category === "retroachievements" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>RetroAchievements</p>
              <RetroAchievementsFields settings={settings} onUpdateSettings={onUpdateSettings} />
            </div>
          )}

          {category === "interface" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Interface</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Launch in fullscreen</div>
                  <div className={styles.rowDescription}>Start Wervik in fullscreen mode.</div>
                </div>
                <Switch
                  checked={!!settings?.fullscreen}
                  onChange={(v) => onUpdateSettings({ fullscreen: v })}
                  label="Launch in fullscreen"
                />
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Launch on system startup</div>
                  <div className={styles.rowDescription}>Open Wervik automatically when you log in.</div>
                </div>
                <Switch
                  checked={!!settings?.launchOnStartup}
                  onChange={(v) => onUpdateSettings({ launchOnStartup: v })}
                  label="Launch on system startup"
                />
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Interface scale</div>
                  <div className={styles.rowDescription}>Adjust text and card size.</div>
                </div>
                <div className={styles.scaleControl}>
                  <button
                    type="button"
                    className={styles.scaleButton}
                    data-nav-item
                    onClick={() => stepScale(-1)}
                    aria-label="Decrease interface scale"
                  >
                    −
                  </button>
                  <span className={styles.scaleValue}>{Math.round((settings?.uiScale ?? 1) * 100)}%</span>
                  <button
                    type="button"
                    className={styles.scaleButton}
                    data-nav-item
                    onClick={() => stepScale(1)}
                    aria-label="Increase interface scale"
                  >
                    +
                  </button>
                </div>
              </div>
            </div>
          )}

          {category === "audio" && (
            <div className={styles.group}>
              <p className={styles.groupTitle}>Audio</p>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Background music</div>
                  <div className={styles.rowDescription}>
                    A small generated chiptune loop while browsing your library — pauses automatically whenever you
                    open a game.
                  </div>
                </div>
                <Switch
                  checked={!!settings?.musicEnabled}
                  onChange={(v) => onUpdateSettings({ musicEnabled: v })}
                  label="Background music"
                />
              </div>
              <div className={styles.row}>
                <div>
                  <div className={styles.rowLabel}>Music volume</div>
                  <div className={styles.rowDescription}>How loud the background music plays.</div>
                </div>
                <div className={styles.scaleControl}>
                  <button
                    type="button"
                    className={styles.scaleButton}
                    data-nav-item
                    onClick={() => stepMusicVolume(-0.1)}
                    disabled={!settings?.musicEnabled}
                    aria-label="Decrease music volume"
                  >
                    −
                  </button>
                  <span className={styles.scaleValue}>{Math.round((settings?.musicVolume ?? 0) * 100)}%</span>
                  <button
                    type="button"
                    className={styles.scaleButton}
                    data-nav-item
                    onClick={() => stepMusicVolume(0.1)}
                    disabled={!settings?.musicEnabled}
                    aria-label="Increase music volume"
                  >
                    +
                  </button>
                </div>
              </div>
            </div>
          )}

          <p className={styles.about}>Wervik — by Wergely</p>
        </div>
      </div>
    </div>
  );
}
