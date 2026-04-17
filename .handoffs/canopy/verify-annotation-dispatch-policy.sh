#!/usr/bin/env bash
# verify-annotation-dispatch-policy.sh
# Checks that the annotation-aware dispatch policy implementation is in place.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PASS=0
FAIL=0

check() {
    local label="$1"
    local result="$2"
    if [ "$result" = "ok" ]; then
        echo "[PASS] $label"
        ((PASS += 1))
    else
        echo "[FAIL] $label"
        ((FAIL += 1))
    fi
}

# 1. src/tools/policy.rs exists
if [ -f "$REPO_ROOT/src/tools/policy.rs" ]; then
    check "src/tools/policy.rs exists" "ok"
else
    check "src/tools/policy.rs exists" "fail"
fi

# 2. dispatch_tool references policy
if grep -q "policy::" "$REPO_ROOT/src/tools/mod.rs"; then
    check "dispatch_tool references policy module" "ok"
else
    check "dispatch_tool references policy module" "fail"
fi

# 3. canopy policy subcommand exists in cli.rs
if grep -q "PolicyCommand" "$REPO_ROOT/src/cli.rs"; then
    check "canopy policy subcommand defined in cli.rs" "ok"
else
    check "canopy policy subcommand defined in cli.rs" "fail"
fi

# 4. schema.rs imports annotations_for_tool
if grep -q "annotations_for_tool" "$REPO_ROOT/src/mcp/schema.rs"; then
    check "schema.rs uses annotations_for_tool" "ok"
else
    check "schema.rs uses annotations_for_tool" "fail"
fi

# 5. cargo test passes
echo ""
echo "Running cargo test --workspace..."
if (cd "$REPO_ROOT" && cargo test --workspace --quiet 2>&1); then
    check "cargo test --workspace passes" "ok"
else
    check "cargo test --workspace passes" "fail"
fi

# 6. canopy policy show runs without error
if (cd "$REPO_ROOT" && cargo run --bin canopy --quiet -- policy show 2>/dev/null | grep -q "default"); then
    check "canopy policy show runs and prints policy name" "ok"
else
    check "canopy policy show runs and prints policy name" "fail"
fi

echo ""
echo "Results: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
