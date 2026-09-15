import { useEffect, useState } from "react";
import { getSettings, setSettings as saveSettings } from "../api/tauri";
import type { Settings } from "../types";

export function useSettings() {
  const [settings, setSettingsState] = useState<Settings | null>(null);

  useEffect(() => {
    getSettings().then(setSettingsState);
  }, []);

  const update = (partial: Partial<Settings>) => {
    setSettingsState((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...partial };
      saveSettings(next).catch(() => {});
      return next;
    });
  };

  return { settings, update };
}
