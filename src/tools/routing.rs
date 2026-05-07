use serde_json::{Value, json};

use crate::store::CanopyStore;
use crate::tools::ToolResult;

/// Rank agents for a task or assign it explicitly.
///
/// If `agent_id` is provided, assigns the task directly without ranking.
/// Otherwise, returns a ranked list of agents based on capability match and tier.
pub fn tool_task_assign_ranked(
    store: &(impl CanopyStore + ?Sized),
    agent_id: &str,
    args: &Value,
) -> ToolResult {
    let task_id = match super::validate_required_string(args, "task_id") {
        Ok(id) => id,
        Err(e) => return e,
    };

    let explicit_agent = super::get_str(args, "agent_id");

    if let Some(assigned_agent) = explicit_agent {
        // Explicit assignment mode
        match store.assign_task(task_id, assigned_agent, agent_id, None) {
            Ok(_) => ToolResult::json(&json!({
                "assigned": true,
                "agent_id": assigned_agent
            })),
            Err(e) => ToolResult::error(format!("assignment failed: {e}")),
        }
    } else {
        // Advisory ranking mode
        let required_tags = super::get_string_array(args, "required_tags");

        match store.rank_agents_for_task(task_id, &required_tags) {
            Ok(ranked) => {
                if ranked.is_empty() {
                    ToolResult::json(&json!({
                        "ranked": [],
                        "message": "no active agents available for ranking"
                    }))
                } else {
                    ToolResult::json(&json!({
                        "ranked": ranked,
                        "assign_hint": "pass agent_id to assign explicitly"
                    }))
                }
            }
            Err(e) => ToolResult::error(format!("ranking failed: {e}")),
        }
    }
}
