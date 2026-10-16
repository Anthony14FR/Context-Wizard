# Privacy policy

Last updated: 2026-10-16

Context Wizard is a free, open source plugin for Claude Code. It runs entirely on your machine. This policy describes what it stores and what it sends.

## What Context Wizard collects

Nothing is collected by the project. There is no server, no account, no analytics and no telemetry. The maintainers receive no data from your use of the plugin.

## What it stores on your machine

In each project where you enable it:

- `.claude/wizard.json`: your decision to enable or disable it.
- `.claude/settings.local.json`: an entry that disables the plugin when the project says no, and a rule that allows the plugin's read-only tools when it says yes.
- `.wizard/`: the index of the project (file hashes, names, signatures and line numbers), and the learning store. The learning store holds the prompts of your sessions with secrets redacted, the files Claude read, searched and edited, and the predictions made. It holds a `.gitignore` that ignores everything, so it is never committed.

In the plugin's data folder (`~/.claude/plugins/data/`): the downloaded binaries, the date of the last release check, and grammar files.

You can delete the project data at any time with `/context-wizard:purge`. Uninstalling the plugin with `claude plugin uninstall` deletes its data folder.

## What it sends, and to whom

| Request | Recipient | Content | When |
| --- | --- | --- | --- |
| Binary download | GitHub (github.com) | A download request for the release file of your platform | Once per version |
| Release check | GitHub API (api.github.com) | A request for the latest release, with no data about you or your project | At most once a day, skipped when `DISABLE_UPDATES`, `DISABLE_AUTOUPDATER` or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` is set |
| Grammar download | GitHub (github.com) | A download request for the grammar bundle of your platform | On first use |
| Token calibration | Anthropic API (api.anthropic.com) | A sample of project files, never files containing secrets, with your own API key | Only when you run `wizard calibrate` yourself |

GitHub and Anthropic process these requests under their own privacy policies. Like any HTTP request, they see your IP address.

The context Context Wizard prepares is sent to Claude by Claude Code as part of your session, under your agreement with Anthropic, like any other content of the session.

## Changes

Changes to this policy are made in this file, in the public repository, and listed in its history.

## Contact

Open an issue at https://github.com/Anthony14FR/Context-Wizard/issues, or report a security problem privately as described in [SECURITY.md](../SECURITY.md).
