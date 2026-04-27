use serde_json::{Value, json};

use crate::tools::policy::annotations_for_tool;

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn tool_definitions() -> Vec<Value> {
    let mut tools = Vec::new();

    // ─────────────────────────────────────────────────────────────────────────
    // Identity & Lifecycle (4)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_register",
        "Register or update this agent's capabilities, role, and worktree. Call on session start.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Stable identifier for this agent (e.g. claude-implementer-1)"
                },
                "host_id": {
                    "type": "string",
                    "description": "Host process or container ID"
                },
                "host_type": {
                    "type": "string",
                    "description": "Host type (e.g. claude-code, api)"
                },
                "host_instance": {
                    "type": "string",
                    "description": "Instance discriminator within host_type"
                },
                "model": {
                    "type": "string",
                    "description": "Model name (e.g. claude-sonnet-4-6)"
                },
                "project_root": {
                    "type": "string",
                    "description": "Absolute path to the project root"
                },
                "worktree_id": {
                    "type": "string",
                    "description": "Git worktree identifier (main for primary)"
                },
                "role": {
                    "type": "string",
                    "enum": ["orchestrator", "implementer", "validator"],
                    "description": "Agent role in the coordination system"
                },
                "capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Skill tags this agent can handle (e.g. rust, frontend)"
                }
            },
            "required": ["agent_id", "host_id", "host_type", "host_instance", "model", "project_root", "worktree_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_heartbeat",
        "Send a heartbeat to update agent liveness and current task. Call every ~10 tool calls.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent identifier"
                },
                "status": {
                    "type": "string",
                    "enum": ["idle", "assigned", "in_progress", "blocked", "review_required"],
                    "description": "Current agent status"
                },
                "current_task_id": {
                    "type": "string",
                    "description": "ULID of the task currently being worked on, if any"
                }
            },
            "required": ["agent_id", "status"]
        }),
    ));

    tools.push(tool_def(
        "canopy_whoami",
        "Retrieve the registered profile and current status for this agent.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent identifier to look up"
                }
            },
            "required": ["agent_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_situation",
        "Get a full situational snapshot: this agent's active task, pending handoffs, and queue depth.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent identifier"
                },
                "project_root": {
                    "type": "string",
                    "description": "Scope snapshot to a project root"
                }
            },
            "required": ["agent_id"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Queue (3)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_work_queue",
        "List tasks available for this agent to claim, filtered by role and capabilities.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent identifier used to filter by role and capabilities"
                },
                "project_root": {
                    "type": "string",
                    "description": "Filter to tasks in this project"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 10,
                    "description": "Maximum number of tasks to return"
                }
            },
            "required": ["agent_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_claim",
        "Atomically claim an open task. Fails if another agent claimed it first. Always check canopy_work_queue first.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to claim"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent claiming the task"
                },
                "force_claim": {
                    "type": "boolean",
                    "description": "Bypass heartbeat freshness checks for manual operator intervention"
                }
            },
            "required": ["task_id", "agent_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_yield",
        "Release a claimed task back to the open queue without completing it.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to yield"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent releasing the task"
                },
                "reason": {
                    "type": "string",
                    "description": "Why the task is being yielded"
                }
            },
            "required": ["task_id", "agent_id"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Task (8)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_task_create",
        "Create a new top-level task or subtask in the coordination queue.",
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short title for the task"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of what needs to be done"
                },
                "requested_by": {
                    "type": "string",
                    "description": "Agent or user ID requesting the task"
                },
                "project_root": {
                    "type": "string",
                    "description": "Absolute path to the project root"
                },
                "parent_task_id": {
                    "type": "string",
                    "description": "ULID of parent task if this is a subtask"
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "critical"],
                    "description": "Task priority"
                },
                "required_role": {
                    "type": "string",
                    "enum": ["orchestrator", "implementer", "validator"],
                    "description": "Role required to work on this task"
                },
                "required_capabilities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Capability tags required"
                },
                "auto_review": {
                    "type": "boolean",
                    "description": "Automatically request review on completion"
                },
                "verification_required": {
                    "type": "boolean",
                    "description": "Task requires explicit verification before close"
                },
                "workflow_id": {
                    "type": "string",
                    "description": "Workflow instance this task belongs to (optional)"
                },
                "phase_id": {
                    "type": "string",
                    "description": "Workflow phase this task is currently in (optional)"
                }
            },
            "required": ["title", "requested_by", "project_root"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_decompose",
        "Decompose a task into subtasks. Creates all subtasks in a single atomic operation.",
        json!({
            "type": "object",
            "properties": {
                "parent_task_id": {
                    "type": "string",
                    "description": "ULID of the parent task to decompose"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent performing the decomposition"
                },
                "subtasks": {
                    "type": "array",
                    "description": "List of subtasks to create",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "description": { "type": "string" },
                            "priority": {
                                "type": "string",
                                "enum": ["low", "medium", "high", "critical"]
                            },
                            "required_role": {
                                "type": "string",
                                "enum": ["orchestrator", "implementer", "validator"]
                            },
                            "required_capabilities": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        },
                        "required": ["title"]
                    }
                }
            },
            "required": ["parent_task_id", "agent_id", "subtasks"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_get",
        "Retrieve full details for a single task including history, evidence, and council messages.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to retrieve"
                }
            },
            "required": ["task_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_list",
        "List tasks with optional filters. Use canopy_task_snapshot for dashboard views.",
        json!({
            "type": "object",
            "properties": {
                "project_root": {
                    "type": "string",
                    "description": "Filter to tasks in this project"
                },
                "status": {
                    "type": "string",
                    "enum": ["open", "assigned", "in_progress", "blocked", "review_required", "completed", "closed", "cancelled"],
                    "description": "Filter by status"
                },
                "assigned_to": {
                    "type": "string",
                    "description": "Filter by assigned agent"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 25
                }
            }
        }),
    ));

    tools.push(tool_def(
        "canopy_task_update_status",
        "Update the status of a task. Use canopy_task_complete for terminal completion.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                },
                "status": {
                    "type": "string",
                    "enum": ["open", "assigned", "in_progress", "blocked", "review_required", "completed", "closed", "cancelled"],
                    "description": "New status"
                },
                "changed_by": {
                    "type": "string",
                    "description": "Agent or user making the change"
                },
                "blocked_reason": {
                    "type": "string",
                    "description": "Required when status is blocked"
                },
                "closure_summary": {
                    "type": "string",
                    "description": "Summary when closing or completing"
                }
            },
            "required": ["task_id", "status", "changed_by"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_complete",
        "Mark a task as completed with a structured closure summary and release any file locks. If handoff_path is provided, validates that all checklist items are checked and paste markers filled before allowing completion.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to complete"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent completing the task"
                },
                "summary": {
                    "type": "string",
                    "description": "Structured summary of what was done and the outcome"
                },
                "verification_state": {
                    "type": "string",
                    "enum": ["not_required", "pending", "verified", "failed"],
                    "description": "Verification outcome if verification was required"
                },
                "handoff_path": {
                    "type": "string",
                    "description": "Optional path to a handoff document. When provided, completion is rejected if checklist items remain unchecked or paste markers are empty."
                },
                "output": {
                    "type": "object",
                    "description": "Optional structured task output containing raw output, parsed JSON, and token usage"
                }
            },
            "required": ["task_id", "agent_id", "summary"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_block",
        "Mark a task as blocked and optionally create a handoff requesting help.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to block"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent reporting the block"
                },
                "blocked_on": {
                    "type": "string",
                    "description": "What is blocking progress"
                },
                "request_help": {
                    "type": "boolean",
                    "description": "Create a request_help handoff to an orchestrator",
                    "default": false
                },
                "to_agent_id": {
                    "type": "string",
                    "description": "Agent to request help from (required if request_help is true)"
                }
            },
            "required": ["task_id", "agent_id", "blocked_on"]
        }),
    ));

    tools.push(tool_def(
        "canopy_task_snapshot",
        "Get a dashboard-style snapshot of tasks for a project with attention scores.",
        json!({
            "type": "object",
            "properties": {
                "project_root": {
                    "type": "string",
                    "description": "Scope snapshot to this project"
                },
                "preset": {
                    "type": "string",
                    "enum": ["default", "attention", "critical", "blocked", "overdue", "review_queue"],
                    "description": "Preset filter for common views"
                },
                "view": {
                    "type": "string",
                    "enum": ["summary", "detail", "tree"],
                    "description": "Output format"
                },
                "sort": {
                    "type": "string",
                    "enum": ["attention", "priority", "created", "updated"],
                    "description": "Sort order"
                },
                "priority_at_least": {
                    "type": "string",
                    "enum": ["low", "medium", "high", "critical"],
                    "description": "Minimum priority to include"
                }
            }
        }),
    ));

    tools.push(tool_def(
        "canopy_task_output",
        "Retrieve structured output from a completed task.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                }
            },
            "required": ["task_id"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Scope (2)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_report_scope_gap",
        "Report a scope gap for the active task. The runtime classifies the gap and either logs it as non-blocking or blocks the task and creates a child follow-up task.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task being worked"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent reporting the scope gap"
                },
                "work_item": {
                    "type": "string",
                    "description": "Description of the out-of-scope work item"
                }
            },
            "required": ["task_id", "agent_id", "work_item"]
        }),
    ));

    tools.push(tool_def(
        "canopy_get_handoff_scope",
        "Retrieve the declared scope for a task so the agent can compare a candidate work item against it.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                }
            },
            "required": ["task_id"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Files (4)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_files_lock",
        "Acquire exclusive locks on files before editing. Prevents concurrent edits by other agents.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task this lock belongs to"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent acquiring the locks"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Absolute file paths to lock"
                },
                "ttl_seconds": {
                    "type": "integer",
                    "minimum": 30,
                    "maximum": 3600,
                    "default": 300,
                    "description": "Lock TTL in seconds before auto-expiry"
                }
            },
            "required": ["task_id", "agent_id", "files"]
        }),
    ));

    tools.push(tool_def(
        "canopy_files_unlock",
        "Release file locks held by this agent. Call when done editing or on task completion.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task whose locks to release"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent releasing the locks"
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Specific file paths to unlock. Omit to release all locks for this task."
                },
                "force": {
                    "type": "boolean",
                    "description": "When true, release locks even if they are held by a different agent. Operator override — releases locks regardless of owner. Default: false."
                }
            },
            "required": ["task_id", "agent_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_files_check",
        "Check whether files are locked and by whom before attempting to edit.",
        json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Absolute file paths to check"
                }
            },
            "required": ["files"]
        }),
    ));

    tools.push(tool_def(
        "canopy_files_list_locks",
        "List all active file locks, optionally filtered by task or agent.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Filter locks to this task"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Filter locks held by this agent"
                },
                "include_expired": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include expired locks in the response"
                }
            }
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Handoff (5)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_handoff_create",
        "Create a structured handoff to transfer work or request review/verification from another agent.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task being handed off"
                },
                "from_agent_id": {
                    "type": "string",
                    "description": "Agent initiating the handoff"
                },
                "to_agent_id": {
                    "type": "string",
                    "description": "Agent receiving the handoff"
                },
                "handoff_type": {
                    "type": "string",
                    "enum": ["request_help", "request_review", "transfer_ownership", "request_verification", "record_decision", "close_task"],
                    "description": "Type of handoff"
                },
                "summary": {
                    "type": "string",
                    "description": "Structured summary: current state, intent, and handoff boundary"
                },
                "requested_action": {
                    "type": "string",
                    "description": "Specific action requested of the receiving agent"
                },
                "due_at": {
                    "type": "string",
                    "description": "RFC3339 UTC deadline for the requested action"
                },
                "expires_at": {
                    "type": "string",
                    "description": "RFC3339 UTC time after which the handoff is no longer valid"
                },
                "goal": {
                    "type": "string",
                    "description": "Concrete objective for the receiving agent (optional)"
                },
                "next_steps": {
                    "type": "string",
                    "description": "Specific next actions the receiver should take (optional)"
                },
                "stop_reason": {
                    "type": "string",
                    "description": "Why the sender is stopping work on this task (optional)"
                }
            },
            "required": ["task_id", "from_agent_id", "to_agent_id", "handoff_type", "summary"]
        }),
    ));

    tools.push(tool_def(
        "canopy_handoff_accept",
        "Accept a pending handoff and take ownership of the associated task.",
        json!({
            "type": "object",
            "properties": {
                "handoff_id": {
                    "type": "string",
                    "description": "ULID of the handoff to accept"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent accepting the handoff"
                },
                "note": {
                    "type": "string",
                    "description": "Optional acknowledgement note"
                }
            },
            "required": ["handoff_id", "agent_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_handoff_reject",
        "Reject a pending handoff with a reason. The task stays with the originating agent.",
        json!({
            "type": "object",
            "properties": {
                "handoff_id": {
                    "type": "string",
                    "description": "ULID of the handoff to reject"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent rejecting the handoff"
                },
                "reason": {
                    "type": "string",
                    "description": "Why the handoff is being rejected"
                }
            },
            "required": ["handoff_id", "agent_id", "reason"]
        }),
    ));

    tools.push(tool_def(
        "canopy_handoff_complete",
        "Mark a handoff as completed after fulfilling the requested action.",
        json!({
            "type": "object",
            "properties": {
                "handoff_id": {
                    "type": "string",
                    "description": "ULID of the handoff to complete"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent completing the handoff"
                },
                "outcome_summary": {
                    "type": "string",
                    "description": "What was done in response to the handoff"
                }
            },
            "required": ["handoff_id", "agent_id", "outcome_summary"]
        }),
    ));

    tools.push(tool_def(
        "canopy_handoff_list",
        "List handoffs, optionally filtered by task, agent, or status.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Filter to handoffs for this task"
                },
                "to_agent_id": {
                    "type": "string",
                    "description": "Filter to handoffs directed at this agent"
                },
                "from_agent_id": {
                    "type": "string",
                    "description": "Filter to handoffs originating from this agent"
                },
                "status": {
                    "type": "string",
                    "enum": ["open", "accepted", "rejected", "completed", "expired", "cancelled"],
                    "description": "Filter by handoff status"
                }
            }
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Evidence (4)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_attach_evidence",
        "Attach evidence to a task using the compact agent-facing form. The tool generates a label and returns the updated task evidence summary.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to attach evidence to"
                },
                "evidence_type": {
                    "type": "string",
                    "enum": [
                        "hyphae_session", "hyphae_recall", "hyphae_outcome",
                        "cortina_event",
                        "mycelium_command", "mycelium_explain",
                        "rhizome_impact", "rhizome_export",
                        "script_verification",
                        "manual_note"
                    ],
                    "description": "Which ecosystem tool the evidence comes from"
                },
                "ref_id": {
                    "type": "string",
                    "description": "ID or reference in the source tool (for example a session ID or event URI)"
                },
                "note": {
                    "type": "string",
                    "description": "Optional human-readable context for the evidence"
                }
            },
            "required": ["task_id", "evidence_type", "ref_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_evidence_add",
        "Attach an evidence reference to a task. Evidence links to external ecosystem tools — never copies payload.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task to attach evidence to"
                },
                "source_kind": {
                    "type": "string",
                    "enum": [
                        "hyphae_session", "hyphae_recall", "hyphae_outcome",
                        "cortina_event",
                        "mycelium_command", "mycelium_explain",
                        "rhizome_impact", "rhizome_export",
                        "script_verification",
                        "manual_note"
                    ],
                    "description": "Which ecosystem tool the evidence comes from"
                },
                "source_ref": {
                    "type": "string",
                    "description": "ID or reference in the source tool (e.g. session ID, command ID)"
                },
                "label": {
                    "type": "string",
                    "description": "Short human-readable label for this evidence"
                },
                "summary": {
                    "type": "string",
                    "description": "Optional brief summary of what this evidence shows"
                },
                "related_handoff_id": {
                    "type": "string",
                    "description": "Link to a related handoff"
                },
                "related_session_id": {
                    "type": "string",
                    "description": "Link to a related hyphae session"
                },
                "related_memory_query": {
                    "type": "string",
                    "description": "Query used for hyphae recall"
                },
                "related_symbol": {
                    "type": "string",
                    "description": "Code symbol referenced by rhizome evidence"
                },
                "related_file": {
                    "type": "string",
                    "description": "File path referenced by this evidence"
                }
            },
            "required": ["task_id", "source_kind", "source_ref", "label"]
        }),
    ));

    tools.push(tool_def(
        "canopy_evidence_list",
        "List all evidence attached to a task.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                }
            },
            "required": ["task_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_evidence_verify",
        "Verify that evidence references are still reachable in their source tools.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Verify evidence for this task only. Omit to verify all tasks."
                }
            }
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Council (2)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_council_post",
        "Post a message to the task council thread. Use for proposals, decisions, and status updates.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                },
                "author_agent_id": {
                    "type": "string",
                    "description": "Agent posting the message"
                },
                "message_type": {
                    "type": "string",
                    "enum": ["proposal", "objection", "evidence", "decision", "handoff", "status"],
                    "description": "Type of council message"
                },
                "body": {
                    "type": "string",
                    "description": "Message body"
                }
            },
            "required": ["task_id", "author_agent_id", "message_type", "body"]
        }),
    ));

    tools.push(tool_def(
        "canopy_council_show",
        "Read the full council thread for a task.",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ULID of the task"
                }
            },
            "required": ["task_id"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Import (1)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_import_handoff",
        "Import a handoff document from a file path and create the corresponding task and handoff record.",
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the handoff document (JSON or Markdown)"
                },
                "assign_to": {
                    "type": "string",
                    "description": "Agent to assign the imported task to"
                }
            },
            "required": ["path"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Completeness (1)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_check_handoff_completeness",
        "Check whether a handoff document meets completion criteria. Returns checkbox counts, paste marker status, and optional verify script results. Use before requesting task completion.",
        json!({
            "type": "object",
            "properties": {
                "handoff_path": {
                    "type": "string",
                    "description": "Absolute path to the handoff markdown document"
                },
                "run_verify_script": {
                    "type": "boolean",
                    "description": "When true, execute the paired verify script and include its output. Default: false."
                }
            },
            "required": ["handoff_path"]
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Outcome Learning Loop (4) — observational only, no policy side-effects
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_outcome_record",
        "Record a workflow-outcome-v1 JSON payload in the outcome ledger. Accepts either a 'json' string key containing the full payload or a 'json_object' key with the parsed object. Observational only — does not alter routing policy.",
        json!({
            "type": "object",
            "properties": {
                "json": {
                    "type": "string",
                    "description": "Raw workflow-outcome-v1 JSON string"
                },
                "json_object": {
                    "type": "object",
                    "description": "Parsed workflow-outcome-v1 object (alternative to json string)"
                }
            }
        }),
    ));

    tools.push(tool_def(
        "canopy_outcome_list",
        "List all stored workflow outcomes, most recent first (by completed_at). Observational only.",
        json!({
            "type": "object",
            "properties": {}
        }),
    ));

    tools.push(tool_def(
        "canopy_outcome_show",
        "Retrieve a single stored workflow outcome by workflow_id. Returns an error if the outcome is not found.",
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "description": "The workflow instance ULID to look up"
                }
            },
            "required": ["workflow_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_outcome_summary",
        "Return outcome counts grouped by template_id, failure_type, and the tail phase of the route taken. Use to observe outcome patterns before considering policy changes. Observational only — does not modify routing policy.",
        json!({
            "type": "object",
            "properties": {}
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // Tool Adoption Scoring (1)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_record_tool_usage",
        "Record a tool-usage-event-v1 JSON payload to compute and store tool adoption scores. Accepts either a 'json' string key containing the full payload or a 'json_object' key with the parsed object. Observational only — does not alter routing policy.",
        json!({
            "type": "object",
            "properties": {
                "json": {
                    "type": "string",
                    "description": "Raw tool-usage-event-v1 JSON string"
                },
                "json_object": {
                    "type": "object",
                    "description": "Parsed tool-usage-event-v1 object (alternative to json string)"
                }
            }
        }),
    ));

    // ─────────────────────────────────────────────────────────────────────────
    // DAG Task Graph (5)
    // ─────────────────────────────────────────────────────────────────────────

    tools.push(tool_def(
        "canopy_dag_create",
        "Create a new DAG task graph. Returns a graph_id that must be passed to subsequent dag calls.",
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for this task graph"
                }
            },
            "required": ["name"]
        }),
    ));

    tools.push(tool_def(
        "canopy_dag_add_node",
        "Add a node to a DAG graph. Returns a node_id. Nodes represent work items in the graph.",
        json!({
            "type": "object",
            "properties": {
                "graph_id": {
                    "type": "string",
                    "description": "The graph_id returned by canopy_dag_create"
                },
                "label": {
                    "type": "string",
                    "description": "Human-readable label for this node"
                },
                "task_id": {
                    "type": "string",
                    "description": "Optional task_id to associate with this node"
                }
            },
            "required": ["graph_id", "label"]
        }),
    ));

    tools.push(tool_def(
        "canopy_dag_add_edge",
        "Add a dependency edge between two nodes in a DAG graph. Use edge_type 'blocks' (default) to gate a node on its predecessors, or 'informs' for a non-blocking informational link.",
        json!({
            "type": "object",
            "properties": {
                "graph_id": {
                    "type": "string",
                    "description": "The graph_id returned by canopy_dag_create"
                },
                "from_node_id": {
                    "type": "string",
                    "description": "The source node_id (the dependency)"
                },
                "to_node_id": {
                    "type": "string",
                    "description": "The target node_id (the dependent)"
                },
                "edge_type": {
                    "type": "string",
                    "enum": ["blocks", "informs"],
                    "description": "Dependency type: 'blocks' gates the target on the source; 'informs' is non-blocking"
                }
            },
            "required": ["graph_id", "from_node_id", "to_node_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_dag_ready_nodes",
        "Get all nodes in a graph that are ready to run — status 'pending' with all blocking predecessors complete.",
        json!({
            "type": "object",
            "properties": {
                "graph_id": {
                    "type": "string",
                    "description": "The graph_id returned by canopy_dag_create"
                }
            },
            "required": ["graph_id"]
        }),
    ));

    tools.push(tool_def(
        "canopy_dag_complete_node",
        "Mark a node as complete, freeing any nodes it blocks. Returns the node_id and updated status.",
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "The node_id to mark complete"
                }
            },
            "required": ["node_id"]
        }),
    ));

    tools
}

#[allow(clippy::needless_pass_by_value)]
fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    let ann = annotations_for_tool(name);
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": ann.read_only_hint,
            "destructiveHint": ann.destructive_hint,
            "idempotentHint": ann.idempotent_hint
        }
    })
}
