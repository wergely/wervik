import { useEffect, useRef, useState } from "react";
import { Switch } from "./Switch";
import type { EnvVar, GamescopeConfig, GameSetupInput, GameSetupStatus, MangoHudConfig, Upscaler } from "../types";
import styles from "./GameSetupDrawer.module.css";

interface GameSetupDrawerProps {
  /** "Game Setup" for a Steam game, "Platform Setup" for a platform. */
  title: string;
  /** Game name, or platform display name. */
  subtitle: string;
  fetchStatus: () => Promise<GameSetupStatus>;
  applyStatus: (input: GameSetupInput) => Promise<GameSetupStatus>;
  /** Steam-only: whether to show the "Launch Options" sub-view at all —
   * a native emulator launch has no external app to paste anything into. */
  hasLaunchOptions: boolean;
  onClose: () => void;
  registerBackHandler: (handler: (() => void) | null) => void;
  registerNavScope: (el: HTMLElement | null) => void;
  onStatusChange: (status: GameSetupStatus) => void;
}

type Preset = "minimal" | "standard" | "detailed";
type FpsLimitChoice = "off" | "30" | "60" | "90" | "120" | "144" | "custom";
type SubView = "root" | "launchOptions" | "envVars";

interface Draft {
  gamemodeEnabled: boolean;
  overlayOn: boolean;
  preset: Preset;
  fpsLimitChoice: FpsLimitChoice;
  fpsLimitCustom: number;
  gamescopeEnabled: boolean;
  internalWidth: number;
  internalHeight: number;
  outputWidth: number;
  outputHeight: number;
  upscaler: Upscaler;
  envVars: EnvVar[];
}

const PRESET_FLAGS: Record<
  Preset,
  Pick<MangoHudConfig, "fps" | "frameTiming" | "gpuStats" | "cpuStats" | "ram" | "vram">
> = {
  minimal: { fps: true, frameTiming: false, gpuStats: false, cpuStats: false, ram: false, vram: false },
  standard: { fps: true, frameTiming: true, gpuStats: true, cpuStats: true, ram: false, vram: false },
  detailed: { fps: true, frameTiming: true, gpuStats: true, cpuStats: true, ram: true, vram: true },
};

const FPS_LIMIT_PRESETS = [30, 60, 90, 120, 144];

function matchPreset(config: MangoHudConfig): Preset {
  const match = (Object.keys(PRESET_FLAGS) as Preset[]).find((name) => {
    const flags = PRESET_FLAGS[name];
    return (
      config.fps === flags.fps &&
      config.frameTiming === flags.frameTiming &&
      config.gpuStats === flags.gpuStats &&
      config.cpuStats === flags.cpuStats &&
      config.ram === flags.ram &&
      config.vram === flags.vram
    );
  });
  return match ?? "standard";
}

function draftFromStatus(status: GameSetupStatus): Draft {
  const cfg = status.mangohudConfig;
  const gs = status.gamescopeConfig;
  return {
    gamemodeEnabled: status.gamemodeEnabled,
    overlayOn: status.mangohudEnabled && !cfg.noDisplay,
    preset: matchPreset(cfg),
    fpsLimitChoice: FPS_LIMIT_PRESETS.includes(cfg.fpsLimit)
      ? (String(cfg.fpsLimit) as FpsLimitChoice)
      : cfg.fpsLimit > 0
        ? "custom"
        : "off",
    fpsLimitCustom: cfg.fpsLimit > 0 ? cfg.fpsLimit : 60,
    gamescopeEnabled: status.gamescopeEnabled,
    internalWidth: gs.internalWidth,
    internalHeight: gs.internalHeight,
    outputWidth: gs.outputWidth,
    outputHeight: gs.outputHeight,
    upscaler: gs.upscaler,
    envVars: status.envVars,
  };
}

