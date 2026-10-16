# Changelog

All notable changes to Context Wizard are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-10-16

### Added

- Focused reads: a read of a file of more than 400 lines without a range is narrowed to the definitions matching the session's task, with the file's outline beside it. Without a clear match the read is left whole.
- Shorter command outputs: Bash outputs of more than 200 lines keep their first and last lines and every error, failure and warning with the lines around them, with a note of what was left out.
- The first sentence of the comment documenting each definition, Python docstrings included, is indexed, shown in the outlines of `explain`, and used to rank files.

### Changed

- Files are also ranked by the names they use. On the benchmark's locate tasks, recall of the predicted files rose from 19% to 25%, and predictions are shown only when about half of their files are right (47% when shown, 85% with at least one right file).
- The index schema moves to version 4 and is rebuilt once to pick up documentation.

## [0.2.0] - 2026-10-16

### Changed

- Predictions rank files with BM25 over stemmed words of definition names, file names and folders, instead of substrings, so a common word no longer pulls in every name that contains it. On the benchmark's locate tasks, recall of the predicted files rose from 14% to 19%.
- A prediction is shown only when the predicted files hold the rare words of the task, or when learning vouches for them. On the benchmark tasks, predictions are now shown for 38% of prompts instead of 86%, with a recall of 34% instead of 15% when shown, and 88% of the shown ones include at least one right file.
- The `slice` tool uses the same ranking, and documentation ranks after code unless the task is about it.
- The session overview says what each tool is for, and the prediction text points to `explain` for reading a file's outline.
- Enabling a project allows the plugin's read-only tools, so Claude Code no longer asks before each first use.

### Added

- The benchmark measures exploration tokens, runs plugin mode `cold` or `warm`, and grades predictions offline with `bench/eval_predictions.py`.

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

[0.3.0]: https://github.com/Anthony14FR/Context-Wizard/releases/tag/v0.3.0
[0.2.0]: https://github.com/Anthony14FR/Context-Wizard/releases/tag/v0.2.0
[0.1.0]: https://github.com/Anthony14FR/Context-Wizard/releases/tag/v0.1.0
