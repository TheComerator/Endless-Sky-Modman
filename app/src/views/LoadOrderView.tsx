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
  describeSuggestion,
  effectiveOrder,
  formatNumber,
  groupWinnerChanges,
  moveItem,
  moveToEnd,
  sameOrder,
} from "../loadOrder";

const EXAMPLES_PER_GROUP = 8;
const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), summary, [href], [tabindex]:not([tabindex="-1"])';

/**
 * What every dialog here needs: focus moves into it when it opens and goes back to where it was
 * when it closes, Tab stays inside, and Escape closes it (unless `canClose` is false). The
 * listener is registered once and reads the latest values through refs, so a re-render of the
 * page can't remove it in the middle of a key press. `returnTo` is the element that opened the
 * dialog, noted at click time (by the time the dialog mounts, a button that was disabled while
 * working has already lost the focus).
 */
function useDialog(onClose: () => void, canClose: boolean, returnTo?: { current: HTMLElement | null }) {
  const ref = useRef<HTMLDivElement>(null);
  const latest = useRef({ onClose, canClose });
  latest.current = { onClose, canClose };
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && latest.current.canClose) {
        e.stopPropagation();
        latest.current.onClose();
      } else if (e.key === "Tab" && ref.current) {
        const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
        if (items.length === 0) return;
        const first = items[0];
        const last = items[items.length - 1];
        const active = document.activeElement;
        if (e.shiftKey && (active === first || active === ref.current)) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && active === last) {
          e.preventDefault();
          first.focus();
        } else if (!ref.current.contains(active)) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      (returnTo?.current ?? previous)?.focus?.();
    };
  }, []);
  return ref;
}

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
  returnTo,
  onClose,
  onDone,
  onFailed,
}: {
  plan: LoadOrderPlanView;
  /** The order being applied; unused for a restore. */
  wanted: string[];
  gameRunning: boolean;
  /** The element to give the focus back to when the dialog closes. */
  returnTo: { current: HTMLElement | null };
  onClose: () => void;
  onDone: (renamed: number) => Promise<void>;
  /** A commit that failed may still have changed folders; the page reads them again. */
  onFailed: () => Promise<void>;
}) {
  const [committing, setCommitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const restore = plan.kind === "restore";
  const dialogRef = useDialog(onClose, !committing, returnTo);

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
      await onFailed();
    }
  };

  const count = plan.renames.length;
  const title = restore ? "Put the original folder names back" : "Set the load order";
  return (
    <div className="modal-backdrop">
      <div ref={dialogRef} tabIndex={-1} className="modal wide" role="dialog" aria-modal="true" aria-label={title}>
        <h2>{title}</h2>
        <section>
          <h3>This will</h3>
          {count === 0 ? (
            <p className="muted">Change nothing.</p>
          ) : (
            <ul className="steps steps-scroll">
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
              carry over: {plan.identityChanges.map((c) => `${c.from} becomes ${c.to}`).join("; ")}.
            </p>
          </section>
        )}

        {plan.changes.length > 0 && (
          <section>
            <h3>Which plugin wins would change</h3>
            <p className="muted">
              Where two switched-on plugins change the same thing, the one that loads last wins. This would change the
              winner in {plan.changes.length} place{plan.changes.length === 1 ? "" : "s"}:
            </p>
            <div className="changes-scroll">
              <WinnerChanges changes={plan.changes} />
            </div>
          </section>
        )}

        {gameRunning && (
          <p className="callout warn" role="alert">
            Endless Sky is running. Close it first; the folders can't be renamed while it runs.
          </p>
        )}
        {error && (
          <p className="callout error" role="alert">
            {error}
          </p>
        )}

        <div className="modal-actions modal-actions-sticky">
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
type PreviewStatus = "pending" | "ready" | "failed";

export function LoadOrderView({ loadOrder, manager, notify }: { loadOrder: LoadOrder; manager: Manager; notify: Notify }) {
  const { state, refresh, draft, setDraft } = loadOrder;
  const gameRunning = manager.game === "running";
  const plugins = useMemo(() => state?.plugins ?? [], [state]);
  const serverKey = plugins.map((p) => p.folder).join("\u0000");
  const serverOrder = useMemo(() => (serverKey ? serverKey.split("\u0000") : []), [serverKey]);
  // The unsaved arrangement is kept in the hook (so leaving the page doesn't lose it) and
  // used only while it still covers exactly the installed plugins.
  const order = useMemo(() => effectiveOrder(draft, serverOrder) as string[], [draft, serverOrder]);
  const setOrder = useCallback(
    (next: string[]) => setDraft(sameOrder(next, serverOrder) ? null : next),
    [setDraft, serverOrder],
  );
  const [preview, setPreview] = useState<LoadOrderPreview | null>(null);
  const [previewStatus, setPreviewStatus] = useState<PreviewStatus>("ready");
  const [review, setReview] = useState<Review | null>(null);
  const [busy, setBusy] = useState(false);
  const trigger = useRef<HTMLElement | null>(null);

  // Read the list again when the page opens and whenever the installed plugins change.
  useEffect(() => {
    void refresh();
  }, [refresh, manager.state]);

  const byFolder = useMemo(() => new Map(plugins.map((p) => [p.folder, p])), [plugins]);
  const dirty = !sameOrder(order, serverOrder);
  const hasUnnumbered = plugins.some((p) => p.number === null);

  // What reordering would do, updated a moment after each change. Until the answer arrives the
  // old banner is hidden and Apply is off, so the page never offers a stale verdict.
  const sequence = useRef(0);
  useEffect(() => {
    const mine = ++sequence.current;
    if (order.length === 0) {
      setPreview(null);
      setPreviewStatus("ready");
      return;
    }
    setPreviewStatus("pending");
    const timer = window.setTimeout(async () => {
      try {
        const next = await api.loadOrderPreview(order);
        if (mine === sequence.current) {
          setPreview(next);
          setPreviewStatus("ready");
        }
      } catch {
        if (mine === sequence.current) {
          setPreview(null);
          setPreviewStatus("failed");
        }
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [order]);
  const shown = previewStatus === "ready" ? preview : null;

  const notesFor = useMemo(() => {
    const map = new Map<string, LoadOrderDependencyNote[]>();
    for (const note of preview?.dependencyNotes ?? []) {
      map.set(note.plugin, [...(map.get(note.plugin) ?? []), note]);
    }
    return map;
  }, [preview]);

  const suggest = async () => {
    try {
      // Starts from the order on screen, not only the saved one.
      const suggestion = await api.loadOrderSuggest(order);
      if (suggestion.fixed.length === 0) {
        notify("info", "Every plugin already loads after the plugins it requires.");
        return;
      }
      setOrder(suggestion.order);
      notify("success", describeSuggestion(suggestion.fixed.length));
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  const plan = async (kind: "apply" | "restore") => {
    trigger.current = document.activeElement as HTMLElement | null;
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
      setDraft(null);
      notify("success", renamed === 0 ? "Nothing needed renaming." : `Renamed ${renamed} folder${renamed === 1 ? "" : "s"}.`);
      await refresh();
      await manager.refreshState();
    },
    [notify, refresh, manager, setDraft],
  );

  const onFailed = useCallback(async () => {
    await refresh();
    await manager.refreshState();
  }, [refresh, manager]);

  // Buttons at the end of the list stay focusable (aria-disabled, not disabled) so pressing one
  // doesn't throw the keyboard focus away.
  const move = (index: number, delta: -1 | 1) => {
    const target = index + delta;
    if (target < 0 || target >= order.length) return;
    setOrder(moveItem(order, index, delta));
  };

  if (!state || !manager.state) return <p className="empty muted">Loading…</p>;
  if (!manager.state.install) return <p className="empty muted">Select a game install in Settings first.</p>;

  const canApply =
    !busy && !gameRunning && (dirty || hasUnnumbered) && previewStatus === "ready" && !shown?.problem;
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
        <button onClick={() => setOrder(serverOrder as string[])} disabled={!dirty}>
          Undo changes
        </button>
        <button className="primary" disabled={!canApply} onClick={() => void plan("apply")}>
          Apply…
        </button>
        <button disabled={busy || gameRunning || state.renamedCount === 0} onClick={() => void plan("restore")}>
          Restore original names
        </button>
        {gameRunning && <span className="muted toolbar-note">Close Endless Sky to change the order.</span>}
      </div>

      <div className="preview-slot">
        {previewStatus === "failed" && (
          <p className="callout warn" role="alert">
            Couldn't check this order, so it can't be applied right now. Move a plugin again to retry.
          </p>
        )}
        {shown?.problem && (
          <div className="banner error" role="alert">
            This order can't be applied: {shown.problem}
          </div>
        )}
        {shown && shown.changes.length > 0 && (
          <div className="banner info preview-banner" role="status">
            <details>
              <summary>
                <strong>
                  This order changes who wins in {shown.changes.length} place{shown.changes.length === 1 ? "" : "s"}.
                </strong>{" "}
                <span className="muted">Only switched-on plugins are compared. Click to see the details.</span>
              </summary>
              <WinnerChanges changes={shown.changes} />
            </details>
          </div>
        )}
      </div>

      {plugins.length === 0 ? (
        <p className="empty muted">No plugin installed by the manager can be ordered yet.</p>
      ) : (
        <ul className="rows">
          {order.map((folder, i) => {
            const p = byFolder.get(folder);
            if (!p) return null;
            const notes = notesFor.get(p.identity) ?? [];
            return (
              <li key={folder} className={`row order-row ${p.enabled ? "" : "disabled"}`}>
                <span className="order-pos mono muted">{i + 1}</span>
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{p.identity}</span>
                    <span
                      className="badge neutral"
                      title={
                        p.number === null
                          ? "No number yet: it loads after the numbered plugins until you apply"
                          : "The number in its folder name"
                      }
                    >
                      {p.number === null ? "no number yet" : formatNumber(p.number)}
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
                    aria-disabled={i === 0}
                    aria-label={`Move ${p.identity} to the top (loads first)`}
                    title="Move to the top"
                    onClick={() => setOrder(moveToEnd(order, i, "top"))}
                  >
                    ⤒
                  </button>
                  <button
                    className="small"
                    aria-disabled={i === 0}
                    aria-label={`Move ${p.identity} up (loads earlier)`}
                    onClick={() => move(i, -1)}
                  >
                    ▲
                  </button>
                  <button
                    className="small"
                    aria-disabled={i === order.length - 1}
                    aria-label={`Move ${p.identity} down (loads later)`}
                    onClick={() => move(i, 1)}
                  >
                    ▼
                  </button>
                  <button
                    className="small"
                    aria-disabled={i === order.length - 1}
                    aria-label={`Move ${p.identity} to the bottom (loads last, wins)`}
                    title="Move to the bottom"
                    onClick={() => setOrder(moveToEnd(order, i, "bottom"))}
                  >
                    ⤓
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {state.excluded.length > 0 && (
        <section className="order-excluded">
          <h2>Not part of the order</h2>
          <p className="muted">
            These load by their folder name, as before, after the numbered plugins. They're left alone so nothing about
            them changes.
          </p>
          <ul className="rows">
            {state.excluded.map((e) => (
              <li key={e.folder} className={`row order-row ${e.enabled ? "" : "disabled"}`}>
                <div className="row-text">
                  <div className="row-title">
                    <span className="name">{e.identity}</span>
                    {e.stillRenamed && <span className="badge neutral">Numbered</span>}
                  </div>
                  {e.folder !== e.identity && <div className="muted folder">{e.folder}</div>}
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
          returnTo={trigger}
          onClose={() => setReview(null)}
          onDone={onDone}
          onFailed={onFailed}
        />
      )}
    </div>
  );
}

/** Offered when the switch is turned off while folders are still numbered. */
function RestoreOffer({
  renamed,
  returnTo,
  onKeep,
  onRestore,
}: {
  renamed: number;
  returnTo: { current: HTMLElement | null };
  onKeep: () => void;
  onRestore: () => void;
}) {
  const dialogRef = useDialog(onKeep, true, returnTo);
  return (
    <div className="modal-backdrop">
      <div
        ref={dialogRef}
        tabIndex={-1}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label="Restore original folder names?"
      >
        <h2>Put the original folder names back?</h2>
        <p className="muted">
          {renamed} plugin folder{renamed === 1 ? " has" : "s have"} a number in front of {renamed === 1 ? "its" : "their"} name.
          You can restore the original names now, or keep the numbers (the plugins keep loading in that order). You can
          restore them later from here.
        </p>
        <div className="modal-actions">
          <button onClick={onKeep}>Keep the numbers</button>
          <button className="primary" onClick={onRestore}>
            Restore original names
          </button>
        </div>
      </div>
    </div>
  );
}

/** The Settings section: the switch, a short explanation, and the offer to restore. */
export function LoadOrderSettings({ loadOrder, manager, notify }: { loadOrder: LoadOrder; manager: Manager; notify: Notify }) {
  const { state, enabled, setEnabled, refresh } = loadOrder;
  const [offer, setOffer] = useState(false);
  const [review, setReview] = useState<LoadOrderPlanView | null>(null);
  // Shown ticked or unticked straight away, while the backend saves the choice.
  const [pending, setPending] = useState<boolean | null>(null);
  const renamed = state?.renamedCount ?? 0;
  const trigger = useRef<HTMLElement | null>(null);

  const toggle = async (on: boolean) => {
    trigger.current = document.activeElement as HTMLElement | null;
    setPending(on);
    const result = await setEnabled(on);
    setPending(null);
    // Turning it off with folders still numbered offers to put the original names back.
    if (result && !on && result.renamedCount > 0) setOffer(true);
  };

  const startRestore = async () => {
    setOffer(false);
    trigger.current = trigger.current ?? (document.activeElement as HTMLElement | null);
    try {
      setReview(await api.loadOrderPlanRestore());
    } catch (e) {
      notify("error", asCmdError(e).message);
    }
  };

  return (
    <section>
      <h2>Plugin load order</h2>
      <label>
        <input
          type="checkbox"
          checked={pending ?? enabled}
          aria-describedby="load-order-help"
          disabled={pending !== null}
          onChange={(e) => void toggle(e.target.checked)}
        />{" "}
        Manage plugin load order
      </label>
      <p id="load-order-help" className="muted">
        The game loads plugins by the order of their folder names. Turn this on to get a Load order page where you
        choose the order; the manager does it by numbering the folders of plugins it installed. It's off unless you turn
        it on.
      </p>
      {!enabled && renamed > 0 && (
        <p className="callout info">
          {renamed} plugin folder{renamed === 1 ? " still has" : "s still have"} a number in front of {renamed === 1 ? "its" : "their"} name.{" "}
          <button className="small" onClick={() => void startRestore()}>
            Restore original names
          </button>
        </p>
      )}

      {offer && (
        <RestoreOffer
          renamed={renamed}
          returnTo={trigger}
          onKeep={() => setOffer(false)}
          onRestore={() => void startRestore()}
        />
      )}

      {review && (
        <ReviewDialog
          plan={review}
          wanted={[]}
          gameRunning={manager.game === "running"}
          returnTo={trigger}
          onClose={() => setReview(null)}
          onFailed={async () => {
            await refresh();
            await manager.refreshState();
          }}
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
