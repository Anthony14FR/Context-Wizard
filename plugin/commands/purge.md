---
description: Delete what Context Wizard stored for this project
argument-hint: "[all]"
---

Context Wizard keeps two things for this project:

- `.wizard/`: the index and everything learned from past sessions, which nothing else can rebuild.
- `.claude/wizard.json`: the decision to enable or disable it, which may be committed and shared with the team.

Before deleting anything, tell the user what will be deleted and ask them to confirm. By default only `.wizard/` is deleted, and learning starts over. With the argument `all` ($ARGUMENTS), the decision is deleted too and the question comes back on the next prompt. Do not run the command until the user has confirmed.

Once confirmed, run the Context Wizard binary with the shell tool, adding ` --all` only for `all`, using the form for the shell you use:

- bash or sh: `'${CLAUDE_PLUGIN_DATA}/bin/wizard' purge --project '${CLAUDE_PROJECT_DIR}'`
- PowerShell: `& '${CLAUDE_PLUGIN_DATA}/bin/wizard' purge --project '${CLAUDE_PROJECT_DIR}'`

Then show the output to the user. If the decision file was committed, remind them that its deletion is a change to commit, or to revert to keep the team's choice.
