# 3. Per-project opt-in stored in the repository

- Status: Accepted
- Date: 2026-10-15

## Context

A Claude Code plugin, once installed, is loaded in every project. Context Wizard indexes the project, records session events and injects context, so it must not act on a project whose owner has not agreed. The decision must also be consistent: every session and every teammate on a project should see the same behaviour, without asking again.

Claude Code documents that a plugin id set to `false` in `.claude/settings.local.json` overrides project settings, but older reports said `enabledPlugins` was only global. This behaviour has to be verified before it is relied upon.

## Decision

The decision is stored in the project, in `.claude/wizard.json`:

```json
{ "enabled": true, "version": 1, "decidedAt": "2026-10-15" }
```

- **No file:** the plugin is undecided. `UserPromptSubmit` adds the opt-in question next to the user's prompt, and Claude asks it. The question is repeated on each prompt until the user answers. Every other hook does nothing.
- **`enabled: true`:** all hooks are active.
- **`enabled: false`:** every hook exits immediately. The plugin also writes `"context-wizard@context-wizard": false` in `.claude/settings.local.json`, so that Claude Code unloads the MCP server and the skill for that project.

The file is meant to be committed, so a team shares the decision. `/context-wizard:on` and `/context-wizard:off` change it later. Readers accept unknown fields and refuse a newer `version` by going inert.

The question is injected by `UserPromptSubmit` rather than `SessionStart`, because an instruction placed next to the prompt is harder for Claude to skip.

## Consequences

- No indexing, logging or context injection happens on a project before an explicit yes.
- Teams get one shared decision, visible in code review.
- If Claude Code ignores the per-project `settings.local.json` override, a disabled project still has the MCP server loaded, but `wizard.json` keeps every hook and tool inert. This fallback is documented after testing.
- Index and learning data live in `.wizard/`, which is separate from the decision and never committed.
