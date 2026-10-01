import { openUrl } from "@tauri-apps/plugin-opener";
import { useMemo, useState } from "react";

import type { CatalogEntryView } from "../bindings/CatalogEntryView";
import type { InstalledPluginView } from "../bindings/InstalledPluginView";
import { PluginIcon } from "../components/PluginIcon";
import { formatAge, shortVersion } from "../format";
import type { Manager } from "../hooks/useManager";
import type { PlanFlow } from "../hooks/usePlanFlow";

type Filter = "all" | "notInstalled" | "installed";

function matches(entry: CatalogEntryView, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return [entry.name, entry.authors, entry.shortDescription].some((s) => s.toLowerCase().includes(q));
}

export function BrowseView({ manager, planFlow }: { manager: Manager; planFlow: PlanFlow }) {
  const { catalog, catalogError, catalogLoading, loadCatalog, state } = manager;
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [expanded, setExpanded] = useState<string | null>(null);

  const installedByCatalog = useMemo(() => {
    const map = new Map<string, InstalledPluginView>();
    for (const p of state?.plugins ?? []) if (p.catalogName) map.set(p.catalogName, p);
    return map;
  }, [state]);

  const entries = useMemo(() => {
    const all = catalog?.entries ?? [];
    return all
      .filter((e) => matches(e, query.trim()))
      .filter((e) => {
        if (filter === "installed") return installedByCatalog.has(e.name);
        if (filter === "notInstalled") return !installedByCatalog.has(e.name);
        return true;
      })
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  }, [catalog, query, filter, installedByCatalog]);

  if (!catalog) {
    return (
      <div className="empty">
        {catalogError ? (
          <>
            <p>Couldn't load the plugin catalog: {catalogError}</p>
            <button onClick={() => void loadCatalog(true)} disabled={catalogLoading}>
              Try again
            </button>
          </>
        ) : (
          <p className="muted">Loading the plugin catalog…</p>
        )}
      </div>
    );
  }

  const noInstall = !state?.install;

  return (
    <div className="view">
      <div className="toolbar">
        <input
          type="search"
          placeholder={`Search ${catalog.entries.length} plugins`}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
        <div className="segmented" role="group" aria-label="Filter">
          {(
            [
              ["all", "All"],
              ["notInstalled", "Not installed"],
              ["installed", "Installed"],
            ] as const
          ).map(([value, label]) => (
            <button key={value} className={filter === value ? "active" : ""} onClick={() => setFilter(value)}>
              {label}
            </button>
          ))}
        </div>
        <span className="muted toolbar-note">
          {catalog.source.kind === "offline" ? "Offline copy, " : "Catalog "}
          updated {formatAge(catalog.fetchedAt)}
        </span>
        <button onClick={() => void loadCatalog(true)} disabled={catalogLoading}>
          {catalogLoading ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      {entries.length === 0 && <p className="empty muted">No plugins match.</p>}

      <ul className="cards">
        {entries.map((entry) => {
          const installed = installedByCatalog.get(entry.name);
          const isOpen = expanded === entry.name;
          return (
            <li key={entry.name} className={`card ${isOpen ? "open" : ""}`}>
              <button className="card-main" onClick={() => setExpanded(isOpen ? null : entry.name)} aria-expanded={isOpen}>
                <PluginIcon url={entry.iconUrl} />
                <div className="card-text">
                  <div className="card-title">
                    <span className="name">{entry.name}</span>
                    <span className="version" title={entry.version}>
                      {shortVersion(entry.version)}
                    </span>
                  </div>
                  <div className="muted authors">by {entry.authors}</div>
                  <div className="summary">{entry.shortDescription}</div>
                </div>
              </button>
              <div className="card-side">
                {installed ? (
                  installed.update.kind === "available" ? (
                    <button
                      className="primary"
                      onClick={() =>
                        void planFlow.start({ kind: "update", folder: installed.folder, label: installed.identity })
                      }
                    >
                      Update
                    </button>
                  ) : (
                    <span className="badge ok">Installed</span>
                  )
                ) : (
                  <button
                    className="primary"
                    disabled={noInstall}
                    title={noInstall ? "Select a game install in Settings first" : undefined}
                    onClick={() => void planFlow.start({ kind: "install", catalogName: entry.name })}
                  >
                    Install
                  </button>
                )}
              </div>
              {isOpen && (
                <div className="card-detail">
                  {entry.description ? <p className="description">{entry.description}</p> : null}
                  <dl>
                    <dt>Version</dt>
                    <dd>{entry.version}</dd>
                    <dt>License</dt>
                    <dd>{entry.license}</dd>
                    <dt>Homepage</dt>
                    <dd>
                      <a
                        href={entry.homepage}
                        onClick={(e) => {
                          e.preventDefault();
                          void openUrl(entry.homepage);
                        }}
                      >
                        {entry.homepage}
                      </a>
                    </dd>
                  </dl>
                  <p className="muted small-print">
                    Dependencies are listed inside the plugin itself, so they're checked when you
                    install it, before anything is changed.
                  </p>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
