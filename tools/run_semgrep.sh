#!/usr/bin/env bash
# Repo-specific static analysis (use-protection-please H-22), run twice:
#
#   1. over the real tree — must be CLEAN;
#   2. over tools/semgrep-selftest/ — every rule must FIRE.
#
# The second pass is the point. A pattern rule that has quietly stopped
# matching (a semgrep upgrade, a refactor that renamed the thing it looked
# for) reports safety it is no longer checking — the same failure mode as a
# fast path nothing calls. Asserting the rules still fire on deliberate
# violations makes that impossible to miss.
set -euo pipefail

RULES="tools/semgrep-rules.yml"
SELFTEST="tools/semgrep-selftest"
SEMGREP="${SEMGREP:-semgrep}"

echo "== pass 1: the real tree must be clean"
"$SEMGREP" --config "$RULES" --metrics off --error --exclude "$SELFTEST" .

echo
echo "== pass 2: every rule must fire on the self-test"
# Count how many DISTINCT rules matched the deliberate violations.
fired=$("$SEMGREP" --config "$RULES" --metrics off --json "$SELFTEST" \
    | python3 -c 'import sys,json; print(len({r["check_id"] for r in json.load(sys.stdin)["results"]}))')
total=$(python3 -c "
import sys,re
text = open('$RULES', encoding='utf-8').read()
print(sum(1 for line in text.splitlines() if re.match(r'^  - id: ', line)))
")

echo "rules firing on self-test: $fired / $total"
if [ "$fired" -ne "$total" ]; then
    echo "STATIC-ANALYSIS SELF-TEST FAILED: $((total - fired)) rule(s) no longer match" >&2
    echo "A rule that cannot fire is a gate that reports safety it does not check." >&2
    exit 1
fi
echo "all rules verified live"
