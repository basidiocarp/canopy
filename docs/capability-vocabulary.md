# Capability Vocabulary

This document is the canonical source for the shared capability vocabulary used by both Canopy and Hymenium.

Capabilities are short string labels that describe what a Worker agent can do. They are distinct from tier hints (`opus`, `sonnet`, `haiku`, `any`), which remain as coarse cost/quality recommendations. Capabilities are about what kind of work the task requires, not which model tier should execute it.

## Stable Labels

| Label | Meaning |
|-------|---------|
| `rust` | Rust compilation and Cargo tooling (build, test, clippy, fmt) |
| `frontend` | React/TypeScript work (cap dashboard, npm build) |
| `schema` | JSON schema and septa contract work |
| `sqlite` | SQLite schema migrations and direct database work |
| `docs` | Markdown authoring only (no compilation required) |
| `shell` | Bash/zsh scripting |
| `orchestration` | Workflow runtime work (hymenium, canopy internals) |

## Rules

- Keep this list at 10 labels or fewer.
- Do not add a new label for every new task type — prefer composing existing labels.
- Labels are lowercase ASCII with no spaces or special characters.
- `required_capabilities` on a task is additive: a task requiring `["rust", "shell"]` needs an agent that has both.

## Where It Is Used

- **Canopy** (`src/capability.rs`): exports string constants matching these labels.
- **Hymenium** (`src/dispatch/capability.rs`): exports string constants matching these labels and uses them in dispatch.
- Both repos derive the `required_capabilities` `Vec<String>` from these constants, not from free-form strings.

## Backward Compatibility

Tasks with an empty `required_capabilities` list are claimable by any agent regardless of its declared capabilities. This preserves backward compatibility with tasks created before this vocabulary was introduced.
