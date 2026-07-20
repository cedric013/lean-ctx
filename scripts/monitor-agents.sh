#!/bin/bash
set -euo pipefail

WTBASE="/tmp/lean-ctx-agents"
PROJECT="/Users/yvesgugger/Documents/Privat/Projects/lean-ctx"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  OCLA Agent Monitor — $(date '+%H:%M:%S')                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

RUNNING=$(ps aux | grep '[c]odex.*exec' | wc -l | tr -d ' ')
echo "Codex-Prozesse: $RUNNING"
echo ""

printf "%-5s %-6s %-4s %-45s\n" "Agent" "Status" "LOC" "Dateien"
printf "%-5s %-6s %-4s %-45s\n" "-----" "------" "----" "---------------------------------------------"

DONE=0
WORKING=0
IDLE=0

for i in $(seq -w 1 20); do
    WT="$WTBASE/agent-$i"
    if [ ! -d "$WT" ]; then
        printf "%-5s %-6s %-4s %-45s\n" "$i" "MISS" "-" "worktree missing"
        continue
    fi

    cd "$WT"
    AHEAD=$(git log main..HEAD --oneline 2>/dev/null | wc -l | tr -d ' ')
    CHANGED=$(git diff --name-only 2>/dev/null | wc -l | tr -d ' ')
    LINES=$(git diff --shortstat 2>/dev/null | grep -oE '[0-9]+ insertion' | grep -oE '[0-9]+' || echo "0")

    if [ "$AHEAD" -gt 0 ]; then
        STATUS="DONE"
        COMMIT_MSG=$(git log -1 --format='%s' 2>/dev/null | cut -c1-45)
        FILES="$COMMIT_MSG"
        DONE=$((DONE + 1))
    elif [ "$CHANGED" -gt 0 ]; then
        STATUS="WORK"
        FILES=$(git diff --name-only 2>/dev/null | sed 's|rust/src/||' | tr '\n' ', ' | sed 's/,$//')
        FILES="${FILES:0:45}"
        WORKING=$((WORKING + 1))
    else
        STATUS="IDLE"
        FILES="-"
        LINES="-"
        IDLE=$((IDLE + 1))
    fi

    printf "%-5s %-6s %-4s %-45s\n" "$i" "$STATUS" "+$LINES" "$FILES"
done

echo ""
echo "Zusammenfassung: $DONE fertig | $WORKING arbeiten | $IDLE idle"
