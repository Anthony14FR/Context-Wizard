# 1. Record architecture decisions

- Status: Accepted
- Date: 2026-10-15

## Context

Context Wizard is a public project that combines a Claude Code plugin, a Rust binary, a learning model and a distribution pipeline. Many of its decisions are not obvious from the code: why a dependency was rejected, why a component is built in-house, why a hook does one thing and not another. Contributors who join later need that reasoning, and maintainers need a stable record to avoid reopening settled questions without new facts.

## Decision

Architecture decisions are recorded as Architecture Decision Records (ADRs), in the format described by Michael Nygard, in `docs/adr/`.

- One decision per file, named `NNNN-title-in-kebab-case.md`, numbered sequentially.
- Each record has a status, a date, and the sections Context, Decision and Consequences.
- Statuses are `Proposed`, `Accepted`, `Superseded by NNNN` or `Deprecated`.
- An accepted record is never edited, except to change its status. A changed decision is a new record that supersedes the old one.
- A record is added in a pull request, ideally the one that introduces the change it describes.

## Consequences

- The reasoning behind the architecture is versioned with the code and reviewed like it.
- Reopening a decision requires writing a new record with the new facts, which keeps discussions focused.
- The log grows over time. Superseded records stay in place, so the history of a decision remains readable.
