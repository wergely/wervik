import { useEffect, useRef, useState } from "react";
import {
  getPlatformStatus,
  installNativeEmulator,
  openNativeEmulatorSettings,
  setDefaultEmulator,
  setGraphicsPreset,
} from "../api/tauri";
import type { EmulatorOption, PlatformStatus } from "../types";
import { BiosControl } from "./BiosControl";
import { platformArtworkUrl, PlatformIcon } from "./PlatformIcon";
import { Spinner } from "./Spinner";
import { Switch } from "./Switch";
import styles from "./PlatformDetail.module.css";

interface PlatformDetailProps {
  platformKey: string;
  onBack: () => void;
}

/** What will run once `selectedId` is picked, given each option's current
 * availability — mirrors the Rust resolution in `commands::effective_emulator_id`
 * so the UI can update instantly without waiting on a round trip. */
function resolveEffective(options: EmulatorOption[], selectedId: string): string | null {
  const selected = options.find((o) => o.id === selectedId);
  return selected?.available ? selected.id : null;
}

function availabilityLabel(option: EmulatorOption): string {
  return option.available ? "Installed" : "Not installed";
}

export function PlatformDetail({ platformKey, onBack }: PlatformDetailProps) {
  const [platform, setPlatform] = useState<PlatformStatus | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [applyingPreset, setApplyingPreset] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const backButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    backButtonRef.current?.focus();
    setActionError(null);
    getPlatformStatus().then((list) => setPlatform(list.find((p) => p.key === platformKey) ?? null));
  }, [platformKey]);

  function handleSelectEmulator(emulatorId: string) {
    setPlatform((prev) =>
      prev
        ? { ...prev, selectedEmulator: emulatorId, effectiveEmulator: resolveEffective(prev.emulatorOptions, emulatorId) }
        : prev,
    );
    setDefaultEmulator(platformKey, emulatorId).catch(() => {});
  }

  async function handleInstall(emulatorId: string) {
    setInstalling(emulatorId);
    setActionError(null);
    try {
      await installNativeEmulator(platformKey, emulatorId);
      const list = await getPlatformStatus();
      setPlatform(list.find((p) => p.key === platformKey) ?? null);
    } catch (err) {
      setActionError(typeof err === "string" ? err : "Couldn't download this emulator.");
    } finally {
      setInstalling(null);
    }
  }

  async function handleSetGraphicsPreset(optimized: boolean) {
    if (applyingPreset) return;
    setActionError(null);
    setApplyingPreset(true);
    const previous = platform?.graphicsPreset ?? null;
    setPlatform((prev) => (prev ? { ...prev, graphicsPreset: optimized ? "optimized" : "original" } : prev));
    try {
      await setGraphicsPreset(platformKey, optimized);
    } catch (err) {
      setPlatform((prev) => (prev ? { ...prev, graphicsPreset: previous } : prev));
      setActionError(typeof err === "string" ? err : "Couldn't apply that graphics preset.");
    } finally {
      setApplyingPreset(false);
    }
  }

  async function handleOpenEmulatorSettings(emulatorId: string) {
    setActionError(null);
    try {
      await openNativeEmulatorSettings(platformKey, emulatorId);
    } catch (err) {
      setActionError(typeof err === "string" ? err : "Couldn't open the emulator's settings.");
    }
  }

  if (!platform) {
    return (
      <div className={styles.detail}>
        <button
          type="button"
          ref={backButtonRef}
          className={styles.backButton}
          data-nav-item
          onClick={onBack}
          aria-label="Back to platforms"
        >
          ← Platforms
        </button>
      </div>
    );
  }

  const selected = platform.emulatorOptions.find((o) => o.id === platform.selectedEmulator);
  const effective = platform.emulatorOptions.find((o) => o.id === platform.effectiveEmulator);
  const artworkUrl = platformArtworkUrl(platform.key);

  return (
    <div className={styles.detail}>
      <button
        type="button"
        ref={backButtonRef}
        className={styles.backButton}
        data-nav-item
        onClick={onBack}
        aria-label="Back to platforms"
      >
        ← Platforms
      </button>

      <div className={styles.hero}>
        {artworkUrl ? (
          <img
            className={styles.heroImage}
            src={artworkUrl}
            alt=""
            draggable={false}
            loading="eager"
            decoding="async"
          />
        ) : (
          <div className={styles.heroFallbackIcon}>
            <PlatformIcon source="retro" platformKey={platform.key} size={120} />
          </div>
        )}
        <div className={styles.heroScrim} />
        <div className={styles.heroContent}>
          <h1 className={styles.title}>{platform.displayName}</h1>
          <p className={styles.count}>
            {platform.romCount} {platform.romCount === 1 ? "game" : "games"} in your library
          </p>
        </div>
      </div>

      <div className={styles.layout}>
        <div className={styles.main}>
          <p className={styles.sectionLabel}>Default Emulator</p>

          {platform.emulatorOptions.length > 0 ? (
            <div className={styles.emulatorList}>
              {platform.emulatorOptions.map((option) => {
                const isSelected = option.id === platform.selectedEmulator;
                const isInstalling = installing === option.id;
                const offerDownload = !option.available && option.managed;
                return (
                  <div key={option.id} className={styles.emulatorItem}>
                    <button
                      type="button"
                      className={`${styles.emulatorCard} ${isSelected ? styles.emulatorCardSelected : ""}`}
                      data-nav-item
                      onClick={() => handleSelectEmulator(option.id)}
                      disabled={!option.available}
                      aria-pressed={isSelected}
                    >
                      <span className={styles.radioDot} aria-hidden="true" />
                      <div className={styles.emulatorCardBody}>
                        <div className={styles.emulatorCardTop}>
                          <span className={styles.emulatorName}>{option.displayName}</span>
                          <span className={styles.kindBadge}>Native</span>
                        </div>
                        <div className={styles.emulatorCardBottom}>
                          <span className={!option.available ? styles.statusWarning : styles.status}>
                            {isSelected ? "Selected · " : ""}
                            {availabilityLabel(option)}
                          </span>
                          {option.recommended && <span className={styles.recommended}>Recommended</span>}
                        </div>
                      </div>
                    </button>

                    {offerDownload && (
                      <button
                        type="button"
                        className={styles.downloadButton}
                        data-nav-item
                        onClick={() => handleInstall(option.id)}
                        disabled={isInstalling}
                        aria-label={`Download ${option.displayName}`}
                      >
                        {isInstalling && <Spinner />}
                        {isInstalling ? "Downloading…" : "Download"}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <p className={styles.fallbackNotice}>
              No compatible emulator is available for this platform in this build yet.
            </p>
          )}

          {platform.requiresBios && platform.emulatorOptions.length > 0 && (
            <BiosControl platformKey={platform.key} platformName={platform.displayName} />
          )}

          {platform.graphicsPreset != null && (
            <div className={styles.graphicsPreset}>
              <p className={styles.sectionLabel}>Graphics</p>
              <div className={styles.presetRow}>
                <div>
                  <div className={styles.presetLabel}>Optimized resolution</div>
                  <div className={styles.presetDescription}>
                    Renders at a higher internal resolution than the original hardware for sharper
                    edges — same game, same art, just less jagged. Applies once you're using the
                    native RetroArch emulator above.
                  </div>
                </div>
                <Switch
                  checked={platform.graphicsPreset === "optimized"}
                  onChange={handleSetGraphicsPreset}
                  disabled={applyingPreset}
                  label="Optimized resolution"
                />
              </div>
            </div>
          )}

          {actionError && <p className={styles.actionError}>{actionError}</p>}

          {selected && !effective && (
            <p className={styles.fallbackNotice}>
              {selected.displayName} is not installed. Download it above before launching games on this platform.
            </p>
          )}

          {platform.emulatorOptions.length > 0 && (
            <p className={styles.hint}>
              Wervik uses the selected emulator by default for {platform.displayName} games. Individual games can
              override this setting.
            </p>
          )}

          {selected && selected.kind === "native" && (
            <div className={styles.actionRow}>
              {selected.available && (
                <button
                  type="button"
                  className={styles.setupButton}
                  data-nav-item
                  onClick={() => handleOpenEmulatorSettings(selected.id)}
                  aria-label={`Open ${selected.displayName} settings — controller, video, audio`}
                >
                  Open {selected.displayName} Settings
                </button>
              )}
            </div>
          )}
        </div>

        <aside className={styles.summary}>
          <div className={styles.summaryRow}>
            <span className={styles.summaryLabel}>Platform</span>
            <span className={styles.summaryValue}>{platform.displayName}</span>
          </div>
          <div className={styles.summaryRow}>
            <span className={styles.summaryLabel}>Library</span>
            <span className={styles.summaryValue}>
              {platform.romCount} {platform.romCount === 1 ? "game" : "games"}
            </span>
          </div>
          <div className={styles.summaryRow}>
            <span className={styles.summaryLabel}>Default Emulator</span>
            <span className={styles.summaryValue}>{selected?.displayName ?? "—"}</span>
          </div>
          {platform.graphicsPreset != null && (
            <div className={styles.summaryRow}>
              <span className={styles.summaryLabel}>Graphics</span>
              <span className={styles.summaryValue}>
                {platform.graphicsPreset === "optimized" ? "Optimized" : "Original"}
              </span>
            </div>
          )}
        </aside>
      </div>

    </div>
  );
}