function defaultDraft(): Draft {
  return {
    gamemodeEnabled: false,
    overlayOn: false,
    preset: "standard",
    fpsLimitChoice: "off",
    fpsLimitCustom: 60,
    gamescopeEnabled: false,
    internalWidth: 0,
    internalHeight: 0,
    outputWidth: 0,
    outputHeight: 0,
    upscaler: "none",
    envVars: [],
  };
}

function resolvedFpsLimit(draft: Draft): number {
  if (draft.fpsLimitChoice === "off") return 0;
  if (draft.fpsLimitChoice === "custom") return Math.max(0, draft.fpsLimitCustom);
  return Number(draft.fpsLimitChoice);
}

function buildInput(draft: Draft, baseConfig: MangoHudConfig) {
  const fpsLimit = resolvedFpsLimit(draft);
  const mangohudConfig: MangoHudConfig = {
    ...baseConfig,
    ...PRESET_FLAGS[draft.preset],
    fpsLimit,
    noDisplay: !draft.overlayOn && fpsLimit > 0,
  };
  const gamescopeConfig: GamescopeConfig = {
    internalWidth: draft.internalWidth,
    internalHeight: draft.internalHeight,
    outputWidth: draft.outputWidth,
    outputHeight: draft.outputHeight,
    upscaler: draft.upscaler,
  };
  return {
    mangohudEnabled: draft.overlayOn || fpsLimit > 0,
    mangohudConfig,
    gamemodeEnabled: draft.gamemodeEnabled,
    gamescopeEnabled: draft.gamescopeEnabled,
    gamescopeConfig,
    envVars: draft.envVars,
  };
}

