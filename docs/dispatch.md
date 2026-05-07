# Wave → Checkpoint → Wave Parallel Dispatch Model

## Model

**Wave**: a set of handoffs that run in parallel because they have no shared write surface.

**Checkpoint**: a synchronization point where the parent agent waits for all in-flight
implementers to finish, reviews their output, and verifies correctness before launching
the next wave.

**Wave → Checkpoint → Wave**: batch independent operations in a wave, stop at a
checkpoint when downstream work depends on upstream output, then batch the next
independent wave.

## When to use a Wave

Handoffs belong in the same wave when ALL of the following are true:

1. They write to different files or directories (no shared path)
2. Neither handoff's output is the other handoff's input
3. No shared septa schema is both produced by one and consumed by the other
4. No shared test suite validates both handoffs

## When NOT to parallelize (Checkpoint required)

Use a Checkpoint — do not parallelize — when any of the following apply:

- Two handoffs touch the same septa schema file
- Two handoffs are in the same owning repo but share a module or file
- One handoff is an auditor of the other
- One handoff produces a schema, fixture, or type that another handoff consumes
- One handoff's verification command overlaps with another's (same test suite)
- The parent agent cannot verify correctness until both are done sequentially

## Concrete examples

**Valid parallel wave** (from this project):
- `impl/cortina/hook-turn-halt/1` and `impl/canopy/dag-topology/1` — different repos,
  different files, no shared septa schema.

**Checkpoint required**:
- `impl/septa/heartbeat-schema/1` must finish before `impl/annulus/heartbeat-display/1`
  starts — annulus consumes the schema that septa produces.
- `impl/cortina/signals/1` and `impl/cortina/adapters/1` cannot run in parallel —
  same repo, one produces a type the other imports.

## Metadata fields

Handoffs may declare wave membership in their frontmatter:

```
- **Wave:** 2
- **Depends-on:** septa-heartbeat-schema
- **Produces:** septa/agent-heartbeat-v1.schema.json
```

These fields are advisory. The parent agent enforces the checkpoint; the fields provide
visibility into the intended ordering. The canonical template that includes these fields is
[`templates/handoffs/WORK-ITEM-TEMPLATE.md`](../../templates/handoffs/WORK-ITEM-TEMPLATE.md).

## Relationship to the implementer/auditor pattern

Each wave still follows the implementer → Stage 1 review → fix → Stage 2 review → commit
sequence. The checkpoint is between waves, not between stage reviews within a single lane.
