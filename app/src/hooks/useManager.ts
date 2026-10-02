// App-wide data, all owned by the backend: this hook only caches the latest snapshot of
// each and knows when to re-fetch (after every commit, on window focus, on a timer for the
// game process). There is no client-side model of plugin state to drift out of sync.
import { useCallback, useEffect, useRef, useState } from "react";

import { api, asCmdError } from "../api";
import type { CatalogView } from "../bindings/CatalogView";
import type { GameProcessView } from "../bindings/GameProcessView";
import type { InstallsView } from "../bindings/InstallsView";
import type { ManagerState } from "../bindings/ManagerState";
import type { Notify } from "./useToasts";

const GAME_POLL_MS = 4000;

export function useManager(notify: Notify) {
  const [catalog, setCatalog] = useState<CatalogView | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [state, setState] = useState<ManagerState | null>(null);
  const [stateError, setStateError] = useState<string | null>(null);
  const [installs, setInstalls] = useState<InstallsView | null>(null);
  const [game, setGame] = useState<GameProcessView>("unknown");
  const notifyRef = useRef(notify);
  notifyRef.current = notify;

  const refreshState = useCallback(async () => {
    try {
      const next = await api.getState();
      setState(next);
      setGame(next.game);
      setStateError(null);
      if (next.adopted.length > 0) {
        notifyRef.current(
          "info",
          `Linked ${next.adopted.length} existing plugin(s) to the catalog: ${next.adopted.join(", ")}.`,
        );
      }
      return next;
    } catch (e) {
      setStateError(asCmdError(e).message);
      return null;
    }
  }, []);

  const loadCatalog = useCallback(
    async (refresh: boolean) => {
      setCatalogLoading(true);
      try {
        const next = await api.loadCatalog(refresh);
        setCatalog(next);
        setCatalogError(null);
        if (refresh && next.source.kind === "offline") {
          notifyRef.current("warn", `Couldn't reach the catalog; showing the cached copy. (${next.source.error})`);
        }
        // Update status and adoption both depend on the catalog.
        const nextState = await refreshState();
        // Only on a user-requested refresh, not the silent load on mount: tell them
        // outright when there was nothing to find, since the alternative (an update
        // being available) is already obvious from the badge and "Update all" button.
        if (refresh && nextState && next.source.kind !== "offline") {
          const updates = nextState.plugins.filter((p) => p.update.kind === "available").length;
          if (updates === 0) notifyRef.current("info", "All mods are up to date.");
        }
      } catch (e) {
        setCatalogError(asCmdError(e).message);
      } finally {
        setCatalogLoading(false);
      }
    },
    [refreshState],
  );

  const refreshInstalls = useCallback(async (refresh: boolean) => {
    try {
      setInstalls(await api.listInstalls(refresh));
    } catch (e) {
      notifyRef.current("error", asCmdError(e).message);
    }
  }, []);

  /** After any change to which install is selected. */
  const onInstallsChanged = useCallback(
    async (next: InstallsView) => {
      setInstalls(next);
      await refreshState();
    },
    [refreshState],
  );

  useEffect(() => {
    void refreshState();
    void refreshInstalls(false);
    void loadCatalog(false);
  }, [refreshState, refreshInstalls, loadCatalog]);

  // The game rewrites plugins.txt itself, so coming back to the window re-reads everything.
  useEffect(() => {
    const onFocus = () => void refreshState();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshState]);

  useEffect(() => {
    const timer = window.setInterval(async () => {
      try {
        const next = await api.gameStatus();
        setGame((prev) => {
          // The game just exited: it may have rewritten plugins.txt (drift).
          if (prev === "running" && next !== "running") void refreshState();
          return next;
        });
      } catch {
        // Polling is best-effort; the next commit re-checks for real.
      }
    }, GAME_POLL_MS);
    return () => window.clearInterval(timer);
  }, [refreshState]);

  return {
    catalog,
    catalogLoading,
    catalogError,
    loadCatalog,
    state,
    stateError,
    refreshState,
    installs,
    refreshInstalls,
    onInstallsChanged,
    game,
  };
}

export type Manager = ReturnType<typeof useManager>;
