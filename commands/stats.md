---
description: Show what Context Wizard learned about this project and how well it predicts
---

Run the Context Wizard binary with the shell tool, using the form for the shell you use:

- bash or sh: `'${CLAUDE_PLUGIN_DATA}/bin/wizard' stats --project '${CLAUDE_PROJECT_DIR}'`
- PowerShell: `& '${CLAUDE_PLUGIN_DATA}/bin/wizard' stats --project '${CLAUDE_PROJECT_DIR}'`

Then show the output to the user as is, inside a code block so the precision bars stay aligned. Add one or two sentences on the trend: whether precision rises or falls, and what the weight of the code structure means (closer to 0.9, predictions lean on the code structure; closer to 0.5, they lean on what past sessions taught).

If the binary is not found, tell the user that Context Wizard downloads it at the start of the next session, and that `/context-wizard:status` reports any download error.
