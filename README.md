# Context Wizard

**English** | [Français](README.fr.md)

Give Claude Code the right codebase context at the right time, for the fewest tokens, on any project.

> **Status: pre-release.** Every phase of the [roadmap](#roadmap) up to recipes and LSP is implemented. The first `0.x` release and the public benchmark are next; the install commands below work from that release.

## Why

At the start of every session on a medium or large codebase, Claude spends time and tokens rediscovering the same things: the tree, the manifests, the entry points, the models, the conventions. Context Wizard precomputes that context, keeps it fresh automatically, and serves it in the most compact form, so exploration starts from the right files and costs less.

## What it does

- **Overview at session start.** About 800 tokens on the stack, sub-projects, entry points, commands, folder roles and framework facts, injected before the first prompt.
- **Predicted files at each prompt.** The five files a task most likely needs, with line ranges, injected only when the prediction is confident.
- **Answers instead of searches.** An MCP server answers structural questions from an index kept fresh before every answer.
- **Greps that answer themselves.** When Claude greps for a known symbol, its definition is attached to the search.
- **Focused reads.** When Claude reads a file of more than 400 lines without a range, the read is narrowed to the definitions that match the task, with the file's outline beside it, since every tool result is read again at every later turn. Without a clear match the file is read whole.
- **Shorter command outputs.** Outputs of more than 200 lines, such as test runs and builds, keep their first and last lines and every error, failure and warning with the lines around them, with a note of what was left out.
- **Learning.** Each session teaches which files were useful for which kind of task. Predictions grade themselves at the end of each answer and adjust how much they trust learning.

### MCP tools

| Tool | Answers |
| --- | --- |
| `overview` | Stack, sub-projects, entry points, commands and folder roles |
| `map` | The most used definitions, with signatures and line numbers, within 4,000 tokens |
| `find_symbol` | Where a name is defined |
| `refs` | Where a name is used |
| `exact_refs` | Exact references from a language server, when one is installed |
| `slice` | The files most relevant to a task described in words |
| `impact` | What a change to a file may affect: dependents, co-changed files, files opened together |
| `explain` | The outline of a file without reading it |
| `facts` | Framework facts: routes, models and relations, schema, components, hooks, pages, screens |

### Any technology

Support degrades gracefully and never fails:

1. **Universal:** tree, sizes, git churn and co-change, learning. Works on any project.
2. **Syntax:** definitions, references and imports through tree-sitter, for hundreds of languages. Grammars download on first use.
3. **Lexical:** identifiers, headings and top-level keys for files without a grammar.
4. **Recipes:** framework knowledge in declarative TOML files. Laravel, React, Next and Expo ship built in, and you can [write your own](docs/recipes.md).
5. **LSP:** exact references from rust-analyzer, typescript-language-server, pyright, pylsp, intelephense, gopls or clangd, when installed.

### Opt-in per project

On the first prompt in a project, Claude asks whether to enable Context Wizard. The answer is stored in `.claude/wizard.json`, so every later session, and every teammate once the file is committed, gets the same behaviour. A project that says no is never indexed.

### Principles

- **Free.** Permissive licences only, no paid API, no cloud service, no telemetry.
- **Local.** Code, prompts and learned weights never leave your machine. Project data lives in `.wizard/`, which ignores itself in git.
- **Fail inert.** Any error, timeout or missing binary leaves Claude Code working exactly as it would without the plugin.
- **Fresh or nothing.** The index is refreshed from file hashes before answers. Stale context is never served.

## Installation

The repository is its own plugin marketplace:

```sh
claude plugin marketplace add Anthony14FR/Context-Wizard
claude plugin install context-wizard@context-wizard
```

At the next session start, the plugin downloads the `wizard` binary for your platform from GitHub Releases into `${CLAUDE_PLUGIN_DATA}/bin`, verifies its SHA-256 checksum, and installs it atomically. Until the binary is ready, the plugin stays inert. Binaries exist for Linux, macOS and Windows, on x86_64 and aarch64, with no runtime dependency.

To receive updates automatically, open `/plugin`, go to **Marketplaces** and choose **Enable auto-update**. Claude Code leaves this off by default for third-party marketplaces. Otherwise, Context Wizard shows a one-line notice when a new release is out.

### Beta channel

Prereleases ship first to a second marketplace, `context-wizard-beta`, built from the `next` branch. To try them, uninstall the stable plugin first, since both would run their hooks:

```sh
claude plugin uninstall context-wizard@context-wizard --keep-data
claude plugin marketplace add Anthony14FR/Context-Wizard#next
claude plugin install context-wizard@context-wizard-beta
```

To go back, uninstall `context-wizard@context-wizard-beta` and install `context-wizard@context-wizard` again. Project data is shared by both channels.

## Usage

Answer the question on the first prompt of a project, then work as usual. Commands:

| Command | Effect |
| --- | --- |
| `/context-wizard:on` | Enable Context Wizard for this project |
| `/context-wizard:off` | Disable it; Claude Code stops loading the plugin in this project |
| `/context-wizard:status` | Decision, binary location and the last install error |
| `/context-wizard:stats` | Precision of predictions over time, most useful files, glossary |
| `/context-wizard:purge` | Delete the index and learning data, or with `all` the decision too |

The binary also runs from a terminal, for scripts and checks: `wizard index`, `wizard map --level 0|1`, `wizard stats`, `wizard recipes`, `wizard purge`, each with `--project <path>`.

## Configuration

Context Wizard needs no configuration. These settings exist when you want them:

| Setting | Effect |
| --- | --- |
| `DISABLE_UPDATES`, `DISABLE_AUTOUPDATER` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | No daily release check |
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` | Used for the binary download and the release check |
| `.claude/context-wizard/recipes/<name>/` | Project recipes, see [the recipe guide](docs/recipes.md) |
| `wizard calibrate` with `ANTHROPIC_API_KEY` | Measures this project's bytes per token with Anthropic's free `count_tokens` endpoint, so token budgets fit the real tokenizer. It sends a sample of files, never files with secrets |

Context Wizard writes only these files in a project:

- `.claude/wizard.json`: the decision, meant to be committed.
- `.claude/settings.local.json`: a single `enabledPlugins` entry, only when the project says no.
- `.wizard/`: the index, the learning store and its backups. It holds a `.gitignore` that ignores everything.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Claude never asks the question | Run `/context-wizard:status`. If the binary is missing, the last install error says why; the download is retried at every session start. |
| Download fails behind a proxy | Set `HTTPS_PROXY` (and `NO_PROXY` for internal hosts) in the environment Claude Code starts from. |
| Nothing happens after an update | Hooks stay inert while the binary of the new version downloads. Start a new session or run `/reload-plugins`. |
| Windows SmartScreen or macOS Gatekeeper | The launcher downloads with `curl` or PowerShell, which do not mark the file as coming from the internet, so neither inspects it. If you copy the binary by hand from a browser, unblock it in its properties or run `xattr -d com.apple.quarantine` on it. |
| `exact_refs` falls back to name references | No language server for that language is on the `PATH`; the answer says which one to install. |
| A project recipe does nothing | Run `wizard recipes --project .`: it lists where each recipe applies and every problem in broken ones. |
| Stale or wrong answers | Run `/context-wizard:purge`; the index is rebuilt at the next session. |

## Uninstall

```sh
claude plugin uninstall context-wizard@context-wizard
```

Uninstalling from the last scope also deletes `${CLAUDE_PLUGIN_DATA}`, with the downloaded binaries, unless you pass `--keep-data`. Project data stays in each project: run `/context-wizard:purge all` there first, or delete `.wizard/` and `.claude/wizard.json` yourself.

## Roadmap

| Phase | Content | State |
| --- | --- | --- |
| P0 Foundations | Repository, CI, plugin skeleton, opt-in flow, binary bootstrap | Done |
| P1 Index, L0 and L1 | Scanner, hash cache, grammars, lexical extractor, token estimator | Done |
| P2 Graph and MCP | Ranking, git signals, MCP server, hooks | Done |
| P3 Learning | Session signals, task-to-files matching, self-evaluation | Done |
| P4 Recipes and LSP | Laravel, React, Next and Expo recipes, LSP fallback | Done |
| P5 Distribution | Release binaries, community marketplace, public benchmark, 1.0 | In progress |

The 1.0 goal is 60% fewer exploration tokens than Claude Code without the plugin, with an equal or better task success rate, measured by the [benchmark](bench/README.md) on six public projects.

## Security

See [SECURITY.md](SECURITY.md) to report a vulnerability, and the [threat model](docs/threat-model.md) for what Context Wizard protects and how.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or a pull request. Issues, pull requests and code are written in English. Architecture decisions are recorded in [docs/adr](docs/adr/README.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Context Wizard is an independent project and is not affiliated with or endorsed by Anthropic.
