#!/usr/bin/env bash
set -euo pipefail
PASS=0; FAIL=0
ROOT="/Users/williamnewton/projects/basidiocarp"
check() { local name="$1"; shift; if "$@"; then printf 'PASS: %s\n' "$name"; PASS=$((PASS+1)); else printf 'FAIL: %s\n' "$name"; FAIL=$((FAIL+1)); fi }

check "work_queue tests pass" bash -lc "cd '$ROOT/canopy' && cargo test work_queue 2>&1 | grep -qE '[0-9]+ passed'"
check "assigned task tests pass" bash -lc "cd '$ROOT/canopy' && cargo test assigned 2>&1 | grep -qE '[0-9]+ passed'"
check "include_assigned flag exists in CLI" bash -lc "rg -q 'include_assigned' '$ROOT/canopy/src/cli.rs'"
check "list_tasks_for_agent used in work_queue path" bash -lc "rg -q 'list_tasks_for_agent' '$ROOT/canopy/src/tools/queue.rs'"

printf 'Results: %d passed, %d failed\n' "$PASS" "$FAIL"
test "$FAIL" -eq 0
