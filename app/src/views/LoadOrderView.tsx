// The optional "Load order" page, its review dialog and its Settings switch, all in this one
// file so the feature can be removed by deleting it (crates/esmm-core/src/load_order.rs lists
// everything involved). The game loads plugins in the order of their folder names, and the
// plugin loaded last wins where two plugins set the same thing; the manager lets the user pick
// the order by putting a number in front of the folder names of plugins it installed.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, asCmdError } from "../api";
import type { LoadOrderDependencyNote } from "../bindings/LoadOrderDependencyNote";
import type { LoadOrderPlanView } from "../bindings/LoadOrderPlanView";
import type { LoadOrderPreview } from "../bindings/LoadOrderPreview";
import type { LoadOrder } from "../hooks/useLoadOrder";
import type { Manager } from "../hooks/useManager";
import type { Notify } from "../hooks/useToasts";
import {
  describeDependencyNote,
  describeExclusion,
  describeGroup,
  formatNumber,
  groupWinnerChanges,
  moveItem,
  sameOrder,
} from "../loadOrder";

const EXAMPLES_PER_GROUP = 8;

function WinnerChanges({ changes }: { changes: LoadOrderPlanView["changes"] }) {
  const groups = useMemo(() => groupWinnerChanges(changes), [changes]);
  if (groups.length === 0) return null;
  return (
    <ul className="plain">
      {groups.map((g) => (
        <li key={`${g.now}|${g.was}`}>
          {describeGroup(g)}
          <details>
            <summary className="muted">Show why</summary>
            <ul className="plain">
              {g.examples.slice(0, EXAMPLES_PER_GROUP).map((c) => (
                <li key={c.item}>
                  <strong>{c.item}</strong>
                  {c.reason && <span className="muted"> {c.reason}</span>}
                </li>
              ))}
              {g.examples.length > EXAMPLES_PER_GROUP && (
                <li className="muted">…and {g.examples.length - EXAMPLES_PER_GROUP} more.</li>
              )}
            </ul>
          </details>
        </li>
      ))}
    </ul>
  );
}

/** Shows exactly which folders would be renamed, and commits only on the user's say-so. */
function ReviewDialog({
  plan,
  wanted,
  gameRunning,
  onClose,
  onDone,
}: {
  plan: LoadOrderPlanView;
  /** The order being applied; unused for a restore. */
  wanted: string[];
  gameRunning: boolean;
  onClose: () => void;
  onDone: (renamed: number) => Promise<void>;
}) {
  const [committing, setCommitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const restore = plan.kind === "restore";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !committing) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [committing, onClose]);

  const commit = async () => {
    setCommitting(true);
    setError(null);
    try {
      const done = restore
        ? await api.loadOrderCommitRestore(plan.token)
        : await api.loadOrderCommitApply(wanted, plan.token);
      await onDone(done.renamed);
    } catch (e) {
      setError(asCmdError(e).message);
      setCommitting(false);
    }
  };

  const count = plan.renames.length;
  const title = restore ? "Put the original folder names back" : "Set the load order";
  return (
    <div className="modal-backdrop">
      <div className="modal wide" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        <section>
          <h3>This will</h3>
          {count === 0 ? (
            <p className="muted">Change nothing.</p>
          ) : (
            <ul className="steps">
              {plan.renames.map((r) => (
                <li key={r.from} className="step step-update">
                  Rename <span className="folder">{r.from}</span> to <span className="folder">{r.to}</span>{" "}
                  <span className="muted">({r.identity})</span>
                </li>
              ))}
            </ul>
          )}
          <p className="callout info">
            Only folder names change. Nothing inside any plugin is touched, and your enabled plugins and profiles carry
            on as they are, because they refer to plugin names, not folders. The manager keeps a record of every
            original name so this can be undone.
          </p>
        </section>

        {plan.loadsAmong.length > 0 && (
          <section>
            <h3>Good to know</h3>
            <p className="callout warn">
              These plugins aren't part of the order, but their folder names make them load before or among the
              numbered ones: {plan.loadsAmong.join(", ")}.
            </p>
          </section>
        )}

        {plan.identityChanges.length > 0 && (
          <section>
            <h3>Names that change</h3>
            <p className="callout warn">
              These plugins have no name in their plugin.txt, so the game knows them by their folder name. Putting the
              original folder name back changes that name, so their on/off setting (saved under the old name) won't
              carry over:{" "}
              {plan.identityChanges.map((c) => `${c.from} becomes ${c.to}`).join("; ")}.
            </p>
          </section>
        )}

        {plan.changes.length > 0 && (
          <section>
            <h3>Who wins changes</h3>
            <p className="muted">
              Where two switched-on plugins change the same thing, the one that loads last wins. This would change the
              winner in {plan.changes.length} place{plan.changes.length === 1 ? "" : "s"}:
            </p>
            <WinnerChanges changes={plan.changes} />
          </section>
        )}

        {gameRunning && (
          <p className="callout warn">Endless Sky is running. Close it first; the folders can't be renamed while it runs.</p>
        )}
        {error && <p className="callout error">{error}</p>}

        <div className="modal-actions">
          <button onClick={onClose} disabled={committing}>
            Cancel
          </button>
          <button className="primary" disabled={committing || gameRunning || count === 0} onClick={() => void commit()}>
            {committing
              ? "Working…"
              : restore
                ? `Restore ${count} folder${count === 1 ? "" : "s"}`
                : `Rename ${count} folder${count === 1 ? "" : "s"}`}
          </button>
        </div>
      </div>
    </div>
  );
}

