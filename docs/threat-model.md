# Threat model

Context Wizard runs on the developer's machine, inside Claude Code, with the developer's rights. This document lists what it protects, what it trusts, and how each threat is handled. Report a vulnerability as described in [SECURITY.md](../SECURITY.md).

## Assets

- **The developer's machine**: Context Wizard runs a native binary from every Claude Code hook.
- **Secrets in the project**: API keys, tokens and `.env` values must not reach Claude's context or the learning store.
- **The project's files**: Context Wizard reads them and must never modify them, except its own files.
- **The context sent to Claude**: what Context Wizard injects shapes what Claude does.
- **The learning data**: `.wizard/learn.db`, built over weeks of sessions and impossible to rebuild.

## Trust boundaries

| Source | Trust |
| --- | --- |
| The released binary, verified against `binaries.lock.json` | Trusted once its SHA-256 matches |
| The plugin files installed by Claude Code | Trusted, as Claude Code installs them from the marketplace |
| Project files, including project recipes, `.claude/wizard.json` and `.claude/settings.local.json` | Untrusted: a cloned repository may be hostile |
| Hook input from Claude Code | Trusted in structure, untrusted in content (prompts, tool results) |
| Language servers on the `PATH` | Trusted as much as any program the developer installed |
| The network | Untrusted |

## Threats and mitigations

### A tampered binary

The SessionStart launcher downloads the binary over HTTPS from the GitHub release named in `binaries.lock.json`. It checks the file's SHA-256 against the pinned value before making it executable, and installs it with an atomic rename, so an interrupted or altered download never runs. Pinned checksums are written by the release workflow from the binaries it built, and the workflow pins every action to a commit. The launcher honours `HTTPS_PROXY` and `NO_PROXY` without disabling verification.

### Secrets reaching Claude or the disk

- Names and signatures extracted from code go through secret redaction before they are stored: provider token formats, private key blocks, JSON Web Tokens, credentials in URLs, and quoted values assigned to names such as `password` or `api_key`.
- `.env` files keep their keys only, never their values.
- Prompts, search patterns and recipe facts are redacted before they enter `learn.db`, and prompts are capped at 4,000 characters.
- `wizard calibrate`, the only command that sends file contents anywhere, skips `.env` files and every file in which redaction finds something. It runs only when the developer starts it with their own API key.
- `.wizard/` holds a `.gitignore` that ignores everything, so the index and the learning store are never committed, even in a project that does not ignore them.

### A hostile repository

A cloned project can contain crafted files, recipes, decision files and settings.

- **Project recipes** are data: TOML, glob patterns, regular expressions and tree-sitter queries. None of them can run code. The regular expression engine runs in linear time, and query files must sit inside the recipe directory (`..`, absolute paths and drive letters are refused).
- **Parsers** of recipes, decision files, settings files, cached structures and lexical content are fuzzed with property tests on arbitrary input. They return errors and never panic.
- **Settings**: Context Wizard writes `.claude/settings.local.json` only to add or remove its own `enabledPlugins` entry. A file that is not a JSON object is left untouched, and every write is atomic.
- **Opt-in**: nothing is indexed, learned or injected before the developer answers the opt-in question for that project. A committed `.claude/wizard.json` reflects a choice the team made, and `/context-wizard:off` overrides it locally.
- **Size limits**: files over 1 MiB and binary files are skipped, and every answer and injection has a size or token cap.

### Prompt injection through code

Text extracted from the project, such as names, signatures, headings and facts, reaches Claude's context, as the project's code does when Claude reads it directly. Context Wizard does not make this worse: it injects short, structured, capped lines (facts are capped at 200 characters), labelled as coming from Context Wizard, and never instructions of its own beyond the opt-in question. Claude Code's permission system still applies to every action Claude takes.

### Running other programs

- Hooks run only the Context Wizard binary.
- `exact_refs` starts a language server only when Claude calls that tool. Only a known server name (such as `rust-analyzer` or `gopls`) is looked up on the `PATH`. Nothing is downloaded or installed, and the server is stopped after the answer.
- Git history is read with the `git` command line, with fixed arguments.

### Network use

Context Wizard contacts the network in three cases only:

1. the launcher's binary download, at most once per version;
2. a daily check of the latest release on GitHub, skipped when `DISABLE_UPDATES`, `DISABLE_AUTOUPDATER` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` is set, and run in a detached process so no hook waits on it;
3. the grammar bundle of tree-sitter-language-pack, downloaded on first use and checked against its manifest.

`wizard calibrate` is a fourth, explicit case.

### Losing the learning data

`learn.db` uses SQLite transactions and WAL. Schema changes only add to it, after a copy to `learn.db.v<version>.bak`. A store written by a newer version is left untouched, and a corrupt store is renamed instead of deleted. See [ADR 0009](adr/0009-data-upgrades.md).

### Denial of service

Hooks have Claude Code timeouts and never wait on the network. Index refreshes are incremental and bounded by file size limits. The MCP server refreshes at most every two seconds, and language servers have a 60-second limit.

## Dependencies

`cargo deny` checks licences, advisories, banned crates and sources on every pull request, and a weekly audit runs on the default branch. New dependencies need an issue with a justification, as the contributing guide describes.

## Out of scope

- A compromised Claude Code installation, operating system or developer account.
- A language server or `git` binary the developer installed and that is itself malicious.
- Secrets in formats that redaction does not recognise. Report new formats as security issues.
