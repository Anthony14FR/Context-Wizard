#!/bin/sh
# Records a session with the latest release, then checks the current build keeps its data.
set -eu

target=$1
new=$2
previous=$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName --jq .tagName 2>/dev/null || true)
if [ -z "$previous" ]; then
  echo "No release published yet, nothing to upgrade from."
  exit 0
fi
work=$(mktemp -d)
suffix=""
case "$target" in *windows*) suffix=".exe" ;; esac
gh release download "$previous" --repo "$GITHUB_REPOSITORY" --pattern "wizard-$target$suffix" --dir "$work"
old="$work/wizard-$target$suffix"
chmod +x "$old"
project="$work/project"
mkdir -p "$project/src"
printf 'def compute_invoice(total):\n    return round(total, 2)\n' > "$project/src/billing.py"
file="$project/src/billing.py"
case "$target" in *windows*) file=$(cygpath -m "$file") ;; esac

hook() {
  printf '%s' "$2" | CLAUDE_PROJECT_DIR="$project" "$1" hook > /dev/null
}

echo "Recording a session with $previous"
"$old" decide on --project "$project"
hook "$old" '{"hook_event_name":"UserPromptSubmit","session_id":"old","prompt":"fix the invoice total"}'
hook "$old" "{\"hook_event_name\":\"PostToolUse\",\"session_id\":\"old\",\"tool_name\":\"Read\",\"tool_input\":{\"file_path\":\"$file\"}}"
hook "$old" "{\"hook_event_name\":\"PostToolUse\",\"session_id\":\"old\",\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$file\"}}"
hook "$old" '{"hook_event_name":"Stop","session_id":"old"}'

echo "Upgrading to the current build"
context=$(printf '%s' '{"hook_event_name":"SessionStart","source":"startup"}' | CLAUDE_PROJECT_DIR="$project" "$new" hook)
echo "$context" | grep -q 'Project overview' || { echo "no overview after the upgrade: $context"; exit 1; }
grep -q '"enabled": true' "$project/.claude/wizard.json" || { echo "decision lost"; exit 1; }
stats=$("$new" stats --project "$project")
echo "$stats"
echo "$stats" | grep -q 'Sessions recorded: 1' || { echo "learning data lost"; exit 1; }
"$new" index --project "$project"
echo "Upgrade from $previous kept the decision, the learning data and the index."
