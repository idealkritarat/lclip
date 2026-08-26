//! Shared diagnostic result shape for `lcp doctor` (spec §17.2). Real checks land in Phase 5.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticResult {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
    pub detail: String,
    pub suggested_action: Option<String>,
}
