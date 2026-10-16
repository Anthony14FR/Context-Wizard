# Changelog

All notable changes to Context Wizard are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-10-16

First public release.

### Plugin and installation

- Plugin and marketplace in one repository: `claude plugin marketplace add Anthony14FR/Context-Wizard`, then `claude plugin install context-wizard@context-wizard`.
- Per-project opt-in: Claude asks once per project, and the answer is stored in `.claude/wizard.json`. `/context-wizard:on`, `:off`, `:status`, `:stats` and `:purge` manage it.
- Bootstrap that downloads the binary for Linux, macOS or Windows on x86_64 or aarch64, verifies its SHA-256, installs it atomically, honours proxies, and keeps the two previous versions.
- Version handshake: hooks stay inert until the binary matches the plugin version.
- Daily update notice, disabled by the Claude Code opt-out variables, and a one-time tip on enabling auto-update.
- Beta channel `context-wizard-beta` on the `next` branch.

### Index and context

- Incremental index in `.wizard/index.db`, keyed by BLAKE3 hashes, hermetic scan respecting `.gitignore`, files over 1 MiB skipped.
- Definitions, references and signatures through tree-sitter for hundreds of languages, with a lexical fallback, and secret redaction.
- Overview of about 800 tokens injected at session start, and a ranked map of definitions within 4,000 tokens.
- File graph from references, git co-change and learned co-access, ranked by the lift of personalised PageRank.
- MCP server with `overview`, `map`, `find_symbol`, `refs`, `exact_refs`, `slice`, `impact`, `explain` and `facts`.
- Grep for a known symbol answered with its definition.

### Learning

- Session events recorded in `.wizard/learn.db`: prompts, reads, searches, edits and the end of each answer.
- Signals of useful and noisy reads, map gaps, pivot files and co-access, consolidated at each `Stop`.
- Prediction of the five files a prompt needs from BM25 over past prompts, a domain glossary and file history, combined with the code structure. Predictions are injected when confident.
- Self-evaluation with precision@5 and a rolling average over 20 sessions, which adjusts the weight of the code structure between 0.5 and 0.9.

### Recipes and LSP

- Declarative TOML recipes, validated as a whole and reloaded at each refresh. Built in for Laravel, React, Next and Expo; projects can add their own.
- Exact references from rust-analyzer, typescript-language-server, pyright, pylsp, intelephense, gopls or clangd when installed.

### Data safety

- The learning store migrates with a backup, and a newer schema is left untouched on a downgrade.
- `wizard calibrate` measures token ratios with Anthropic's `count_tokens` endpoint, on request only.
- Property-based fuzzing of every parser of untrusted input, and a threat model.

[0.1.0]: https://github.com/Anthony14FR/Context-Wizard/releases/tag/v0.1.0
