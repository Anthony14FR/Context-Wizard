---
description: Disable Context Wizard for this project
---

Run the Context Wizard binary with the shell tool, using the form for the shell you use:

- bash or sh: `'${CLAUDE_PLUGIN_DATA}/bin/wizard' decide off --project '${CLAUDE_PROJECT_DIR}'`
- PowerShell: `& '${CLAUDE_PLUGIN_DATA}/bin/wizard' decide off --project '${CLAUDE_PROJECT_DIR}'`

Then tell the user in one sentence that Context Wizard is disabled for this project. Mention that Claude Code stops loading the plugin here from the next session, and that running the same command with `decide on`, or removing the `context-wizard@` entry from `.claude/settings.local.json`, enables it again.

If the binary is not found, tell the user that Context Wizard downloads it at the start of the next session, and that `/context-wizard:status` reports any download error.
