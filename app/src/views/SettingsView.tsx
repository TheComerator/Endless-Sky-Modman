import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { api, asCmdError } from "../api";
import type { InstallKindView } from "../bindings/InstallKindView";
import type { LoadOrder } from "../hooks/useLoadOrder"; // load order
import type { Manager } from "../hooks/useManager";
import type { Notify } from "../hooks/useToasts";
import type { Updater } from "../hooks/useUpdater";
import { LoadOrderSettings } from "./LoadOrderView"; // load order

export const KIND_LABEL: Record<InstallKindView, string> = {
  standalone: "Standalone",
  steam: "Steam",
  flatpak: "Flatpak",
  custom: "Custom",
};

export function SettingsView({
  manager,
  notify,
  updater,
  loadOrder,
}: {
  manager: Manager;
  notify: Notify;
  updater: Updater;
  loadOrder: LoadOrder;
}) {
  const { installs, refreshInstalls, onInstallsChanged } = manager;
  const [configDir, setConfigDir] = useState("");
  const [executable, setExecutable] = useState("");
  const [detecting, setDetecting] = useState(false);
  const [gameFolder, setGameFolder] = useState("");

  const run = async (action: () => Promise<Awaited<ReturnType<typeof api.listInstalls>>>) => {
    try {
      await onInstallsChanged(await action());
      return true;
    } catch (e) {
      notify("error", asCmdError(e).message);
      return false;
    }
  };

  /** Adds the install in `folder`: the backend finds the executable, the kind and the config folder. */
  const addFolder = async (folder: string) => {
    try {
      const installs = await api.addGameFolder(folder);
      await onInstallsChanged(installs);
      const added = installs.installs.find((i) => i.key === installs.selected);
      setGameFolder("");
      notify(
        "success",
        added
          ? `Found ${KIND_LABEL[added.kind]} Endless Sky${added.gameVersion ? ` v${added.gameVersion}` : ""}. Its plugins are in ${added.configDir}.`
          : "Game folder added.",
      );
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  /** Browse… for the game folder; choosing one uses it straight away. */
  const pickGameFolder = async () => {
    const chosen = await open({ directory: true, multiple: false, title: "Choose the folder Endless Sky is installed in" });
    if (typeof chosen === "string") {
      setGameFolder(chosen);
      await addFolder(chosen);
    }
  };

  const pick = async (directory: boolean) => {
    const chosen = await open({ directory, multiple: false });
    if (typeof chosen === "string") (directory ? setConfigDir : setExecutable)(chosen);
  };

  return (
    <div className="view">
      <section>
        <div className="section-head">
          <h2>Updates</h2>
          <button disabled={updater.status.phase === "checking" || updater.status.phase === "installing"} onClick={() => void updater.checkNow(false)}>
            {updater.status.phase === "checking" ? "Checking…" : "Check now"}
          </button>
        </div>
        <label className="muted">
          <input type="checkbox" checked={updater.autoCheck} onChange={(e) => updater.setAutoCheck(e.target.checked)} />{" "}
          Check for a new version of the mod manager when it starts. This contacts GitHub; nothing else is sent, and
          nothing is ever installed without your click.
        </label>
      </section>
      <LoadOrderSettings loadOrder={loadOrder} manager={manager} notify={notify} />
      <section>
        <div className="section-head">
          <h2>Game installs</h2>
          <button
            disabled={detecting}
            onClick={async () => {
              setDetecting(true);
              await refreshInstalls(true);
              setDetecting(false);
            }}
          >
            {detecting ? "Detecting…" : "Detect again"}
          </button>
        </div>
        <p className="muted">
          The manager changes plugins for the selected install. Installs that share a config
          folder share their plugins.
        </p>
        {!installs ? (
          <p className="muted">Detecting…</p>
        ) : installs.installs.length === 0 ? (
          <p className="callout warn">No Endless Sky install was found. Pick the game folder below.</p>
        ) : (
          <ul className="rows">
            {installs.installs.map((i) => (
              <li key={i.key} className="row">
                <input
                  type="radio"
                  name="install"
                  checked={installs.selected === i.key}
                  onChange={() => void run(() => api.selectInstall(i.key))}
                  aria-label={`Use ${KIND_LABEL[i.kind]} install at ${i.configDir}`}
                />
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{KIND_LABEL[i.kind]}</span>
                    <span className="version">{i.gameVersion ? `v${i.gameVersion}` : "version unknown"}</span>
                    {i.userAdded && <span className="badge neutral">Added by you</span>}
                  </div>
                  <div className="muted mono">{i.configDir}</div>
                  <div className="muted">
                    {i.launch ? `Launches via ${i.launch}` : "No known way to launch; add its executable below."}
                  </div>
                </div>
                {i.userAdded && (
                  <div className="row-actions">
                    <button className="small ghost-danger" onClick={() => void run(() => api.removeCustomInstall(i.key))}>
                      Remove
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <h2>Game Folder</h2>
        <p className="muted">
          Pick the folder Endless Sky is installed in and the manager finds the rest by itself: the game, whether it's
          a Steam copy, and the folder where its plugins and settings live. If you pick a folder that holds the game
          (for example your Steam library, <span className="mono">Program Files</span> or{" "}
          <span className="mono">Applications</span>), it looks inside. A Flatpak install is found automatically, so it
          needs no folder.
        </p>
        <form
          className="stacked-form"
          onSubmit={async (e) => {
            e.preventDefault();
            await addFolder(gameFolder);
          }}
        >
          <label>
            Game folder
            <span className="input-with-button">
              <input
                value={gameFolder}
                onChange={(e) => setGameFolder(e.target.value)}
                placeholder="The folder Endless Sky is installed in"
              />
              <button type="button" onClick={() => void pickGameFolder()}>
                Browse…
              </button>
            </span>
          </label>
          <div>
            <button type="submit" className="primary" disabled={!gameFolder.trim()}>
              Use this folder
            </button>
          </div>
        </form>

        <details className="advanced">
          <summary className="muted">Advanced: give the game's executable or settings folder yourself</summary>
          <p className="muted">
            For an AppImage or other manual install, give its executable. If you run the game with a custom{" "}
            <code>-c</code> config folder, give that folder too.
          </p>
          <form
            className="stacked-form"
            onSubmit={async (e) => {
              e.preventDefault();
              const ok = await run(() => api.addCustomInstall(configDir || null, executable || null));
              if (ok) {
                setConfigDir("");
                setExecutable("");
                notify("success", "Install added and selected.");
              }
            }}
          >
            <label>
              Game executable
              <span className="input-with-button">
                <input value={executable} onChange={(e) => setExecutable(e.target.value)} placeholder="Optional" />
                <button type="button" onClick={() => void pick(false)}>
                  Browse…
                </button>
              </span>
            </label>
            <label>
              Config folder
              <span className="input-with-button">
                <input
                  value={configDir}
                  onChange={(e) => setConfigDir(e.target.value)}
                  placeholder="Optional: the default for your OS if empty"
                />
                <button type="button" onClick={() => void pick(true)}>
                  Browse…
                </button>
              </span>
            </label>
            <div>
              <button type="submit" className="primary" disabled={!configDir.trim() && !executable.trim()}>
                Add install
              </button>
            </div>
          </form>
        </details>
      </section>
    </div>
  );
}
