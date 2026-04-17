//! Annotation-aware dispatch policy for MCP tool calls.
//!
//! Tools carry three optional hint annotations declared in the MCP schema:
//!
//! - `readOnlyHint`: the tool does not mutate any coordination state
//! - `destructiveHint`: the tool permanently removes or closes records
//! - `idempotentHint`: calling the tool multiple times has the same effect as calling it once
//!
//! The active [`DispatchPolicy`] maps those hints to a [`DispatchDecision`].
//! By default, destructive tools are flagged for review; all others proceed.

use crate::runtime::DispatchDecision;

/// Annotation hints carried by a single MCP tool definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolAnnotations {
    /// The tool only reads state; it never mutates coordination records.
    pub read_only_hint: bool,
    /// The tool can permanently remove or irreversibly close records.
    pub destructive_hint: bool,
    /// Calling the tool multiple times is safe and has the same effect as once.
    pub idempotent_hint: bool,
}

/// The active dispatch policy.
///
/// `Default` mode (the only currently-supported mode) auto-allows read-only
/// tools and flags destructive tools for operator review.  All other tools
/// are allowed to proceed.
///
/// This type is marked `#[non_exhaustive]` so that additional policy modes can
/// be added in future versions without a breaking change to callers that match
/// on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DispatchPolicy {
    /// Standard policy: flag destructive operations, allow everything else.
    #[default]
    Default,
}

impl DispatchPolicy {
    /// Evaluate the policy for a tool with the given annotations.
    ///
    /// Returns [`DispatchDecision::Proceed`] when the call is auto-allowed, or
    /// [`DispatchDecision::FlagForReview`] when the call should be blocked and
    /// returned to the operator for explicit confirmation.
    #[must_use]
    pub fn evaluate(self, tool_name: &str, annotations: ToolAnnotations) -> DispatchDecision {
        match self {
            DispatchPolicy::Default => default_policy(tool_name, annotations),
        }
    }

    /// Human-readable description of the active policy rules.
    #[must_use]
    pub fn describe(self) -> PolicyDescription {
        match self {
            DispatchPolicy::Default => PolicyDescription {
                name: "default",
                read_only: "auto-allowed (Proceed)",
                destructive: "blocked (FlagForReview) — requires operator confirmation",
                other: "auto-allowed (Proceed)",
            },
        }
    }
}

/// A static description of a policy's rules, suitable for operator display.
#[derive(Debug)]
pub struct PolicyDescription {
    /// Short policy name.
    pub name: &'static str,
    /// Rule applied to `readOnlyHint=true` tools.
    pub read_only: &'static str,
    /// Rule applied to `destructiveHint=true` tools.
    pub destructive: &'static str,
    /// Rule applied to tools with no special hint.
    pub other: &'static str,
}

fn default_policy(tool_name: &str, annotations: ToolAnnotations) -> DispatchDecision {
    if annotations.destructive_hint {
        return DispatchDecision::FlagForReview {
            reason: format!(
                "tool `{tool_name}` is marked destructiveHint=true; \
                 operator confirmation required before proceeding"
            ),
        };
    }

    // Read-only and everything else proceeds automatically.
    DispatchDecision::Proceed
}

/// Return the [`ToolAnnotations`] for a named canopy MCP tool.
///
/// Tools not found in the registry (unknown names) return the zero-value
/// `ToolAnnotations` — no hints set — so they default to `Proceed` under the
/// standard policy.
#[must_use]
pub fn annotations_for_tool(name: &str) -> ToolAnnotations {
    match name {
        // ── Read-only ──────────────────────────────────────────────────────
        "canopy_whoami"
        | "canopy_situation"
        | "canopy_work_queue"
        | "canopy_task_get"
        | "canopy_task_list"
        | "canopy_task_snapshot"
        | "canopy_get_handoff_scope"
        | "canopy_files_check"
        | "canopy_files_list_locks"
        | "canopy_handoff_list"
        | "canopy_evidence_list"
        | "canopy_evidence_verify"
        | "canopy_council_show"
        | "canopy_outcome_list"
        | "canopy_outcome_show"
        | "canopy_outcome_summary"
        | "canopy_check_handoff_completeness" => ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
        },

        // ── Destructive ────────────────────────────────────────────────────
        "canopy_task_complete"
        | "canopy_handoff_complete"
        | "canopy_handoff_reject"
        | "canopy_task_update_status" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
        },

        // ── Idempotent (safe to retry, not read-only, not destructive) ─────
        "canopy_register"
        | "canopy_heartbeat"
        | "canopy_files_unlock"
        | "canopy_task_claim"
        | "canopy_task_yield"
        | "canopy_outcome_record" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: true,
        },

        // ── All other tools: no special hints → default Proceed ───────────
        _ => ToolAnnotations::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::DispatchDecision;

    #[test]
    fn read_only_tool_proceeds() {
        let annotations = annotations_for_tool("canopy_task_list");
        assert!(annotations.read_only_hint);
        let decision = DispatchPolicy::Default.evaluate("canopy_task_list", annotations);
        assert_eq!(decision, DispatchDecision::Proceed);
    }

    #[test]
    fn destructive_tool_flags_for_review() {
        let annotations = annotations_for_tool("canopy_task_complete");
        assert!(annotations.destructive_hint);
        let decision = DispatchPolicy::Default.evaluate("canopy_task_complete", annotations);
        assert!(
            matches!(decision, DispatchDecision::FlagForReview { .. }),
            "expected FlagForReview, got {decision:?}"
        );
    }

    #[test]
    fn idempotent_tool_proceeds() {
        let annotations = annotations_for_tool("canopy_heartbeat");
        assert!(annotations.idempotent_hint);
        assert!(!annotations.destructive_hint);
        let decision = DispatchPolicy::Default.evaluate("canopy_heartbeat", annotations);
        assert_eq!(decision, DispatchDecision::Proceed);
    }

    #[test]
    fn unknown_tool_proceeds() {
        let annotations = annotations_for_tool("canopy_nonexistent_tool");
        let decision = DispatchPolicy::Default.evaluate("canopy_nonexistent_tool", annotations);
        assert_eq!(decision, DispatchDecision::Proceed);
    }

    #[test]
    fn flag_for_review_reason_names_tool() {
        let annotations = annotations_for_tool("canopy_handoff_complete");
        let decision = DispatchPolicy::Default.evaluate("canopy_handoff_complete", annotations);
        if let DispatchDecision::FlagForReview { reason } = decision {
            assert!(reason.contains("canopy_handoff_complete"));
        } else {
            panic!("expected FlagForReview");
        }
    }

    #[test]
    fn policy_describe_returns_default_name() {
        let desc = DispatchPolicy::Default.describe();
        assert_eq!(desc.name, "default");
        assert!(desc.destructive.contains("FlagForReview"));
        assert!(desc.read_only.contains("Proceed"));
    }
}
