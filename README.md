# Context Wizard

Context Wizard gives Claude Code the right codebase context at the right time, for the fewest tokens, on any project.

## What it does

- **Opt-in per project.** On the first prompt in a project, Claude asks whether to enable Context Wizard. Nothing is indexed before the answer, and a project that says no is never touched.
- **Overview at session start.** About 800 tokens on the stack, sub-projects, entry points, commands and folder roles.
- **Predicted files at each prompt.** The five files a task most likely needs, with line ranges, shown only when the prediction is confident.
- **MCP tools.** `overview`, `map`, `find_symbol`, `refs`, `exact_refs`, `slice`, `impact`, `explain` and `facts` answer structural questions from a local index kept fresh before each answer.
- **Learning.** Each session teaches which files were useful for which kind of task, and predictions grade themselves.
- **Framework recipes** for Laravel, React, Next and Expo, and exact references from a language server when one is installed.

Commands: `/context-wizard:on`, `/context-wizard:off`, `/context-wizard:status`, `/context-wizard:stats` and `/context-wizard:purge`.

## Data it sends

Context Wizard has no server and no telemetry. Code, prompts and everything it learns stay on your machine, in the project's `.wizard/` folder. It contacts the network only to:

1. download its `wizard` binary from GitHub Releases, once per version, verified against a pinned SHA-256 checksum;
2. check once a day for a newer release on the GitHub API, which sends no data about you or your project and is skipped when `DISABLE_UPDATES`, `DISABLE_AUTOUPDATER` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` is set;
3. download tree-sitter grammars on first use, checked against their manifest.

The optional `wizard calibrate` command, which you run yourself, sends a sample of project files to Anthropic's `count_tokens` endpoint with your own API key, never files that contain secrets.

See the [privacy policy](https://github.com/Anthony14FR/Context-Wizard/blob/main/docs/privacy.md) and the [project README](https://github.com/Anthony14FR/Context-Wizard#readme).

## Install

```sh
claude plugin marketplace add Anthony14FR/Context-Wizard
claude plugin install context-wizard@context-wizard
```

The `wizard` binary is not stored in this folder: the plugin downloads the one for your platform at the next session start.

## License

MIT OR Apache-2.0.