type Review = { plan: LoadOrderPlanView; wanted: string[] };

export function LoadOrderView({ loadOrder, manager, notify }: { loadOrder: LoadOrder; manager: Manager; notify: Notify }) {
  const { state, refresh } = loadOrder;
  const gameRunning = manager.game === "running";
  const plugins = useMemo(() => state?.plugins ?? [], [state]);
  const serverKey = plugins.map((p) => p.folder).join("\u0000");
  const serverOrder = useMemo(() => (serverKey ? serverKey.split("\u0000") : []), [serverKey]);
  const [order, setOrder] = useState<string[]>(serverOrder);
  const [preview, setPreview] = useState<LoadOrderPreview | null>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [busy, setBusy] = useState(false);

  // Read the list again when the page opens and whenever the installed plugins change.
  useEffect(() => {
    void refresh();
  }, [refresh, manager.state]);

  // The saved order is the starting point; it is replaced when the plugins on disk change.
  useEffect(() => setOrder(serverOrder), [serverOrder]);

  const byFolder = useMemo(() => new Map(plugins.map((p) => [p.folder, p])), [plugins]);
  const dirty = !sameOrder(order, serverOrder);
  const hasUnnumbered = plugins.some((p) => p.number === null);

  // What reordering would do, updated a moment after each change.
  const sequence = useRef(0);
  useEffect(() => {
    if (order.length === 0) {
      setPreview(null);
      return;
    }
    const mine = ++sequence.current;
    const timer = window.setTimeout(async () => {
      try {
        const next = await api.loadOrderPreview(order);
        if (mine === sequence.current) setPreview(next);
      } catch {
        if (mine === sequence.current) setPreview(null);
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [order]);

  const notesFor = useMemo(() => {
    const map = new Map<string, LoadOrderDependencyNote[]>();
    for (const note of preview?.dependencyNotes ?? []) {
      map.set(note.plugin, [...(map.get(note.plugin) ?? []), note]);
    }
    return map;
  }, [preview]);

  const suggest = async () => {
    try {
      const suggestion = await api.loadOrderSuggest();
      if (suggestion.fixed.length === 0) {
        notify("info", "Every plugin already loads after the plugins it requires.");
        return;
      }
      setOrder(suggestion.order);
      notify(
        "success",
        `Moved plugins so ${suggestion.fixed.length} requirement${suggestion.fixed.length === 1 ? "" : "s"} load first. Nothing changes until you apply.`,
      );
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  const plan = async (kind: "apply" | "restore") => {
    setBusy(true);
    try {
      const next = kind === "apply" ? await api.loadOrderPlanApply(order) : await api.loadOrderPlanRestore();
      setReview({ plan: next, wanted: order });
    } catch (e) {
      notify("error", asCmdError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const onDone = useCallback(
    async (renamed: number) => {
      setReview(null);
      notify("success", renamed === 0 ? "Nothing needed renaming." : `Renamed ${renamed} folder${renamed === 1 ? "" : "s"}.`);
      await refresh();
      await manager.refreshState();
    },
    [notify, refresh, manager],
  );

  if (!state) return <p className="empty muted">Loading…</p>;
  if (!manager.state?.install) return <p className="empty muted">Select a game install in Settings first.</p>;

  const canApply = !busy && !gameRunning && (dirty || hasUnnumbered) && !preview?.problem;
  return (
    <div className="view">
      <p className="muted intro">
        When two plugins change the same ship, outfit or image, the one that loads last wins. The game loads plugins in
        the order of their folder names, so to let you choose, the manager puts a number in front of the folder names of
        plugins it installed (like <span className="folder">010-Name</span>). Move plugins up or down, then apply. The
        top of the list loads first; the bottom wins.
      </p>

      <div className="toolbar">
        <button onClick={() => void suggest()} title="Put plugins after the plugins they require, changing as little else as possible">
          Suggest order from dependencies
        </button>
        <button onClick={() => setOrder(serverOrder)} disabled={!dirty}>
          Undo changes
        </button>
        <button className="primary" disabled={!canApply} onClick={() => void plan("apply")}>
          Apply…
        </button>
        <button disabled={busy || state.renamedCount === 0} onClick={() => void plan("restore")}>
          Restore original names
        </button>
        {gameRunning && <span className="muted toolbar-note">Close Endless Sky to change the order.</span>}
      </div>

      {preview?.problem && <div className="banner error">This order can't be applied: {preview.problem}</div>}
      {preview && preview.changes.length > 0 && (
        <div className="banner info" style={{ flexDirection: "column", alignItems: "stretch" }}>
          <div>
            <strong>This order changes who wins in {preview.changes.length} place{preview.changes.length === 1 ? "" : "s"}.</strong>{" "}
            Only switched-on plugins are compared.
          </div>
          <WinnerChanges changes={preview.changes} />
        </div>
      )}

      {plugins.length === 0 ? (
        <p className="empty muted">No plugin installed by the manager can be ordered yet.</p>
      ) : (
        <ul className="rows">
          {order.map((folder, i) => {
            const p = byFolder.get(folder);
            if (!p) return null;
            const notes = notesFor.get(p.identity) ?? [];
            return (
              <li key={folder} className={`row ${p.enabled ? "" : "disabled"}`}>
                <span className="order-pos mono muted">{i + 1}</span>
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{p.identity}</span>
                    <span
                      className="badge neutral"
                      title={p.number === null ? "No number yet: it loads after the numbered plugins until you apply" : "The number in its folder name"}
                    >
                      {formatNumber(p.number)}
                    </span>
                    {!p.enabled && <span className="badge neutral">Off</span>}
                  </div>
                  <div className="muted folder">{p.folder}</div>
                  {notes.map((n) => (
                    <div key={n.requires} className="deps order-warn">
                      {describeDependencyNote(n)}
                    </div>
                  ))}
                </div>
                <div className="row-actions">
                  <button
                    className="small"
                    disabled={i === 0}
                    aria-label={`Move ${p.identity} up (loads earlier)`}
                    onClick={() => setOrder(moveItem(order, i, -1))}
                  >
                    ▲
                  </button>
                  <button
                    className="small"
                    disabled={i === order.length - 1}
                    aria-label={`Move ${p.identity} down (loads later)`}
                    onClick={() => setOrder(moveItem(order, i, 1))}
                  >
                    ▼
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {state.excluded.length > 0 && (
        <section>
          <h2>Not part of the order</h2>
          <p className="muted">
            These load by their folder name, as before, after the numbered plugins. They're left alone so nothing about
            them changes.
          </p>
          <ul className="rows">
            {state.excluded.map((e) => (
              <li key={e.folder} className={`row ${e.enabled ? "" : "disabled"}`}>
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{e.identity}</span>
                    {e.stillRenamed && <span className="badge neutral">Numbered</span>}
                  </div>
                  <div className="muted folder">{e.folder}</div>
                  <div className="muted deps">{describeExclusion(e.reasons, e.stillRenamed)}</div>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      {review && (
        <ReviewDialog
          plan={review.plan}
          wanted={review.wanted}
          gameRunning={gameRunning}
          onClose={() => setReview(null)}
          onDone={onDone}
        />
      )}
    </div>
  );
}

/** The Settings section: the switch, a one-line explanation, and the offer to restore. */
export function LoadOrderSettings({ loadOrder, manager, notify }: { loadOrder: LoadOrder; manager: Manager; notify: Notify }) {
  const { state, enabled, setEnabled, refresh } = loadOrder;
  const [offer, setOffer] = useState(false);
  const [review, setReview] = useState<LoadOrderPlanView | null>(null);
  const renamed = state?.renamedCount ?? 0;

  const toggle = async (on: boolean) => {
    const result = await setEnabled(on);
    // Turning it off with folders still numbered offers to put the original names back.
    if (result && !on && result.renamedCount > 0) setOffer(true);
  };

  const startRestore = async () => {
    setOffer(false);
    try {
      setReview(await api.loadOrderPlanRestore());
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  return (
    <section>
      <h2>Plugin load order</h2>
      <label className="muted">
        <input type="checkbox" checked={enabled} onChange={(e) => void toggle(e.target.checked)} /> Manage plugin load
        order. The game loads plugins by the order of their folder names; this adds a Load order page that lets you
        choose, by numbering the folders of plugins the manager installed. Off by default.
      </label>
      {!enabled && renamed > 0 && (
        <p className="callout info">
          {renamed} plugin folder{renamed === 1 ? " still has" : "s still have"} a number in front of {renamed === 1 ? "its" : "their"} name.{" "}
          <button className="small" onClick={() => void startRestore()}>
            Restore original names
          </button>
        </p>
      )}

      {offer && (
        <div className="modal-backdrop">
          <div className="modal" role="dialog" aria-modal="true" aria-label="Restore original folder names?">
            <h2>Put the original folder names back?</h2>
            <p className="muted">
              {renamed} plugin folder{renamed === 1 ? " has" : "s have"} a number in front of {renamed === 1 ? "its" : "their"} name.
              You can restore the original names now, or keep the numbers (the plugins keep loading in that order). You
              can restore them later from here.
            </p>
            <div className="modal-actions">
              <button onClick={() => setOffer(false)}>Keep the numbers</button>
              <button className="primary" onClick={() => void startRestore()}>
                Restore original names
              </button>
            </div>
          </div>
        </div>
      )}

      {review && (
        <ReviewDialog
          plan={review}
          wanted={[]}
          gameRunning={manager.game === "running"}
          onClose={() => setReview(null)}
          onDone={async (n) => {
            setReview(null);
            notify("success", n === 0 ? "Nothing needed renaming." : `Restored ${n} folder name${n === 1 ? "" : "s"}.`);
            await refresh();
            await manager.refreshState();
          }}
        />
      )}
    </section>
  );
}
