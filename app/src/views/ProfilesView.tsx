import { useState } from "react";

import { api, asCmdError } from "../api";
import type { Manager } from "../hooks/useManager";
import type { PlanFlow } from "../hooks/usePlanFlow";
import type { Notify } from "../hooks/useToasts";

export function ProfilesView({ manager, planFlow, notify }: { manager: Manager; planFlow: PlanFlow; notify: Notify }) {
  const { state, refreshState } = manager;
  const [name, setName] = useState("");
  if (!state?.install) return <p className="empty muted">Select a game install in Settings first.</p>;
  const { profiles } = state;

  const create = async () => {
    try {
      const stored = await api.createProfile(name);
      notify("success", `Saved the current plugins as ${stored}, now the active profile.`);
      setName("");
      await refreshState();
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  return (
    <div className="view">
      <p className="muted intro">
        A profile is a set of enabled plugins. Switching profiles only enables and disables
        plugins; it never installs or deletes anything. Changes you make while a profile is
        active are saved to it.
      </p>
      <ul className="rows">
        {profiles.profiles.map((p) => {
          const active = p.name === profiles.active;
          return (
            <li key={p.name} className="row">
              <div className="row-text">
                <div className="row-title">
                  <span className="name">{p.name}</span>
                  {active && <span className="badge accent">Active</span>}
                </div>
                <div className="muted">
                  {p.enabledCount} enabled plugin{p.enabledCount === 1 ? "" : "s"}
                </div>
              </div>
              <div className="row-actions">
                <button
                  className={active ? "small" : "primary small"}
                  onClick={() => void planFlow.start({ kind: "applyProfile", name: p.name })}
                >
                  {active ? "Re-apply" : "Switch"}
                </button>
                {!active && (
                  <button
                    className="small ghost-danger"
                    onClick={async () => {
                      if (!window.confirm(`Delete the profile ${p.name}? Its plugins stay installed.`)) return;
                      try {
                        await api.deleteProfile(p.name);
                        await refreshState();
                      } catch (e) {
                        notify("error", asCmdError(e).message);
                      }
                    }}
                  >
                    Delete
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <input placeholder="New profile name" value={name} onChange={(e) => setName(e.target.value)} />
        <button type="submit" className="primary" disabled={!name.trim()}>
          Save current plugins as new profile
        </button>
      </form>
    </div>
  );
}
