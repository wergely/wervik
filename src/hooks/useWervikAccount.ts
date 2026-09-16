import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  connectWervikAccount,
  disconnectWervikAccount,
  getWervikAccount,
} from "../api/tauri";
import type { WervikAccount } from "../types";

export function useWervikAccount() {
  const [account, setAccount] = useState<WervikAccount | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getWervikAccount().then(setAccount).catch((reason) => {
      setError(reason instanceof Error ? reason.message : "Could not load Wervik Account.");
    });

    const updated = listen<WervikAccount>("wervik-account-updated", (event) => {
      setAccount(event.payload);
      setConnecting(false);
      setError(null);
    });
    const failed = listen<string>("wervik-account-error", (event) => {
      setConnecting(false);
      setError(event.payload);
    });
    const settingsFailed = listen<string>("wervik-settings-sync-error", (event) => {
      setError(event.payload);
    });

    return () => {
      updated.then((unlisten) => unlisten());
      failed.then((unlisten) => unlisten());
      settingsFailed.then((unlisten) => unlisten());
    };
  }, []);

  async function connect() {
    setConnecting(true);
    setError(null);
    try {
      await connectWervikAccount();
    } catch (reason) {
      setConnecting(false);
      setError(reason instanceof Error ? reason.message : "Could not open the Wervik Account login.");
    }
  }

  async function disconnect() {
    setError(null);
    try {
      await disconnectWervikAccount();
      setAccount(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not disconnect Wervik Account.");
    }
  }

  return { account, connecting, error, connect, disconnect };
}
