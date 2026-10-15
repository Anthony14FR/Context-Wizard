---
description: Enable Context Wizard for this project
---

Run the Context Wizard binary with the shell tool, using the form for the shell you use:

- bash or sh: `'${CLAUDE_PLUGIN_DATA}/bin/wizard' decide on --project '${CLAUDE_PROJECT_DIR}'`
- PowerShell: `& '${CLAUDE_PLUGIN_DATA}/bin/wizard' decide on --project '${CLAUDE_PROJECT_DIR}'`

Then tell the user in one sentence that Context Wizard is enabled for this project.

If the binary is not found, tell the user that Context Wizard downloads it at the start of the next session, and that `/context-wizard:status` reports any download error.
