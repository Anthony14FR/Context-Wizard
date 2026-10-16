# 8. Hooks read the index directly

- Status: Accepted
- Date: 2026-10-16
- Supersedes: the local socket and single index server described in [ADR 0002](0002-single-rust-binary-with-cli-and-mcp-modes.md)

## Context

ADR 0002 planned one long-lived index server per project, guarded by a lock file, with hooks reaching it through a Unix domain socket or a named pipe to stay within the hook latency budget.

Measurements on the implementation changed the picture:

- The index is a SQLite database in WAL mode, indexed by path and by symbol name. A hook that opens it and answers a symbol lookup on a 3,500-file project takes 39 ms at p50 and 41 ms at p95 on Windows, of which 33 ms is starting the process. Hooks that only read the decision take 35 ms at p95.
- On Windows, process start time grows with the size of the executable: 9 ms for 230 KB, 33 ms for 9 MB with the same code. A socket would not remove that cost, since the hook process starts anyway.
- Claude Code starts one MCP server per session. Several sessions on one project already share the index safely through SQLite transactions.

## Decision

- Hooks open the index read-only for lookups and never wait on another process.
- Each session's MCP server refreshes the index incrementally before answering, at most every two seconds; `SessionStart` refreshes it before rendering the overview. Concurrent writers are serialised by SQLite.
- There is no socket, no lock file and no shared server. The version handshake of each process replaces the versioned socket names.
- Selection under a token budget is a greedy fill by score, line by line, which the overview and the map implement and test.

## Consequences

- No cross-platform socket code, no stale lock recovery, no server lifecycle to manage.
- Hook latency is bounded by process start and one SQLite query, within the 50 ms target on every platform measured.
- Two sessions may refresh the index at the same moment; the second one finds nothing to do.
