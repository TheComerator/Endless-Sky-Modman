// Typed wrappers over the Tauri commands in src-tauri/src/commands.rs. Argument names are
// camelCase: Tauri maps them onto the Rust commands' snake_case parameters.
import { invoke } from "@tauri-apps/api/core";

import type { CatalogView } from "./bindings/CatalogView";
import type { CmdError } from "./bindings/CmdError";
import type { CommitView } from "./bindings/CommitView";
import type { GameProcessView } from "./bindings/GameProcessView";
import type { InstallsView } from "./bindings/InstallsView";
import type { ManagerState } from "./bindings/ManagerState";
import type { PlanView } from "./bindings/PlanView";

export const api = {
  loadCatalog: (refresh: boolean) => invoke<CatalogView>("load_catalog", { refresh }),
  getIcon: (url: string) => invoke<string>("get_icon", { url }),

  listInstalls: (refresh: boolean) => invoke<InstallsView>("list_installs", { refresh }),
  selectInstall: (key: string) => invoke<InstallsView>("select_install", { key }),
  addCustomInstall: (configDir: string | null, executable: string | null) =>
    invoke<InstallsView>("add_custom_install", { configDir, executable }),
  removeCustomInstall: (key: string) => invoke<InstallsView>("remove_custom_install", { key }),
  gameStatus: () => invoke<GameProcessView>("game_status"),
  launchGame: () => invoke<void>("launch_game"),

  getState: () => invoke<ManagerState>("get_state"),
  adoptPlugin: (folder: string, catalogName: string) =>
    invoke<void>("adopt_plugin", { folder, catalogName }),

  planInstall: (catalogName: string) => invoke<PlanView>("plan_install", { catalogName }),
  planUpdate: (folder: string) => invoke<PlanView>("plan_update", { folder }),
  planUpdateAll: () => invoke<PlanView>("plan_update_all"),
  planEnable: (identity: string) => invoke<PlanView>("plan_enable", { identity }),
  planDisable: (identity: string) => invoke<PlanView>("plan_disable", { identity }),
  planUninstall: (folder: string) => invoke<PlanView>("plan_uninstall", { folder }),
  planApplyProfile: (name: string) => invoke<PlanView>("plan_apply_profile", { name }),
  cancelPlanning: () => invoke<void>("cancel_planning"),
  resolveConflict: (planId: number, identity: string) =>
    invoke<PlanView>("resolve_conflict", { planId, identity }),
  discardPlan: (planId: number) => invoke<void>("discard_plan", { planId }),
  commitPlan: (planId: number, overrideIssues: boolean) =>
    invoke<CommitView>("commit_plan", { planId, overrideIssues }),

  createProfile: (name: string) => invoke<string>("create_profile", { name }),
  renameProfile: (oldName: string, newName: string) =>
    invoke<string>("rename_profile", { oldName, newName }),
  exportProfile: (name: string, path: string) => invoke<void>("export_profile", { name, path }),
  importProfile: (path: string) => invoke<string>("import_profile", { path }),
  updateActiveProfile: () => invoke<void>("update_active_profile"),
  deleteProfile: (name: string) => invoke<void>("delete_profile", { name }),
};

/** Every command rejects with a `CmdError`; Tauri's own argument errors arrive as strings. */
export function asCmdError(e: unknown): CmdError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    return e as CmdError;
  }
  return { kind: "io", message: typeof e === "string" ? e : String(e) };
}

export const DOWNLOAD_PROGRESS_EVENT = "download-progress";
