# 2. Single Rust binary with CLI and MCP modes

- Status: Accepted
- Date: 2026-10-15

## Context

Context Wizard has two kinds of callers:

- **Hooks.** Claude Code runs a command on `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse` and `Stop`. Each call must finish quickly: a slow hook delays every prompt and every tool call. The target is under 50 ms at p95 when the plugin is enabled, and under 5 ms when it is disabled.
- **MCP tools.** Claude queries the index on demand (`overview`, `find_symbol`, `refs`, `slice`, `impact`, `explain`). Answers must come from a warm index, not from a fresh parse of the project.

The project must run on Linux, macOS and Windows, on x86_64 and aarch64, without asking users to install a runtime such as Node.js or Python.

## Decision

Context Wizard ships one Rust binary, `wizard`, with two modes:

- **CLI mode**, short-lived, called by hooks through the plugin launchers. It reads the per-project decision first and exits immediately when the plugin is disabled.
- **Server mode**, long-lived, started by Claude Code as the plugin's MCP server. It holds the index in memory and also listens on a local socket: a Unix domain socket on Linux and macOS, a named pipe on Windows.

Hooks that need the index connect to the running server through the socket instead of loading the index themselves. A lock file guarantees one server per project. Socket and lock names include the binary version, so different versions never talk to each other. When the server cannot be reached within 200 ms, the hook aborts silently and Claude Code continues as without the plugin.

The binary is split into workspace crates by responsibility (`wizard-core`, `wizard-scan`, `wizard-parse`, `wizard-graph`, `wizard-budget`, `wizard-git`, `wizard-learn`, `wizard-recipes`, `wizard-store`, `wizard-mcp`, `wizard-cli`), and only `wizard-cli` produces the executable.

## Consequences

- Users download a single self-contained executable per platform. No runtime is required.
- Hook latency stays low because heavy work happens once in the server, not on every call.
- Cross-platform socket handling and the server lifecycle (lock, stale lock recovery, version mismatch) become part of the codebase and need dedicated tests on all three operating systems.
- The release pipeline must build six targets and pin each checksum.
