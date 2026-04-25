//! Schedule types for daemon and cron-based workflow execution.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A scheduled task stored in git-refs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Unique ID.
    pub id: String,
    /// Cron expression (e.g. "*/15 * * * *").
    pub cron: String,
    /// Workflow to run.
    pub workflow: String,
    /// Task description (can contain {{variables}}).
    pub task: String,
    /// Agent persona to use.
    #[serde(default)]
    pub persona: Option<String>,
    /// LLM provider override.
    #[serde(default)]
    pub provider: Option<String>,
    /// Model override.
    #[serde(default)]
    pub model: Option<String>,
    /// Template variables for the task.
    #[serde(default)]
    pub variables: serde_json::Value,
    /// Last run timestamp.
    #[serde(default)]
    pub last_run: Option<DateTime<Utc>>,
    /// Next scheduled run (computed from cron).
    #[serde(default)]
    pub next_run: Option<DateTime<Utc>>,
    /// Whether this schedule is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Maximum concurrent runs allowed (0 = unlimited, 1 = exclusive/lock).
    #[serde(default)]
    pub max_concurrent: usize,
}

fn default_enabled() -> bool {
    true
}

impl ScheduledTask {
    /// Calculate next run time from cron expression.
    pub fn compute_next_run(&self) -> Option<DateTime<Utc>> {
        use cron::Schedule;
        self.cron
            .parse::<Schedule>()
            .ok()?
            .upcoming(Utc)
            .next()
    }
}
