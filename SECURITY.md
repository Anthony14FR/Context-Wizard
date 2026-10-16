# Security Policy

## Supported versions

Before 1.0, only the latest minor release receives security fixes.

| Version | Supported |
| --- | --- |
| Latest `0.x` minor | Yes |
| Older releases | No |

## Reporting a vulnerability

Do not open a public issue for a security problem.

Report it privately through [GitHub private vulnerability reporting](https://github.com/Anthony14FR/Context-Wizard/security/advisories/new). Include:

- the affected version and platform,
- a description of the issue and its impact,
- steps to reproduce, or a proof of concept,
- any known mitigation.

You receive an acknowledgement within 72 hours. The fix is developed in a private advisory, released as a patch version, and the advisory is published once users can update. Reporters are credited unless they ask otherwise.

## Scope

Context Wizard runs locally inside Claude Code. The most relevant areas are:

- the binary bootstrap: download, SHA-256 verification and installation,
- secret filtering: keeping credentials and `.env` values out of the context sent to Claude,
- parsing of untrusted project files, recipes and configuration,
- the local data written in `.wizard/` and `.claude/`,
- the language servers started for exact references.

The [threat model](docs/threat-model.md) describes the assets, trust boundaries and mitigations.

Vulnerabilities in Claude Code itself should be reported to Anthropic, not to this project.
