import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { api, asCmdError } from "../api";
import type { InstallKindView } from "../bindings/InstallKindView";
import type { Manager } from "../hooks/useManager";
import type { Notify } from "../hooks/useToasts";

export const KIND_LABEL: Record<InstallKindView, string> = {
  standalone: "Standalone",
  steam: "Steam",
  flatpak: "Flatpak",
  custom: "Custom",
};

export function SettingsView({ manager, notify }: { manager: Manager; notify: Notify }) {
  const { installs, refreshInstalls, onInstallsChanged } = manager;
  const [configDir, setConfigDir] = useState("");
  const [executable, setExecutable] = useState("");
  const [detecting, setDetecting] = useState(false);

  const run = async (action: () => Promise<Awaited<ReturnType<typeof api.listInstalls>>>) => {
    try {
      await onInstallsChanged(await action());
      return true;
    } catch (e) {
      notify("error", asCmdError(e).message);
      return false;
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
          <p className="callout warn">No Endless Sky install was found. Add one below.</p>
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
        <h2>Add an install</h2>
        <p className="muted">
          For an AppImage or other manual install, give its executable. If you run the game with
          a custom <code>-c</code> config folder, give that folder too.
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
              <input value={configDir} onChange={(e) => setConfigDir(e.target.value)} placeholder="Optional: the default for your OS if empty" />
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
      </section>
    </div>
  );
}
