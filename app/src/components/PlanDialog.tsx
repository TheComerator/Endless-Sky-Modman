// Decision C's "present ONE plan before anything is committed": every step, every blocking
// issue with the actions that can fix it, every optional-dependency note, and an explicit
// override that has to be ticked before a blocked plan can be committed.
import { useEffect, useState } from "react";

import type { IssueView } from "../bindings/IssueView";
import type { PlanView } from "../bindings/PlanView";
import type { RequirementFixView } from "../bindings/RequirementFixView";
import {
  commitLabel,
  describeIssue,
  describeNote,
  describeStep,
  formatBytes,
  planTitle,
} from "../format";
import type { PlanFlow, PlanRequest } from "../hooks/usePlanFlow";

function IssueActions({
  issue,
  plan,
  onResolve,
  onStart,
}: {
  issue: IssueView;
  plan: PlanView;
  onResolve: (identity: string) => void;
  onStart: (r: PlanRequest) => void;
}) {
  if (issue.kind === "conflict") {
    const sides = [issue.a, issue.b].filter((s) => plan.resolvableConflicts.includes(s));
    return (
      <>
        {sides.map((side) => (
          <button key={side} className="small" onClick={() => onResolve(side)}>
            Disable {side} instead
          </button>
        ))}
      </>
    );
  }
  if (issue.kind === "missingRequirement" || issue.kind === "ambiguousRequirement") {
    const fix: RequirementFixView | undefined = plan.fixes.find((f) => f.requires === issue.requires);
    if (!fix) return <span className="muted">Not in the catalog.</span>;
    if (fix.enable) {
      return (
        <button className="small" onClick={() => onStart({ kind: "enable", identity: fix.enable! })}>
          Enable {fix.enable} first
        </button>
      );
    }
    const options = fix.install ? [fix.install] : fix.candidates;
    return (
      <>
        {options.map((name) => (
          <button key={name} className="small" onClick={() => onStart({ kind: "install", catalogName: name })}>
            Install {name} first
          </button>
        ))}
      </>
    );
  }
  return null;
}

export function PlanDialog({ planFlow }: { planFlow: PlanFlow }) {
  const { flow, cancel, commit, resolveConflict, start } = planFlow;
  const [override, setOverride] = useState(false);
  const planId = flow.phase === "review" ? flow.plan.planId : null;

  // A new (or re-resolved) plan always starts un-overridden.
  const issueCount = flow.phase === "review" ? flow.plan.issues.length : 0;
  useEffect(() => setOverride(false), [planId, issueCount]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel]);

  if (flow.phase === "idle" || (flow.phase === "planning" && flow.quiet)) return null;

  if (flow.phase === "planning") {
    return (
      <div className="modal-backdrop">
        <div className="modal" role="dialog" aria-modal="true" aria-label={flow.title}>
          <h2>{flow.title}</h2>
          <p className="muted">
            {flow.downloads.length ? "Downloading and checking dependencies…" : "Checking…"}
          </p>
          {flow.downloads.map((d) => {
            const pct = d.total ? Math.min(100, (d.received / d.total) * 100) : null;
            return (
              <div key={d.catalogName} className="download">
                <div className="download-label">
                  <span>{d.catalogName}</span>
                  <span className="muted">
                    {formatBytes(d.received)}
                    {d.total ? ` / ${formatBytes(d.total)}` : ""}
                  </span>
                </div>
                <div className={`progress ${pct === null ? "indeterminate" : ""}`}>
                  <div style={{ width: pct === null ? "30%" : `${pct}%` }} />
                </div>
              </div>
            );
          })}
          {!flow.downloads.length && <div className="progress indeterminate"><div /></div>}
          <div className="modal-actions">
            <button onClick={() => void cancel()}>Cancel</button>
          </div>
        </div>
      </div>
    );
  }

  const { plan, committing, error } = flow;
  const blocked = plan.issues.length > 0;
  const canCommit = !committing && (!blocked || override);

  return (
    <div className="modal-backdrop">
      <div className="modal wide" role="dialog" aria-modal="true" aria-label={planTitle(plan)}>
        <h2>{planTitle(plan)}</h2>

        <section>
          <h3>This will</h3>
          {plan.steps.length ? (
            <ul className="steps">
              {plan.steps.map((step, i) => (
                <li key={i} className={`step step-${step.kind}`}>
                  {describeStep(step)}
                </li>
              ))}
            </ul>
          ) : (
            <p className="muted">
              {plan.kind === "applyProfile"
                ? "Change no plugins; it only makes this the active profile."
                : "Change nothing."}
            </p>
          )}
          {plan.unmanagedTarget && (
            <p className="callout warn">
              This plugin wasn't installed by the manager. Uninstalling deletes its folder from your
              plugins directory.
            </p>
          )}
        </section>

        {blocked && (
          <section className="issues">
            <h3>
              {plan.issues.length} problem{plan.issues.length === 1 ? "" : "s"} block this
            </h3>
            <ul>
              {plan.issues.map((issue, i) => {
                const { title, detail } = describeIssue(issue);
                return (
                  <li key={i} className="issue">
                    <div>
                      <strong>{title}.</strong> {detail}
                    </div>
                    <div className="issue-actions">
                      <IssueActions issue={issue} plan={plan} onResolve={resolveConflict} onStart={start} />
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        )}

        {plan.missing.length > 0 && (
          <section>
            <h3>Not installed</h3>
            <p className="muted">
              This profile enables plugins you don't have. Switching leaves them out; install them
              afterwards from the profile banner.
            </p>
            <ul className="plain">
              {plan.missing.map((m) => (
                <li key={m.identity}>
                  {m.identity}
                  {m.catalogName && m.catalogName !== m.identity ? ` (${m.catalogName})` : ""}
                </li>
              ))}
            </ul>
          </section>
        )}

        {plan.notes.length > 0 && (
          <section className="notes">
            <h3>Good to know</h3>
            <ul className="plain">
              {plan.notes.map((note, i) => (
                <li key={i}>{describeNote(note)}</li>
              ))}
            </ul>
          </section>
        )}

        {plan.gameVersion && (
          <p className="muted small-print">Checked against Endless Sky {plan.gameVersion}.</p>
        )}

        {error && <p className="callout error">{error.message}</p>}

        {blocked && (
          <label className="override">
            <input
              type="checkbox"
              checked={override}
              onChange={(e) => setOverride(e.target.checked)}
              disabled={committing}
            />
            I understand these problems and want to proceed anyway.
          </label>
        )}

        <div className="modal-actions">
          <button onClick={() => void cancel()} disabled={committing}>
            Cancel
          </button>
          <button
            className={blocked ? "danger" : plan.kind === "uninstall" ? "danger" : "primary"}
            disabled={!canCommit}
            onClick={() => void commit(plan, override)}
          >
            {committing ? "Working…" : error?.kind === "gameRunning" ? "Retry" : commitLabel(plan.kind, blocked)}
          </button>
        </div>
      </div>
    </div>
  );
}
