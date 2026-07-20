#!/bin/bash
set -euo pipefail

PROJECT="/Users/yvesgugger/Documents/Privat/Projects/lean-ctx"
GOALS="/tmp/codex-goals"
COMBINED="/tmp/codex-combined"
WTBASE="/tmp/lean-ctx-agents"
TOTAL=20

echo "=== OCLA Agent Orchestration (Worktree-Isolated) ==="
echo "Each agent gets its own git worktree — no conflicts."
echo ""

# Pre-flight
cd "$PROJECT"
if [ "$(git branch --show-current)" != "main" ]; then
    echo "ERROR: must be on main"; exit 1
fi

# Prepare combined prompt files
rm -rf "$COMBINED" && mkdir -p "$COMBINED"
for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    GOAL=$(ls "$GOALS"/agent-${padded}-*.md 2>/dev/null | head -1)
    if [ -z "$GOAL" ]; then
        echo "SKIP agent $padded: no goal file"
        continue
    fi
    cat "$GOALS/preamble.md" "$GOAL" > "$COMBINED/agent-${padded}.md"
done

# Verify worktrees exist
for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    WT="$WTBASE/agent-$padded"
    if [ ! -d "$WT" ]; then
        echo "Creating worktree for agent $padded..."
        git worktree add "$WT" -b "agent/wt-$padded" main 2>/dev/null || true
    fi
done

echo ""
echo "Launching $TOTAL agents in isolated worktrees..."
echo ""

for i in $(seq 1 $TOTAL); do
    padded=$(printf "%02d" $i)
    PROMPT_FILE="$COMBINED/agent-${padded}.md"
    WT="$WTBASE/agent-$padded"

    if [ ! -f "$PROMPT_FILE" ]; then
        echo "SKIP agent $padded: no prompt file"
        continue
    fi

    name=$(head -10 "$PROMPT_FILE" | grep 'DEINE ROLLE' | sed 's/.*: //' || echo "agent-$padded")
    echo "[$i/$TOTAL] $name → $WT"

    osascript <<APPLESCRIPT
tell application "Terminal"
    do script "cd $WT && cat $PROMPT_FILE | codex exec -s workspace-write -"
end tell
APPLESCRIPT

    sleep 2
done

echo ""
echo "=== $TOTAL agents launched in isolated worktrees ==="
echo ""
echo "Monitor:"
echo "  git worktree list"
echo "  for i in \$(seq -w 1 20); do echo \"=== agent-\$i ===\"; cd $WTBASE/agent-\$i && git log --oneline -1 && git diff --stat; done"
echo "  lean-ctx agent list"
