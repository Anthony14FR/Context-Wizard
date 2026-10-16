# 7. Git signals through the git command line

- Status: Accepted
- Date: 2026-10-16

## Context

The ranking uses three signals from version control: how often each file changes (churn), which files change together (co-change), and renames, so that the history of a file counts under its current name. The development plan named `gix` for this, a pure Rust implementation of Git.

Reading a few hundred recent commits with their changed paths is all Context Wizard needs. `gix` brings a large dependency tree, which lengthens builds of the six release targets and increases the audit surface, and its rename detection would have to match Git's own. Git is installed wherever a repository is worked on.

## Decision

History is read by running `git log --no-merges --relative -M --name-status` on the most recent commits, 500 by default, and parsing its output. Rename detection is Git's own. Commits touching more than 30 files are left out of co-change. When `git` is missing or the project is not a repository, there are no history signals and ranking relies on the other signals.

## Consequences

- No new dependency, and the signals match what developers see with Git.
- History reading costs one process spawn per index build. It never runs in a hook.
- Context Wizard depends on a `git` executable for these signals only; everything else works without it.
- Parsing is covered by unit tests on recorded output and by an integration test on a repository created for the test.
