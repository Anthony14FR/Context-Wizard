# Contributing to Context Wizard

Thank you for your interest in Context Wizard. This guide explains how to propose a change and what a pull request needs before it can be merged.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Security issues are never reported in public: see [SECURITY.md](SECURITY.md).

## Ways to contribute

- **Report a bug** with the bug report form. Include your OS, Claude Code version and the output of `/context-wizard:status`.
- **Propose a feature** with the feature request form. Explain the problem first, then the solution you have in mind.
- **Add framework knowledge** with a recipe. Recipes are declarative TOML files and do not require touching the engine.
- **Improve the documentation**, in English and French.

English is the reference language for code, issues, pull requests and documentation.

Context Wizard is in Phase 0. Some files and commands referenced below, such as the toolchain pin, `rustfmt.toml`, `plugin/`, the issue forms and recipes, are being added and may not exist yet.

## Development setup

Requirements:

- Rust, installed with [rustup](https://rustup.rs). The toolchain version is pinned in `rust-toolchain.toml` and installed automatically.
- Git.
- Claude Code, to run the plugin locally and to validate it.

Common commands:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
claude plugin validate --strict ./plugin
```

## Workflow

The project follows GitHub Flow. `main` is always releasable and protected.

1. Every change starts from an issue. Small typo fixes are the only exception.
2. Create a branch from an up-to-date `main`.
3. Open a draft pull request early.
4. Mark it ready for review when CI is green and the checklist is complete.
5. A maintainer reviews the pull request.
6. The pull request is merged with **Squash and merge**. The squashed commit uses the pull request title.

### Branch names

```
<type>/<issue-number>-<short-slug>
```

| Type | Use |
| --- | --- |
| `feat` | New user-facing capability |
| `fix` | Bug fix |
| `perf` | Performance improvement |
| `refactor` | Code change without behaviour change |
| `test` | Tests only |
| `docs` | Documentation only |
| `build` | Cargo, toolchain, packaging |
| `ci` | GitHub Actions |
| `chore` | Maintenance |

Branch names are lowercase, use hyphens, have at most 50 characters and carry one concern. Examples: `feat/42-personalised-pagerank`, `fix/57-windows-named-pipe`.

### Commit messages

Commits follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/). The pull request title follows the same format, since it becomes the commit on `main`.

```
<type>(<scope>): <subject>

<body: what and why, wrapped at 72 characters>

Refs: #<issue>
Signed-off-by: Name <email>
```

Scopes: `core`, `adr`, `scan`, `parse`, `graph`, `budget`, `git`, `learn`, `recipes`, `store`, `index`, `mcp`, `cli`, `hooks`, `plugin`, `install`, `release`, `bench`, `ci`, `docs`, `deps`.

- The subject uses the imperative mood, is lowercase, has no final period and has at most 72 characters.
- The body explains why. The diff already shows what.
- Breaking changes add `!` after the scope and a `BREAKING CHANGE:` footer.
- Each commit on a branch builds and passes tests. Squash fixup commits before review.

Example:

```
fix(hooks): exit early when wizard.json disables the plugin

Refs: #57
Signed-off-by: Jane Doe <jane@example.com>
```

### Developer Certificate of Origin

Every commit must be signed off under the [Developer Certificate of Origin](https://developercertificate.org/). Signing off certifies that you wrote the change or have the right to submit it under the project licences. Use `git commit -s`, and make sure the sign-off name and email match your commit author.

## Code standards

- Format with `cargo fmt` and the repository `rustfmt.toml`.
- Clippy must pass with no warnings. Workspace lints deny warnings.
- No `unwrap` or `expect` outside tests. Each crate uses typed error enums.
- No `unsafe` outside FFI boundaries. Justify each block in the pull request.
- No inline comments. Names, types and tests carry intent. Public items have rustdoc (`///`).
- Keep functions under 50 lines and files under 400 lines as a soft limit.
- Every output that reaches Claude is deterministic and covered by a snapshot test.

## Tests

| Kind | Where | Requirement |
| --- | --- | --- |
| Unit | Next to the code | Every public function |
| Integration | `crates/*/tests` against `tests/fixtures` | Every MCP tool and hook |
| Snapshot | L0, L1 and tool outputs | Reviewed diff on any change |
| Property | Graph and budget | Invariants on random inputs |
| Cross-platform | CI matrix | Linux, macOS, Windows |
| Benchmark | `bench/` | Before closing a phase, and when ranking or context output changes |

A pull request that worsens a benchmark metric by more than 5% needs maintainer approval and a written reason.

## Pull requests

- One concern per pull request, with a soft limit of 400 changed lines excluding fixtures and `Cargo.lock`.
- Complete the template checklist: linked issue, tests, documentation, both READMEs if the change is user-facing, benchmark if ranking or context output changed.
- Maintainers aim to review within 3 working days. Answer requested changes with new commits; they are squashed at merge.

## Documentation

- `README.md` (English) and `README.fr.md` (French) stay in sync. A pull request that changes one changes the other.
- Architecture decisions are recorded in `docs/adr/`. Accepted records are never edited. A new record supersedes an old one.

## Dependencies

Adding a dependency needs an issue labelled `dependency` first. A new dependency must:

1. Pass `cargo deny check licenses`. Only permissive licences are accepted.
2. Have a release in the last 12 months, or a written justification.
3. Make no network access at runtime, except grammar download and optional token-count calibration.

Under 100 lines of in-house code is preferred over a new transitive dependency tree.

## Repository hygiene

- Do not commit personal editor or tool configuration files, notes or session logs.
- Do not commit generated files, except `Cargo.lock`.
- Never commit secrets, tokens or personal data, in code, fixtures or history.
- Do not push to the `directory` branch. A workflow rebuilds it from `plugin/` on every push to `main`, with the plugin at its root and no `.gitattributes`, because Anthropic's plugin directory refuses attributes that rewrite file contents and hook paths outside the plugin root in a subfolder.

## License

By contributing, you agree that your contributions are dual licensed under the [MIT license](LICENSE-MIT) and the [Apache License, Version 2.0](LICENSE-APACHE), without any additional terms or conditions.