export function GameSetupDrawer({
  title,
  subtitle,
  fetchStatus,
  applyStatus,
  hasLaunchOptions,
  onClose,
  registerBackHandler,
  registerNavScope,
  onStatusChange,
}: GameSetupDrawerProps) {
  const [status, setStatus] = useState<GameSetupStatus | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [subView, setSubView] = useState<SubView>("root");
  const [applying, setApplying] = useState(false);
  const [checking, setChecking] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const panelRef = useRef<HTMLDivElement>(null);
  const snippetRef = useRef<HTMLInputElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    registerNavScope(panelRef.current);
    return () => registerNavScope(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Escape/gamepad B backs out of a sub-view first, then closes the drawer.
  useEffect(() => {
    registerBackHandler(() => () => {
      if (subView !== "root") setSubView("root");
      else onCloseRef.current();
    });
    return () => registerBackHandler(null);
  }, [subView, registerBackHandler]);

  useEffect(() => {
    fetchStatus().then((s) => {
      setStatus(s);
      setDraft(draftFromStatus(s));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const id = requestAnimationFrame(() => {
      panelRef.current?.querySelector<HTMLElement>("[data-nav-item]")?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, []);

  function updateDraft(partial: Partial<Draft>) {
    setDraft((prev) => (prev ? { ...prev, ...partial } : prev));
  }

  async function handleApply() {
    if (!draft || !status) return;
    setApplying(true);
    setError(null);
    try {
      const updated = await applyStatus(buildInput(draft, status.mangohudConfig));
      setStatus(updated);
      setDraft(draftFromStatus(updated));
      onStatusChange(updated);
    } catch (err) {
      setError(typeof err === "string" ? err : "Couldn't save Game Setup.");
    } finally {
      setApplying(false);
    }
  }

  function handleReset() {
    setDraft(defaultDraft());
  }

  async function checkAgain() {
    setChecking(true);
    try {
      const s = await fetchStatus();
      setStatus(s);
      onStatusChange(s);
    } finally {
      setChecking(false);
    }
  }

  function copySnippet() {
    const text = status?.launchOptionsSnippet ?? "";
    navigator.clipboard?.writeText(text).catch(() => {});
    snippetRef.current?.select();
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  const isDirty = !!(status && draft && JSON.stringify(draft) !== JSON.stringify(draftFromStatus(status)));

  return (
    <>
      <div className={styles.backdrop} onClick={onClose} />
      <div className={styles.drawer} ref={panelRef}>
        <div className={styles.header}>
          <div className={styles.headerTitles}>
            <h2 className={styles.title}>{title}</h2>
            <p className={styles.subtitle}>{subtitle}</p>
          </div>
          <button
            type="button"
            className={styles.closeButton}
            data-nav-item
            onClick={onClose}
            aria-label="Close"
          >
            ×
          </button>
        </div>

        <div className={`${styles.body} wervik-scroll`}>
          {!status || !draft ? null : subView === "launchOptions" ? (
            <>
              <div className={styles.backRow}>
                <button
                  type="button"
                  className={styles.backButton}
                  data-nav-item
                  onClick={() => setSubView("root")}
                >
                  ← Back
                </button>
              </div>
              <h3 className={styles.subViewTitle}>Launch Options</h3>

              {status.steamConfigured ? (
                <p className={styles.configuredNotice}>
                  ✓ Steam is set up to run this game through Wervik. Everything below applies
                  instantly — no further Steam changes needed.
                </p>
              ) : (
                <>
                  <ol className={styles.setupSteps}>
                    <li>In Steam, right-click this game → Properties → General → Launch Options.</li>
                    <li>Paste this, then close the dialog (Steam saves it automatically):</li>
                  </ol>
                  <div className={styles.snippetRow}>
                    <input
                      ref={snippetRef}
                      type="text"
                      readOnly
                      className={styles.snippetInput}
                      data-nav-item
                      value={status.launchOptionsSnippet ?? ""}
                      onFocus={(e) => e.target.select()}
                    />
                    <button type="button" className={styles.copyButton} data-nav-item onClick={copySnippet}>
                      {copied ? "Copied!" : "Copy"}
                    </button>
                  </div>
                  <button
                    type="button"
                    className={styles.checkButton}
                    data-nav-item
                    onClick={checkAgain}
                    disabled={checking}
                  >
                    {checking ? "Checking…" : "I've pasted it — check again"}
                  </button>
                </>
              )}
            </>
          ) : subView === "envVars" ? (
            <>
              <div className={styles.backRow}>
                <button
                  type="button"
                  className={styles.backButton}
                  data-nav-item
                  onClick={() => setSubView("root")}
                >
                  ← Back
                </button>
              </div>
              <h3 className={styles.subViewTitle}>Environment Variables</h3>

              {draft.envVars.length === 0 && (
                <p className={styles.emptyEnvVars}>No custom variables set for this game.</p>
              )}
              {draft.envVars.map((v, i) => (
                <div className={styles.envVarRow} key={i}>
                  <input
                    type="text"
                    className={`${styles.textInput} ${styles.envKeyInput}`}
                    data-nav-item
                    placeholder="KEY"
                    value={v.key}
                    onChange={(e) => {
                      const next = [...draft.envVars];
                      next[i] = { ...next[i], key: e.target.value };
                      updateDraft({ envVars: next });
                    }}
                  />
                  <input
                    type="text"
                    className={`${styles.textInput} ${styles.envValueInput}`}
                    data-nav-item
                    placeholder="value"
                    value={v.value}
                    onChange={(e) => {
                      const next = [...draft.envVars];
                      next[i] = { ...next[i], value: e.target.value };
                      updateDraft({ envVars: next });
                    }}
                  />
                  <button
                    type="button"
                    className={styles.envRemoveButton}
                    data-nav-item
                    aria-label={`Remove ${v.key || "variable"}`}
                    onClick={() => updateDraft({ envVars: draft.envVars.filter((_, j) => j !== i) })}
                  >
                    ×
                  </button>
                </div>
              ))}
              <button
                type="button"
                className={styles.addVarButton}
                data-nav-item
                onClick={() => updateDraft({ envVars: [...draft.envVars, { key: "", value: "" }] })}
              >
                + Add variable
              </button>
            </>
          ) : (
            <>
              <section className={styles.section}>
                <p className={styles.sectionTitle}>Performance</p>
                {!status.gamemodeInstalled ? (
                  <div className={styles.notInstalled}>
                    <div className={styles.notInstalledLabel}>GameMode — Not installed</div>
                    <p className={styles.notInstalledHint}>
                      Install the <code>gamemode</code> package for your distro to reduce CPU/GPU
                      latency while playing.
                    </p>
                  </div>
                ) : (
                  <div className={styles.row}>
                    <span className={styles.rowLabel}>GameMode</span>
                    <Switch
                      checked={draft.gamemodeEnabled}
                      onChange={(v) => updateDraft({ gamemodeEnabled: v })}
                      label="Enable GameMode"
                    />
                  </div>
                )}
                <div className={styles.row}>
                  <div>
                    <div className={styles.rowLabel}>FPS Limit</div>
                    {!status.mangohudInstalled && (
                      <div className={styles.rowHint}>Requires MangoHud</div>
                    )}
                  </div>
                  <div className={styles.fpsLimitControl}>
                    <select
                      className={styles.select}
                      data-nav-item
                      aria-label="FPS limit"
                      disabled={!status.mangohudInstalled}
                      value={draft.fpsLimitChoice}
                      onChange={(e) => updateDraft({ fpsLimitChoice: e.target.value as FpsLimitChoice })}
                    >
                      <option value="off">Off</option>
                      {FPS_LIMIT_PRESETS.map((n) => (
                        <option key={n} value={n}>
                          {n}
                        </option>
                      ))}
                      <option value="custom">Custom</option>
                    </select>
                    {draft.fpsLimitChoice === "custom" && (
                      <input
                        type="number"
                        min={1}
                        className={styles.numberInput}
                        data-nav-item
                        disabled={!status.mangohudInstalled}
                        value={draft.fpsLimitCustom}
                        onChange={(e) =>
                          updateDraft({ fpsLimitCustom: Math.max(1, Number(e.target.value) || 1) })
                        }
                      />
                    )}
                  </div>
                </div>
              </section>

              <section className={styles.section}>
                <p className={styles.sectionTitle}>Overlay</p>
                {!status.mangohudInstalled ? (
                  <div className={styles.notInstalled}>
                    <div className={styles.notInstalledLabel}>MangoHud — Not installed</div>
                    <p className={styles.notInstalledHint}>
                      Install the <code>mangohud</code> package for your distro to see FPS and
                      system stats in-game.
                    </p>
                  </div>
                ) : (
                  <>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>MangoHud</span>
                      <Switch
                        checked={draft.overlayOn}
                        onChange={(v) => updateDraft({ overlayOn: v })}
                        label="Show performance overlay"
                      />
                    </div>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>Preset</span>
                      <select
                        className={styles.select}
                        data-nav-item
                        aria-label="Overlay preset"
                        disabled={!draft.overlayOn}
                        value={draft.preset}
                        onChange={(e) => updateDraft({ preset: e.target.value as Preset })}
                      >
                        <option value="minimal">Minimal</option>
                        <option value="standard">Standard</option>
                        <option value="detailed">Detailed</option>
                      </select>
                    </div>
                  </>
                )}
              </section>

              <section className={styles.section}>
                <p className={styles.sectionTitle}>Display</p>
                {!status.gamescopeInstalled ? (
                  <div className={styles.notInstalled}>
                    <div className={styles.notInstalledLabel}>Gamescope — Not installed</div>
                    <p className={styles.notInstalledHint}>
                      Install the <code>gamescope</code> package for your distro to run this game
                      in its own resolution with upscaling.
                    </p>
                  </div>
                ) : (
                  <>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>Gamescope</span>
                      <Switch
                        checked={draft.gamescopeEnabled}
                        onChange={(v) => updateDraft({ gamescopeEnabled: v })}
                        label="Enable Gamescope"
                      />
                    </div>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>Upscaler</span>
                      <select
                        className={styles.select}
                        data-nav-item
                        aria-label="Upscaler"
                        disabled={!draft.gamescopeEnabled}
                        value={draft.upscaler}
                        onChange={(e) => updateDraft({ upscaler: e.target.value as Upscaler })}
                      >
                        <option value="none">None</option>
                        <option value="fsr">FSR</option>
                        <option value="nis">NIS</option>
                      </select>
                    </div>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>Internal Resolution</span>
                      <div className={styles.resolutionControl}>
                        <input
                          type="number"
                          min={0}
                          className={styles.numberInput}
                          data-nav-item
                          aria-label="Internal width"
                          disabled={!draft.gamescopeEnabled}
                          placeholder="Auto"
                          value={draft.internalWidth || ""}
                          onChange={(e) => updateDraft({ internalWidth: Math.max(0, Number(e.target.value) || 0) })}
                        />
                        <span className={styles.resolutionTimes}>×</span>
                        <input
                          type="number"
                          min={0}
                          className={styles.numberInput}
                          data-nav-item
                          aria-label="Internal height"
                          disabled={!draft.gamescopeEnabled}
                          placeholder="Auto"
                          value={draft.internalHeight || ""}
                          onChange={(e) => updateDraft({ internalHeight: Math.max(0, Number(e.target.value) || 0) })}
                        />
                      </div>
                    </div>
                    <div className={styles.row}>
                      <span className={styles.rowLabel}>Output Resolution</span>
                      <div className={styles.resolutionControl}>
                        <input
                          type="number"
                          min={0}
                          className={styles.numberInput}
                          data-nav-item
                          aria-label="Output width"
                          disabled={!draft.gamescopeEnabled}
                          placeholder="Auto"
                          value={draft.outputWidth || ""}
                          onChange={(e) => updateDraft({ outputWidth: Math.max(0, Number(e.target.value) || 0) })}
                        />
                        <span className={styles.resolutionTimes}>×</span>
                        <input
                          type="number"
                          min={0}
                          className={styles.numberInput}
                          data-nav-item
                          aria-label="Output height"
                          disabled={!draft.gamescopeEnabled}
                          placeholder="Auto"
                          value={draft.outputHeight || ""}
                          onChange={(e) => updateDraft({ outputHeight: Math.max(0, Number(e.target.value) || 0) })}
                        />
                      </div>
                    </div>
                  </>
                )}
              </section>

              <section className={styles.section}>
                <p className={styles.sectionTitle}>Advanced</p>
                {hasLaunchOptions && (
                  <button
                    type="button"
                    className={styles.navRow}
                    data-nav-item
                    onClick={() => setSubView("launchOptions")}
                  >
                    <div className={styles.navRowLabel}>
                      <span className={styles.rowLabel}>Launch Options</span>
                      <span className={styles.navRowMeta}>
                        {status.steamConfigured ? "Configured in Steam" : "Setup needed"}
                      </span>
                    </div>
                    <span className={styles.navRowRight}>›</span>
                  </button>
                )}
                <button
                  type="button"
                  className={styles.navRow}
                  data-nav-item
                  onClick={() => setSubView("envVars")}
                >
                  <div className={styles.navRowLabel}>
                    <span className={styles.rowLabel}>Environment Variables</span>
                    <span className={styles.navRowMeta}>
                      {draft.envVars.length > 0 ? `${draft.envVars.length} set` : "None set"}
                    </span>
                  </div>
                  <span className={styles.navRowRight}>›</span>
                </button>
              </section>

              {error && <p className={styles.errorMessage}>{error}</p>}
            </>
          )}
        </div>

        <div className={styles.footer}>
          <button type="button" className={styles.resetButton} data-nav-item onClick={handleReset}>
            Reset to defaults
          </button>
          <button
            type="button"
            className={styles.applyButton}
            data-nav-item
            onClick={handleApply}
            disabled={!isDirty || applying}
          >
            {applying ? "Applying…" : "Apply"}
          </button>
        </div>
      </div>
    </>
  );
}
