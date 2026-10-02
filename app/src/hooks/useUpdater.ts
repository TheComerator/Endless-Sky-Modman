// Updates to the manager itself, through Tauri's updater plugin. Only updates signed with
// the project's own key are accepted (the public half is in tauri.conf.json), so a hijacked
// download link can't push code to users. Nothing is ever installed without a click.
import { relaunch } from "@tauri-apps/plugin-process";
import { type Update, check } from "@tauri-apps/plugin-updater";
import { useCallback, useEffect, useRef, useState } from "react";

import type { Notify } from "./useToasts";

const SETTING_KEY = "esmm.checkForUpdates";

/** On unless the user turned it off. Storage can be unavailable, so never throw. */
export function readAutoCheck(): boolean {
  try {
    return window.localStorage.getItem(SETTING_KEY) !== "off";
  } catch {
    return true;
  }
}

function writeAutoCheck(on: boolean) {
  try {
    window.localStorage.setItem(SETTING_KEY, on ? "on" : "off");
  } catch {
    // The choice just won't persist; the app still works.
  }
}

export type UpdaterStatus =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "available"; version: string; notes: string | null }
  | { phase: "installing"; percent: number | null };

export function useUpdater(notify: Notify) {
  const [status, setStatus] = useState<UpdaterStatus>({ phase: "idle" });
  const [autoCheck, setAutoCheckState] = useState(readAutoCheck);
  const pending = useRef<Update | null>(null);
  const dismissed = useRef<string | null>(null);

  /** `quiet`: the startup check, which says nothing when there's no update or no network. */
  const checkNow = useCallback(
    async (quiet: boolean) => {
      setStatus({ phase: "checking" });
      try {
        const update = await check();
        if (!update) {
          setStatus({ phase: "idle" });
          if (!quiet) notify("info", "You're on the latest version.");
          return;
        }
        pending.current = update;
        if (quiet && dismissed.current === update.version) {
          setStatus({ phase: "idle" });
          return;
        }
        setStatus({ phase: "available", version: update.version, notes: update.body ?? null });
      } catch (e) {
        setStatus({ phase: "idle" });
        if (!quiet) notify("warn", `Couldn't check for updates: ${String(e)}`);
      }
    },
    [notify],
  );

  useEffect(() => {
    if (autoCheck) void checkNow(true);
  }, [autoCheck, checkNow]);

  const setAutoCheck = useCallback((on: boolean) => {
    writeAutoCheck(on);
    setAutoCheckState(on);
  }, []);

  const dismiss = useCallback(() => {
    if (status.phase === "available") dismissed.current = status.version;
    setStatus({ phase: "idle" });
  }, [status]);

  const install = useCallback(async () => {
    const update = pending.current;
    if (!update) return;
    let total = 0;
    let done = 0;
    setStatus({ phase: "installing", percent: null });
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          setStatus({ phase: "installing", percent: total ? Math.min(100, Math.round((done / total) * 100)) : null });
        }
      });
      await relaunch();
    } catch (e) {
      setStatus({ phase: "idle" });
      notify("error", `The update failed, nothing was changed: ${String(e)}`);
    }
  }, [notify]);

  return { status, autoCheck, setAutoCheck, checkNow, install, dismiss };
}

export type Updater = ReturnType<typeof useUpdater>;
