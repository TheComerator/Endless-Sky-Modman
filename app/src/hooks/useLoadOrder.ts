// The optional load-order feature's data: the Settings switch and the page's list. Delete with
// the feature (crates/esmm-core/src/load_order.rs lists everything involved).
import { useCallback, useEffect, useRef, useState } from "react";

import { api, asCmdError } from "../api";
import type { LoadOrderState } from "../bindings/LoadOrderState";
import type { LoadOrderSwitch } from "../bindings/LoadOrderSwitch";
import type { Notify } from "./useToasts";

export function useLoadOrder(notify: Notify) {
  const [state, setState] = useState<LoadOrderState | null>(null);
  /** The user's unsaved arrangement; kept here so leaving the page and coming back doesn't lose it. */
  const [draft, setDraft] = useState<string[] | null>(null);
  const notifyRef = useRef(notify);
  notifyRef.current = notify;

  const refresh = useCallback(async () => {
    try {
      const next = await api.loadOrderState();
      setState(next);
      return next;
    } catch {
      // The main state banner already reports a broken install; no second message here.
      return null;
    }
  }, []);

  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  /** Flips the Settings switch; returns what the backend reports (null if it failed). */
  const setEnabled = useCallback(
    async (enabled: boolean): Promise<LoadOrderSwitch | null> => {
      try {
        const result = await api.loadOrderSetEnabled(enabled);
        await refresh();
        return result;
      } catch (e) {
        notifyRef.current("error", asCmdError(e).message);
        return null;
      }
    },
    [refresh],
  );

  return { state, enabled: state?.enabled ?? false, refresh, setEnabled, draft, setDraft };
}

export type LoadOrder = ReturnType<typeof useLoadOrder>;
