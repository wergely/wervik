import { useEffect, useRef, useState } from "react";
import { getPlatformStatus, installNativeEmulator } from "../api/tauri";
import { useWervikAccount } from "../hooks/useWervikAccount";
import type { LibrarySnapshot, PlatformStatus, Settings as SettingsType } from "../types";
import { Logo } from "./Logo";
import { PlatformIcon } from "./PlatformIcon";
import { RetroAchievementsFields } from "./RetroAchievementsFields";
import { RomDirectoriesEditor } from "./RomDirectoriesEditor";
import settingsStyles from "./Settings.module.css";
import { Spinner } from "./Spinner";
import styles from "./Welcome.module.css";

interface WelcomeProps {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
  onLibraryChange: (snapshot: LibrarySnapshot) => void;
  onDone: () => void;
}

const STEPS = ["Welcome", "Account", "Emulators", "Achievements", "ROM folders", "Done"];

export function Welcome({ settings, onUpdateSettings, onLibraryChange, onDone }: WelcomeProps) {
  const [step, setStep] = useState(0);
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    requestAnimationFrame(() => cardRef.current?.querySelector<HTMLElement>("[data-nav-item]")?.focus());
  }, [step]);

  return (
    <div className={styles.page}>
      <div className={styles.card} ref={cardRef}>
        <div className={styles.dots} role="tablist" aria-label="Setup steps">
          {STEPS.map((label, index) => (
            <span
              key={label}
              className={`${styles.dot} ${index === step ? styles.dotActive : ""} ${
                index < step ? styles.dotDone : ""
              }`}
              role="tab"
              aria-selected={index === step}
              aria-label={label}
            />
          ))}
        </div>

        <div className={`${styles.content} wervik-scroll`}>
          {step === 0 && <IntroStep />}
          {step === 1 && <AccountStep />}
          {step === 2 && <EmulatorsStep />}
          {step === 3 && <AchievementsStep settings={settings} onUpdateSettings={onUpdateSettings} />}
          {step === 4 && <RomsStep onLibraryChange={onLibraryChange} />}
          {step === 5 && <FinishStep />}
        </div>

        <div className={styles.nav}>
          {step > 0 ? (
            <button
              type="button"
              className={styles.backButton}
              data-nav-item
              onClick={() => setStep((s) => s - 1)}
            >
              ← Back
            </button>
          ) : (
            <span />
          )}
          {step < STEPS.length - 1 ? (
            <button type="button" className={styles.nextButton} data-nav-item onClick={() => setStep((s) => s + 1)}>
              Next
            </button>
          ) : (
            <button type="button" className={styles.nextButton} data-nav-item onClick={onDone}>
              Get started
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function IntroStep() {
  return (
    <div className={styles.intro}>
      <Logo size={56} />
      <h1 className={styles.title}>Welcome to Wervik</h1>
      <p className={styles.lead}>
        Wervik is one home for your whole game library — Steam, Xbox App &amp; PC Game Pass, Battle.net, EA App, and
        retro ROMs — so you stop hopping between launchers to see what you own and what to play next.
      </p>
      <ul className={styles.introList}>
        <li>Launch anything from a single, unified library</li>
        <li>Automatic box art, summaries and ratings for your games</li>
        <li>Achievements from RetroAchievements and Steam, tracked per game</li>
        <li>Native emulators for retro platforms, with BIOS and graphics presets handled for you</li>
      </ul>
      <p className={styles.leadSmall}>The next few screens get the basics set up — it only takes a minute.</p>
    </div>
  );
}

function AccountStep() {
  const { account, connecting, error, connect, disconnect } = useWervikAccount();

  return (
    <div className={styles.step}>
      <h1 className={styles.title}>Your Wervik Account</h1>
      <p className={styles.lead}>
        Choose how Wervik gets game data and, optionally, syncs your settings — you can change this later in
        Settings.
      </p>
      {error && <div className={settingsStyles.statusMessageError}>{error}</div>}
      <div className={settingsStyles.accountChoices}>
        <div className={settingsStyles.accountCard}>
          <div className={settingsStyles.accountCardHeader}>
            <div className={settingsStyles.accountCardTitle}>No Wervik Account</div>
            {!account && <div className={settingsStyles.accountCardBadge}>Current</div>}
          </div>
          <p className={settingsStyles.accountCardDescription}>Nothing ever leaves this computer.</p>
          <ul className={settingsStyles.accountCardList}>
            <li>Set up IGDB yourself (Client ID/Secret) in Settings → Metadata for details, ratings and box art</li>
            <li>Set up a Steam Web API key yourself in Settings → Steam for owned games and achievements</li>
          </ul>
        </div>
        <div className={settingsStyles.accountCard}>
          <div className={settingsStyles.accountCardHeader}>
            <div className={settingsStyles.accountCardTitle}>Connect Wervik Account</div>
            {account && <div className={settingsStyles.accountCardBadge}>Connected</div>}
          </div>
          <p className={settingsStyles.accountCardDescription}>Sign in in your browser to connect this installation.</p>
          <ul className={settingsStyles.accountCardList}>
            <li>Native Steam integration — owned games and achievements, no Web API key to generate</li>
            <li>Metadata via Wervik — no IGDB credentials to set up</li>
            <li>Settings sync automatically to any other installation on this same account</li>
            <li>Your game library itself still stays only on this computer</li>
          </ul>
          {account ? (
            <div className={styles.connectedRow}>
              <div>
                <div className={settingsStyles.rowLabel}>{account.name}</div>
                <div className={settingsStyles.rowDescription}>{account.email}</div>
              </div>
              <button type="button" className={settingsStyles.rescanButton} data-nav-item onClick={disconnect}>
                Disconnect
              </button>
            </div>
          ) : (
            <button
              type="button"
              className={`${settingsStyles.rescanButton} ${settingsStyles.accountCardButton}`}
              data-nav-item
              onClick={connect}
              disabled={connecting}
            >
              {connecting && <Spinner />}
              {connecting ? "Waiting for login…" : "Connect"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** A platform whose selected emulator Wervik can download itself but
 * hasn't yet — see `EmulatorOption.managed`/`available`. */
function pendingDownload(platform: PlatformStatus) {
  const selected = platform.emulatorOptions.find((option) => option.id === platform.selectedEmulator);
  return selected && !selected.available && selected.managed ? selected : null;
}

function EmulatorsStep() {
  const [platforms, setPlatforms] = useState<PlatformStatus[] | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    getPlatformStatus().then(setPlatforms);
  }, []);

  async function handleInstall(platformKey: string, emulatorId: string) {
    setInstalling(emulatorId);
    setInstallError(null);
    try {
      await installNativeEmulator(platformKey, emulatorId);
      setPlatforms(await getPlatformStatus());
    } catch (err) {
      setInstallError(typeof err === "string" ? err : "Couldn't download this emulator.");
    } finally {
      setInstalling(null);
    }
  }

  const pending = platforms?.filter((platform) => pendingDownload(platform) !== null) ?? [];

  return (
    <div className={styles.step}>
      <h1 className={styles.title}>Emulators</h1>
      <p className={styles.lead}>
        Retro platforms run through native emulators, including RetroArch/Libretro cores — Wervik can download the
        recommended one for each platform right now.
      </p>
      {installError && <div className={settingsStyles.statusMessageError}>{installError}</div>}
      {platforms === null ? (
        <p className={styles.statusText}>Loading…</p>
      ) : pending.length === 0 ? (
        <p className={styles.statusText}>Every recommended emulator is already installed. Nothing to do here.</p>
      ) : (
        <div className={styles.emulatorRows}>
          {pending.map((platform) => {
            const option = pendingDownload(platform)!;
            const isInstalling = installing === option.id;
            return (
              <div key={platform.key} className={styles.emulatorRow}>
                <PlatformIcon source="retro" platformKey={platform.key} size={28} />
                <div className={styles.emulatorRowInfo}>
                  <span className={styles.emulatorRowName}>{platform.displayName}</span>
                  <span className={styles.emulatorRowDetail}>{option.displayName}</span>
                </div>
                <button
                  type="button"
                  className={settingsStyles.rescanButton}
                  data-nav-item
                  onClick={() => handleInstall(platform.key, option.id)}
                  disabled={isInstalling}
                >
                  {isInstalling && <Spinner />}
                  {isInstalling ? "Downloading…" : "Download"}
                </button>
              </div>
            );
          })}
        </div>
      )}
      <p className={styles.leadSmall}>
        You can install or switch emulators for any platform later from Platforms, or skip this for now.
      </p>
    </div>
  );
}

function AchievementsStep({
  settings,
  onUpdateSettings,
}: {
  settings: SettingsType | null;
  onUpdateSettings: (partial: Partial<SettingsType>) => void;
}) {
  return (
    <div className={styles.step}>
      <h1 className={styles.title}>RetroAchievements</h1>
      <p className={styles.lead}>
        Optional — connect a RetroAchievements account to see your progress on the game page and unlock real-time
        achievements (and Hardcore mode) in managed RetroArch/PCSX2 games.
      </p>
      <RetroAchievementsFields settings={settings} onUpdateSettings={onUpdateSettings} />
    </div>
  );
}

function RomsStep({ onLibraryChange }: { onLibraryChange: (snapshot: LibrarySnapshot) => void }) {
  return (
    <div className={styles.step}>
      <h1 className={styles.title}>ROM folders</h1>
      <p className={styles.lead}>
        Point Wervik at the folders where your ROMs live — it scans them and matches each game automatically.
      </p>
      <RomDirectoriesEditor onLibraryChange={onLibraryChange} />
    </div>
  );
}

function FinishStep() {
  return (
    <div className={styles.intro}>
      <Logo size={56} />
      <h1 className={styles.title}>You're all set!</h1>
      <p className={styles.lead}>
        Your library is ready. Browse everything from Home, dig into a game's achievements, or fine-tune anything at
        all in Settings.
      </p>
      <p className={styles.haveFun}>Have fun! 🎮</p>
    </div>
  );
}
