# Context Wizard

**English** | [Français](README.fr.md)

Give Claude Code the right codebase context at the right time, for the fewest tokens, on any project.

> **Status: early development.** Context Wizard is in Phase 0 of its [roadmap](#roadmap). Nothing is released yet and the install commands below will work from the first `0.x` release.

## Why

At the start of every session on a medium or large codebase, Claude spends time and tokens rediscovering the same things: the tree, the manifests, the entry points, the models, the conventions. Context Wizard precomputes that context, keeps it fresh automatically, and serves it in the most compact form, so exploration starts from the right files and costs less.

## How it works

Context Wizard is a Claude Code plugin backed by a single Rust binary, `wizard`. The binary runs as a short-lived command for hooks and as a long-lived MCP server that holds the project index in memory.

This section describes the design targeted for 1.0.

Context is disclosed progressively, never all at once:

| Level | Budget | Delivered | Content |
| --- | --- | --- | --- |
| L0 Overview | 800 tokens | Once, at session start | Stack, entry points, commands, folder roles |
| L1 Map | 4,000 tokens | On demand | Ranked signatures with line numbers |
| L2 Queries | Per call | MCP tools | Symbol lookup, references, task slice, impact |

Files and symbols are ranked by a personalised PageRank over import, reference and git co-change edges. The plugin also learns from your sessions: files that are read then edited gain weight, noise fades out, and the files you are likely to need are suggested next to your prompt.

### Any technology

Support degrades gracefully and never fails:

1. **Universal:** tree, sizes, git churn and co-change, learning. Works on any project.
2. **Syntax:** definitions, references and imports through tree-sitter, for hundreds of languages.
3. **Lexical:** identifiers, headings and top-level keys for files without a grammar.
4. **Recipes:** framework knowledge (routes, models, components) in declarative TOML files.
5. **LSP:** exact references from a free language server, when one is installed.

### Opt-in per project

On the first prompt in a project, Claude asks whether to enable Context Wizard. The answer is stored in `.claude/wizard.json`, so every later session, and every teammate once the file is committed, gets the same behaviour. A project that says no is never touched again. You can change the decision with `/context-wizard:on` and `/context-wizard:off`.

### Principles

- **Free.** Permissive licences only, no paid API, no cloud service, no telemetry.
- **Local.** Code, prompts and learned weights never leave your machine. Project data lives in `.wizard/`, which is git-ignored. The only exception is an optional token-count calibration, which uses your own Anthropic API key.
- **Fail inert.** Any error, timeout or missing binary leaves Claude Code working exactly as it would without the plugin.
- **Fresh or nothing.** Every answer is checked against file hashes. Stale context is never served.

## Installation

Available from the first release. The repository is its own plugin marketplace:

```sh
claude plugin marketplace add Anthony14FR/Context-Wizard
claude plugin install context-wizard@context-wizard
```

On first use, the plugin downloads the `wizard` binary for your platform from GitHub Releases and verifies its SHA-256 checksum. Until the binary is ready, the plugin stays inert.

To receive updates automatically, open `/plugin`, go to **Marketplaces** and choose **Enable auto-update**. Claude Code leaves this off by default for third-party marketplaces.

Planned platforms: Linux, macOS and Windows, on x86_64 and aarch64.

## Roadmap

| Phase | Content |
| --- | --- |
| P0 Foundations | Repository, CI, plugin skeleton, opt-in flow, binary bootstrap |
| P1 Index, L0 and L1 | Scanner, hash cache, grammars, lexical extractor, token estimator |
| P2 Graph and MCP | Ranking, git signals, MCP server, hooks |
| P3 Learning | Session signals, task-to-files matching, self-evaluation |
| P4 Recipes and LSP | Laravel, React, Next and Expo recipes, LSP fallback |
| P5 Distribution | Release binaries, community marketplace, 1.0 |

Each phase closes only when its exit criteria are met. The 1.0 goal is 60% fewer exploration tokens than Claude Code without the plugin, with an equal or better task success rate.

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or a pull request. Issues, pull requests and code are written in English.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

Context Wizard is an independent project and is not affiliated with or endorsed by Anthropic.
