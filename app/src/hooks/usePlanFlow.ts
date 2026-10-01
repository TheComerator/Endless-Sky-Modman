// The plan -> review -> commit flow (decision C), as one state machine shared by every view.
//
// The backend holds the plan itself (it owns staged downloads); this hook holds only its
// latest PlanView and drives the transitions. One flow at a time, matching the backend's
// single pending plan: starting a new one (e.g. a "fix" button inside the dialog)
// supersedes the current one.
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

import { DOWNLOAD_PROGRESS_EVENT, api, asCmdError } from "../api";
import type { CmdError } from "../bindings/CmdError";
import type { CommitView } from "../bindings/CommitView";
import type { DownloadProgress } from "../bindings/DownloadProgress";
import type { PlanView } from "../bindings/PlanView";
import { needsReview, planTitle } from "../format";
import type { Notify } from "./useToasts";

export type PlanRequest =
  | { kind: "install"; catalogName: string }
  | { kind: "update"; folder: string; label: string }
  | { kind: "updateAll" }
  | { kind: "enable"; identity: string }
  | { kind: "disable"; identity: string }
  | { kind: "uninstall"; folder: string; label: string }
  | { kind: "applyProfile"; name: string };

export type Flow =
  | { phase: "idle" }
  /** `quiet`: an enable/disable toggle, shown inline rather than in a dialog unless it
   * turns out to need review. */
  | { phase: "planning"; title: string; downloads: DownloadProgress[]; quiet: boolean }
  | { phase: "review"; plan: PlanView; committing: boolean; error: CmdError | null };

function requestTitle(r: PlanRequest): string {
  switch (r.kind) {
    case "install":
      return `Install ${r.catalogName}`;
    case "update":
      return `Update ${r.label}`;
    case "updateAll":
      return "Update all";
    case "enable":
      return `Enable ${r.identity}`;
    case "disable":
      return `Disable ${r.identity}`;
    case "uninstall":
      return `Uninstall ${r.label}`;
    case "applyProfile":
      return `Switch to profile ${r.name}`;
  }
}

function runPlan(r: PlanRequest): Promise<PlanView> {
  switch (r.kind) {
    case "install":
      return api.planInstall(r.catalogName);
    case "update":
      return api.planUpdate(r.folder);
    case "updateAll":
      return api.planUpdateAll();
    case "enable":
      return api.planEnable(r.identity);
    case "disable":
      return api.planDisable(r.identity);
    case "uninstall":
      return api.planUninstall(r.folder);
    case "applyProfile":
      return api.planApplyProfile(r.name);
  }
}

function summarize(plan: PlanView, result: CommitView): string {
  const parts: string[] = [];
  if (result.installed.length) parts.push(`installed ${result.installed.join(", ")}`);
  if (plan.kind !== "install" && result.enabled.length) parts.push(`enabled ${result.enabled.join(", ")}`);
  if (result.disabled.length) parts.push(`${plan.kind === "uninstall" ? "removed" : "disabled"} ${result.disabled.join(", ")}`);
  const what = parts.length ? parts.join("; ") : "done";
  return `${planTitle(plan)}: ${what}.`;
}

export function usePlanFlow(notify: Notify, onCommitted: () => Promise<void>) {
  const [flow, setFlow] = useState<Flow>({ phase: "idle" });
  // Plan ids only ever increase, so progress events from an abandoned (cancelled or
  // superseded) download are recognizable by their lower id.
  const minPlanId = useRef(0);
  const highestSeen = useRef(0);
  const flowRef = useRef(flow);
  flowRef.current = flow;

  useEffect(() => {
    const unlisten = listen<DownloadProgress>(DOWNLOAD_PROGRESS_EVENT, (event) => {
      const p = event.payload;
      highestSeen.current = Math.max(highestSeen.current, p.planId);
      if (p.planId < minPlanId.current) return;
      setFlow((f) => {
        if (f.phase !== "planning") return f;
        const others = f.downloads.filter((d) => d.catalogName !== p.catalogName);
        return { ...f, downloads: [...others, p] };
      });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  const commit = useCallback(
    async (plan: PlanView, override: boolean, quiet = false) => {
      if (!quiet) setFlow({ phase: "review", plan, committing: true, error: null });
      try {
        const result = await api.commitPlan(plan.planId, override);
        setFlow({ phase: "idle" });
        // A quiet toggle's result is visible in the list itself.
        if (!quiet) notify("success", summarize(plan, result));
        if (result.gameDetectionFailed) {
          notify(
            "warn",
            "Couldn't check whether Endless Sky is running. If it is, it may undo this change when it exits.",
          );
        }
        for (const leftover of result.leftovers) {
          notify("warn", `The old copy couldn't be deleted and was left at ${leftover}.`);
        }
      } catch (e) {
        const err = asCmdError(e);
        if (err.kind === "gameRunning" || err.kind === "blocked") {
          // The backend kept the plan: let the user close the game, or override, and retry.
          setFlow({ phase: "review", plan, committing: false, error: err });
          return;
        }
        setFlow({ phase: "idle" });
        notify("error", err.message);
      }
      await onCommitted();
    },
    [notify, onCommitted],
  );

  const start = useCallback(
    async (request: PlanRequest) => {
      minPlanId.current = highestSeen.current + 1;
      const quiet = request.kind === "enable" || request.kind === "disable";
      setFlow({ phase: "planning", title: requestTitle(request), downloads: [], quiet });
      try {
        const plan = await runPlan(request);
        highestSeen.current = Math.max(highestSeen.current, plan.planId);
        if (!needsReview(plan)) {
          await commit(plan, false, true);
        } else {
          setFlow({ phase: "review", plan, committing: false, error: null });
        }
      } catch (e) {
        const err = asCmdError(e);
        // A cancelled run was either the user's choice or superseded by a newer request,
        // which already owns the dialog.
        if (err.kind === "cancelled") return;
        setFlow({ phase: "idle" });
        notify("error", err.message);
      }
    },
    [commit, notify],
  );

  const cancel = useCallback(async () => {
    const f = flowRef.current;
    setFlow({ phase: "idle" });
    if (f.phase === "planning") await api.cancelPlanning();
    if (f.phase === "review") await api.discardPlan(f.plan.planId);
  }, []);

  const resolveConflict = useCallback(
    async (identity: string) => {
      const f = flowRef.current;
      if (f.phase !== "review") return;
      try {
        const plan = await api.resolveConflict(f.plan.planId, identity);
        setFlow({ phase: "review", plan, committing: false, error: null });
      } catch (e) {
        notify("error", asCmdError(e).message);
      }
    },
    [notify],
  );

  return { flow, start, commit, cancel, resolveConflict };
}

export type PlanFlow = ReturnType<typeof usePlanFlow>;
