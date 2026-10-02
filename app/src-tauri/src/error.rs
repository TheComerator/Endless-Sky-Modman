//! The one error type every command returns, and how each core error maps onto it.
//!
//! Tauri serializes a command's `Err` and rejects the frontend's `invoke` promise with it, so
//! this is a plain tagged object (`{ kind, message, ... }`) rather than a string: the UI
//! branches on `kind` (e.g. a `blocked` commit re-shows the plan's issues, `gameRunning`
//! offers a retry) and always has a human-readable `message` to fall back on.

use esmm_core::catalog::CatalogError;
use esmm_core::game_state::WriteError;
use esmm_core::manager::CommitError;
use esmm_core::profiles::ProfileError;
use esmm_core::records::RecordsError;
use serde::Serialize;
use ts_rs::TS;

use crate::views::{CommitView, IssueView, issue_views};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum CmdError {
    /// No game install is selected (none detected, none added).
    NoInstall {
        message: String,
    },
    /// Endless Sky is running; it would overwrite `plugins.txt` on exit (decision F).
    GameRunning {
        message: String,
    },
    /// The plan still has blocking issues and no override was given (decision C).
    Blocked {
        message: String,
        issues: Vec<IssueView>,
    },
    /// The plan id isn't the pending plan any more (committed, discarded or superseded).
    PlanExpired {
        message: String,
    },
    /// The user cancelled planning (or a newer plan replaced it mid-download).
    Cancelled {
        message: String,
    },
    NotFound {
        message: String,
    },
    /// Bad input from the UI, e.g. an empty or duplicate profile name.
    Invalid {
        message: String,
    },
    /// One install in a multi-plugin plan failed; `partial` already happened and is on disk.
    InstallFailed {
        message: String,
        folder: String,
        partial: Box<CommitView>,
    },
    /// Fetching the catalog or an icon failed with nothing cached to fall back on.
    Network {
        message: String,
    },
    Io {
        message: String,
    },
}

impl CmdError {
    pub fn message(&self) -> &str {
        match self {
            CmdError::NoInstall { message }
            | CmdError::GameRunning { message }
            | CmdError::Blocked { message, .. }
            | CmdError::PlanExpired { message }
            | CmdError::Cancelled { message }
            | CmdError::NotFound { message }
            | CmdError::Invalid { message }
            | CmdError::InstallFailed { message, .. }
            | CmdError::Network { message }
            | CmdError::Io { message } => message,
        }
    }

    pub fn no_install() -> Self {
        CmdError::NoInstall {
            message: "No Endless Sky install is selected. Pick or add one in Settings.".into(),
        }
    }

    pub fn game_running() -> Self {
        CmdError::GameRunning {
            message: "Endless Sky is running. Close it first, or it will overwrite your plugin \
                      changes when it exits."
                .into(),
        }
    }

    pub fn plan_expired() -> Self {
        CmdError::PlanExpired {
            message: "That plan is no longer current. Start the action again.".into(),
        }
    }

    pub fn cancelled() -> Self {
        CmdError::Cancelled {
            message: "Cancelled.".into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        CmdError::NotFound {
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        CmdError::Invalid {
            message: message.into(),
        }
    }

    pub fn io(message: impl std::fmt::Display) -> Self {
        CmdError::Io {
            message: message.to_string(),
        }
    }

    pub fn blocked(issues: &[esmm_core::resolve::Issue]) -> Self {
        CmdError::Blocked {
            message: format!(
                "{} blocking issue(s) remain. Resolve them or choose to proceed anyway.",
                issues.len()
            ),
            issues: issue_views(issues),
        }
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for CmdError {}

impl From<CommitError> for CmdError {
    fn from(e: CommitError) -> Self {
        match e {
            CommitError::Blocked(issues) => CmdError::blocked(&issues),
            CommitError::GameRunning => CmdError::game_running(),
            CommitError::InstallFailed {
                report,
                folder,
                error,
            } => CmdError::InstallFailed {
                message: format!("Installing {folder:?} failed: {error}"),
                partial: Box::new(CommitView::from(report.as_ref())),
                folder,
            },
            CommitError::Uninstall(error) => CmdError::io(format!("Uninstall failed: {error}")),
            CommitError::Io(message) => CmdError::Io { message },
        }
    }
}

impl From<WriteError> for CmdError {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::GameRunning => CmdError::game_running(),
            WriteError::Io(e) => CmdError::io(format!("Failed to write plugins.txt: {e}")),
        }
    }
}

impl From<ProfileError> for CmdError {
    fn from(e: ProfileError) -> Self {
        match e {
            ProfileError::EmptyName
            | ProfileError::DuplicateName(_)
            | ProfileError::InvalidShareFile(_) => CmdError::invalid(e.to_string()),
            ProfileError::NotFound(_) | ProfileError::NoActiveProfile => {
                CmdError::not_found(e.to_string())
            }
            ProfileError::Io(_) | ProfileError::Json(_) => CmdError::io(e),
        }
    }
}

impl From<RecordsError> for CmdError {
    fn from(e: RecordsError) -> Self {
        CmdError::io(e)
    }
}

impl From<CatalogError> for CmdError {
    fn from(e: CatalogError) -> Self {
        match e {
            CatalogError::Io(_) => CmdError::io(e),
            CatalogError::Http(_) | CatalogError::Json(_) | CatalogError::Message(_) => {
                CmdError::Network {
                    message: e.to_string(),
                }
            }
        }
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::io(e)
    }
}

pub type CmdResult<T> = Result<T, CmdError>;
