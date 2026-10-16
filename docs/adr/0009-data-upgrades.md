# 9. Data upgrades

- Status: Accepted
- Date: 2026-10-16

## Context

Context Wizard keeps three kinds of data per project, and a new release may change the format of any of them:

- `.claude/wizard.json`, the opt-in decision, committed with the project.
- `.wizard/index.db`, the index, rebuilt from the project files in seconds.
- `.wizard/learn.db`, the learning store: events, outcomes, predictions and grades. It holds what past sessions taught and cannot be rebuilt.

Users update the plugin through Claude Code, possibly in one project while another session still runs the previous version, and may go back to an older version.

## Decision

- **Decision file**: its format only grows with optional fields, and unknown fields are ignored, so every version reads every other version's file.
- **Index**: it is a cache. A different schema version, newer or older, drops and rebuilds it. A corrupt file is deleted and rebuilt. The cached file graph is versioned by its own name and rebuilt when it cannot be decoded.
- **Learning store**: each schema change is a numbered migration that only adds to the store. Before migrating, the store is copied with `VACUUM INTO` to `learn.db.v<old version>.bak`, which is never overwritten. A store written by a newer version is left untouched: learning pauses in that project instead of losing data, and the newer version resumes it. A corrupt store is renamed with a `.corrupt-<time>` suffix before a fresh one is created.
- **Binary and plugin**: hooks and the MCP server stay inert when their version differs from the plugin's (IU-05), so data is never written by a binary the plugin does not expect.

## Consequences

- Going back to an older version costs one index rebuild and pauses learning, and loses nothing.
- Each migration of the learning store is covered by a test that opens a store of the previous version and checks its events survive, along with the backup.
- Backups accumulate one file per schema version, which `/context-wizard:purge` removes with the rest of the project data.
