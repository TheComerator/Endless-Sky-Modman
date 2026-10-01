import { useMemo, useState } from "react";

import { api, asCmdError } from "../api";
import type { InstalledPluginView } from "../bindings/InstalledPluginView";
import type { UnmanagedView } from "../bindings/UnmanagedView";
import { PluginIcon } from "../components/PluginIcon";
import { shortVersion, updateLabel } from "../format";
import type { Manager } from "../hooks/useManager";
import type { PlanFlow } from "../hooks/usePlanFlow";
import type { Notify } from "../hooks/useToasts";

function LinkUnmanaged({ item, manager, notify }: { item: UnmanagedView; manager: Manager; notify: Notify }) {
  const [choice, setChoice] = useState(item.candidates[0] ?? "");
  return (
    <span className="link-unmanaged">
      <select value={choice} onChange={(e) => setChoice(e.target.value)} aria-label={`Catalog entry for ${item.identity}`}>
        {item.candidates.map((c) => (
          <option key={c} value={c}>
            {c}
          </option>
        ))}
      </select>
      <button
        className="small"
        onClick={async () => {
          try {
            await api.adoptPlugin(item.folder, choice);
            notify("success", `${item.identity} is now linked to ${choice}.`);
            await manager.refreshState();
          } catch (e) {
            notify("error", asCmdError(e).message);
          }
        }}
      >
        Link
      </button>
    </span>
  );
}

const UPDATE_HINT: Partial<Record<InstalledPluginView["update"]["kind"], string>> = {
  unknown: "Linked to the catalog, but which version you have is unknown. Reinstall to track updates.",
  unmanaged: "Not installed by the manager and not in the catalog. You can still enable, disable or remove it.",
  notInCatalog: "No longer in the catalog, so updates can't be checked.",
};

function DependencyLine({ plugin }: { plugin: InstalledPluginView }) {
  const parts: string[] = [];
  if (plugin.requires.length) parts.push(`requires ${plugin.requires.join(", ")}`);
  if (plugin.conflicts.length) parts.push(`conflicts with ${plugin.conflicts.join(", ")}`);
  if (plugin.optional.length) parts.push(`optional: ${plugin.optional.join(", ")}`);
  if (plugin.gameVersion) parts.push(`game ${plugin.gameVersion}+`);
  if (!parts.length) return null;
  return <div className="muted deps">{parts.join(" · ")}</div>;
}

export function InstalledView({
  manager,
  planFlow,
  notify,
}: {
  manager: Manager;
  planFlow: PlanFlow;
  notify: Notify;
}) {
  const { state, catalog, loadCatalog, catalogLoading } = manager;
  const [query, setQuery] = useState("");
  const busy = planFlow.flow.phase !== "idle";

  const iconFor = useMemo(() => {
    const map = new Map<string, string | null>();
    for (const e of catalog?.entries ?? []) map.set(e.name, e.iconUrl);
    return (p: InstalledPluginView) => (p.catalogName ? map.get(p.catalogName) : null);
  }, [catalog]);

  if (!state) return <p className="empty muted">Loading…</p>;
  if (!state.install) {
    return (
      <div className="empty">
        <p>No Endless Sky install was found.</p>
        <p className="muted">Add one in Settings to manage its plugins.</p>
      </div>
    );
  }

  const q = query.trim().toLowerCase();
  const plugins = state.plugins.filter(
    (p) => !q || p.identity.toLowerCase().includes(q) || p.folder.toLowerCase().includes(q),
  );
  const updates = state.plugins.filter((p) => p.update.kind === "available").length;
  // Unmanaged plugins with no catalog match just carry an "Unmanaged" badge; only the
  // ambiguous ones need the user's decision (decision B).
  const ambiguous = state.unmanaged.filter((u) => u.candidates.length > 0);

  return (
    <div className="view">
      <div className="toolbar">
        <input
          type="search"
          placeholder={`Filter ${state.plugins.length} installed plugins`}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <span className="muted toolbar-note">
          {state.plugins.filter((p) => p.enabled).length} enabled
          {updates ? ` · ${updates} update${updates === 1 ? "" : "s"} available` : ""}
        </span>
        {updates > 0 && (
          <button
            className="primary"
            disabled={busy}
            onClick={() => void planFlow.start({ kind: "updateAll" })}
          >
            Update all {updates}
          </button>
        )}
        <button onClick={() => void loadCatalog(true)} disabled={catalogLoading}>
          {catalogLoading ? "Checking…" : "Check for updates"}
        </button>
      </div>

      {ambiguous.length > 0 && (
        <div className="callout info">
          <strong>Which catalog plugin is this?</strong>
          <p className="muted">
            These plugins weren't installed by the manager and match more than one catalog entry.
            Linking one lets the manager track its updates.
          </p>
          <ul className="plain">
            {ambiguous.map((u) => (
              <li key={u.folder}>
                <span className="name">{u.identity}</span> <LinkUnmanaged item={u} manager={manager} notify={notify} />
              </li>
            ))}
          </ul>
        </div>
      )}

      {state.plugins.length === 0 ? (
        <p className="empty muted">No plugins installed yet. Find some under Browse.</p>
      ) : (
        <ul className="rows">
          {plugins.map((p) => {
            const label = updateLabel(p.update);
            return (
              <li key={p.folder} className={`row ${p.enabled ? "" : "disabled"}`}>
                <label className="switch" title={p.enabled ? "Disable" : "Enable"}>
                  <input
                    type="checkbox"
                    checked={p.enabled}
                    disabled={busy}
                    onChange={() =>
                      void planFlow.start(
                        p.enabled ? { kind: "disable", identity: p.identity } : { kind: "enable", identity: p.identity },
                      )
                    }
                    aria-label={`${p.enabled ? "Disable" : "Enable"} ${p.identity}`}
                  />
                  <span />
                </label>
                <PluginIcon url={iconFor(p)} size={36} />
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{p.identity}</span>
                    {p.folder !== p.identity && <span className="muted folder">{p.folder}/</span>}
                    <span className="version" title={p.installedVersion || p.pluginVersion || undefined}>
                      {shortVersion(p.installedVersion || p.pluginVersion || "")}
                    </span>
                    {label && (
                      <span
                        className={`badge ${p.update.kind === "available" ? "accent" : "neutral"}`}
                        title={UPDATE_HINT[p.update.kind]}
                      >
                        {label}
                      </span>
                    )}
                  </div>
                  <DependencyLine plugin={p} />
                </div>
                <div className="row-actions">
                  {(p.update.kind === "available" || p.update.kind === "unknown") && (
                    <button
                      className={p.update.kind === "available" ? "primary small" : "small"}
                      disabled={busy}
                      title={p.update.kind === "unknown" ? "Reinstall the catalog's current version so updates can be tracked" : undefined}
                      onClick={() => void planFlow.start({ kind: "update", folder: p.folder, label: p.identity })}
                    >
                      {p.update.kind === "available" ? "Update" : "Reinstall latest"}
                    </button>
                  )}
                  <button
                    className="small ghost-danger"
                    disabled={busy}
                    onClick={() => void planFlow.start({ kind: "uninstall", folder: p.folder, label: p.identity })}
                  >
                    Uninstall
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
