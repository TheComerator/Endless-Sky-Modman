import { getVersion } from "@tauri-apps/api/app";
import { useCallback, useEffect, useRef, useState } from "react";

import { api, asCmdError } from "./api";
import type { ManagerState } from "./bindings/ManagerState";
import type { MissingView } from "./bindings/MissingView";
import { PlanDialog } from "./components/PlanDialog";
import { PluginIcon } from "./components/PluginIcon";
import { Toasts } from "./components/Toasts";
import { formatAge } from "./format";
import { type Manager, useManager } from "./hooks/useManager";
import { type PlanFlow, usePlanFlow } from "./hooks/usePlanFlow";
import { type Updater, useUpdater } from "./hooks/useUpdater";
import { type Notify, useToasts } from "./hooks/useToasts";
import { BrowseView } from "./views/BrowseView";
import { InstalledView } from "./views/InstalledView";
import { ProfilesView } from "./views/ProfilesView";
import { KIND_LABEL, SettingsView } from "./views/SettingsView";

type Tab = "installed" | "browse" | "profiles" | "settings";

function TopBar({ manager, planFlow, notify }: { manager: Manager; planFlow: PlanFlow; notify: Notify }) {
  const { state, game } = manager;
  const install = state?.install ?? null;
  const profiles = state?.profiles;
  const running = game === "running";
  // The running app's own version, so a bug report or an update check can always be matched to it.
  const [appVersion, setAppVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(null));
  }, []);

  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true" />
        Endless Sky Mod Manager
        {appVersion && (
          <span className="app-version" title="The version of this app">
            v{appVersion}
          </span>
        )}
      </div>
      <div className="topbar-controls">
        <span className="chip" title={install?.configDir}>
          {install
            ? `${KIND_LABEL[install.kind]} · ${install.gameVersion ? `v${install.gameVersion}` : "version unknown"}`
            : "No game install"}
        </span>
        {profiles && profiles.active && (
          <label className="profile-select">
            <span className="muted">Profile</span>
            <select
              value={profiles.active}
              onChange={(e) => void planFlow.start({ kind: "applyProfile", name: e.target.value })}
              disabled={planFlow.flow.phase !== "idle"}
            >
              {profiles.profiles.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <button
          className="primary launch"
          disabled={!install?.launch || running}
          title={install && !install.launch ? "Add the game's executable in Settings to launch it from here" : undefined}
          onClick={async () => {
            try {
              await api.launchGame();
              notify("info", "Starting Endless Sky…");
            } catch (e) {
              notify("error", asCmdError(e).message);
            }
          }}
        >
          {running ? "Running" : "Launch game"}
        </button>
      </div>
    </header>
  );
}

function Banners({
  state,
  manager,
  planFlow,
  notify,
  goTo,
  downloadAllMissing,
  updater,
}: {
  state: ManagerState | null;
  manager: Manager;
  planFlow: PlanFlow;
  notify: Notify;
  goTo: (tab: Tab) => void;
  downloadAllMissing: (items: MissingView[]) => Promise<void>;
  updater: Updater;
}) {
  const { catalog, game, refreshState } = manager;
  const drift = state?.profiles.drift;
  const active = state?.profiles.active;
  const drifted = drift && (drift.enabledButNotInProfile.length > 0 || drift.inProfileButDisabled.length > 0);
  const missing = state?.profiles.missing ?? [];
  const busy = planFlow.flow.phase !== "idle";
  const installableMissing = missing.filter(
    (m) => m.catalogName && catalog?.entries.some((e) => e.name === m.catalogName),
  );

  return (
    <>
      {(updater.status.phase === "available" || updater.status.phase === "installing") && (
        <div className="banner info">
          <div>
            {updater.status.phase === "installing" ? (
              <>
                Downloading the update{updater.status.percent !== null ? ` (${updater.status.percent}%)` : ""}… the app
                will restart when it's done.
              </>
            ) : (
              <>
                <strong>Version {updater.status.version}</strong> of the mod manager is available.
                {game === "running" && " Close Endless Sky first."}
                {game !== "running" && busy && " Finish or cancel what you're doing first."}
              </>
            )}
          </div>
          {updater.status.phase === "available" && (
            <div className="banner-actions">
              <button className="small primary" disabled={busy || game === "running"} onClick={() => void updater.install()}>
                Update and restart
              </button>
              <button className="small" onClick={updater.dismiss}>
                Not now
              </button>
            </div>
          )}
        </div>
      )}
      {game === "running" && (
        <div className="banner warn">
          Endless Sky is running. Plugin changes are blocked until it exits, because the game
          rewrites its plugin list when it closes.
        </div>
      )}
      {catalog?.source.kind === "offline" && (
        <div className="banner info">
          Offline: showing the catalog cached {formatAge(catalog.fetchedAt)}. Enabling, disabling
          and profiles still work.
        </div>
      )}
      {drifted && active && (
        <div className="banner warn">
          <div>
            Your plugins changed outside the manager (probably in-game) and no longer match the
            profile <strong>{active}</strong>
            {drift.enabledButNotInProfile.length > 0 && <> · enabled: {drift.enabledButNotInProfile.join(", ")}</>}
            {drift.inProfileButDisabled.length > 0 && <> · disabled: {drift.inProfileButDisabled.join(", ")}</>}
          </div>
          <div className="banner-actions">
            <button
              className="small"
              onClick={async () => {
                try {
                  await api.updateActiveProfile();
                  await refreshState();
                } catch (e) {
                  notify("error", asCmdError(e).message);
                }
              }}
            >
              Update {active} to match
            </button>
            <button className="small" onClick={() => void planFlow.start({ kind: "applyProfile", name: active })}>
              Apply profile {active}
            </button>
            <button className="small" onClick={() => goTo("profiles")}>
              Save as new profile…
            </button>
          </div>
        </div>
      )}
      {missing.length > 0 && active && (
        <div className="banner info" style={{ flexDirection: "column", alignItems: "stretch" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <div>
              The profile <strong>{active}</strong> enables plugins that aren't installed:
            </div>
            {installableMissing.length > 1 && (
              <button className="small primary" disabled={busy} onClick={() => void downloadAllMissing(installableMissing)}>
                Download all {installableMissing.length}
              </button>
            )}
          </div>
          <ul className="rows" style={{ marginTop: 10 }}>
            {missing.map((m) => {
              const entry = m.catalogName ? catalog?.entries.find((e) => e.name === m.catalogName) : undefined;
              return (
                <li key={m.identity} className="row">
                  <PluginIcon url={entry?.iconUrl} size={36} />
                  <div className="row-text">
                    <div className="row-title">
                      <span className="name">{m.identity}</span>
                      {!entry && <span className="muted">not in the catalog</span>}
                    </div>
                  </div>
                  <div className="row-actions">
                    {entry && (
                      <button
                        className="small primary"
                        disabled={busy}
                        onClick={() => void planFlow.start({ kind: "install", catalogName: entry.name })}
                      >
                        Install
                      </button>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </>
  );
}

export default function App() {
  const { toasts, notify, dismiss } = useToasts();
  const manager = useManager(notify);
  const onCommitted = useCallback(async () => {
    await manager.refreshState();
  }, [manager.refreshState]);
  const planFlow = usePlanFlow(notify, onCommitted);
  const updater = useUpdater(notify);
  const [tab, setTab] = useState<Tab>("installed");
  const { state } = manager;

  // Always the latest phase, for a loop that waits on it from inside an async callback
  // (a plain closure over `planFlow.flow` would see only the snapshot from when it started).
  const flowPhaseRef = useRef(planFlow.flow.phase);
  flowPhaseRef.current = planFlow.flow.phase;

  // Installs one missing plugin at a time (the backend holds only one pending plan): each
  // `start` either commits quietly or opens a review dialog, which pauses this loop - sitting
  // at the `while` - until the user resolves it, before moving on to the next plugin.
  const downloadAllMissing = useCallback(
    async (items: MissingView[]) => {
      for (const m of items) {
        if (!m.catalogName) continue;
        await planFlow.start({ kind: "install", catalogName: m.catalogName });
        while (flowPhaseRef.current !== "idle") {
          await new Promise((r) => setTimeout(r, 150));
        }
      }
    },
    [planFlow],
  );

  const updates = state?.plugins.filter((p) => p.update.kind === "available").length ?? 0;
  const tabs: [Tab, string, string | null][] = [
    ["installed", "Installed", state?.plugins.length ? String(state.plugins.length) : null],
    ["browse", "Browse Plugins", null],
    ["profiles", "Profiles", null],
    ["settings", "Settings", null],
  ];

  return (
    <div className="app">
      <TopBar manager={manager} planFlow={planFlow} notify={notify} />
      <div className="body">
        <nav className="sidebar">
          {tabs.map(([id, label, count]) => (
            <button key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
              <span>{label}</span>
              {count && <span className="count">{count}</span>}
              {id === "installed" && updates > 0 && (
                <span className="count accent" title={`${updates} update(s) available`}>
                  ↑{updates}
                </span>
              )}
            </button>
          ))}
        </nav>
        <main>
          <Banners
            state={state}
            manager={manager}
            planFlow={planFlow}
            notify={notify}
            goTo={setTab}
            downloadAllMissing={downloadAllMissing}
            updater={updater}
          />
          {manager.stateError && <div className="banner error">{manager.stateError}</div>}
          {tab === "installed" && <InstalledView manager={manager} planFlow={planFlow} notify={notify} />}
          {tab === "browse" && <BrowseView manager={manager} planFlow={planFlow} />}
          {tab === "profiles" && <ProfilesView manager={manager} planFlow={planFlow} notify={notify} />}
          {tab === "settings" && <SettingsView manager={manager} notify={notify} updater={updater} />}
        </main>
      </div>
      <PlanDialog planFlow={planFlow} />
      <Toasts toasts={toasts} dismiss={dismiss} />
    </div>
  );
}
