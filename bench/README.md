# Benchmark

The benchmark measures what Context Wizard changes for Claude Code on real codebases: tokens spent on context, tool calls made before the first edit, duration, and whether the task succeeds.

## Corpus

Six public repositories of different technologies, each pinned to a commit, with ten frozen tasks per repository in [`tasks/`](tasks):

| Repository | Stack |
| --- | --- |
| [koel/koel](https://github.com/koel/koel) | Laravel (PHP) with a Vue front end |
| [bluesky-social/social-app](https://github.com/bluesky-social/social-app) | React Native with Expo (TypeScript) |
| [pallets/flask](https://github.com/pallets/flask) | Python |
| [junegunn/fzf](https://github.com/junegunn/fzf) | Go |
| [spring-projects/spring-petclinic](https://github.com/spring-projects/spring-petclinic) | Java with Spring Boot and Maven |
| [pester/Pester](https://github.com/pester/Pester) | PowerShell |

Each repository has seven `locate` tasks, which succeed when the final answer contains the expected paths or symbols, and three `edit` tasks, which succeed when a check command exits with status 0 after the session.

## Method

- Every run starts from a clean checkout of the pinned commit.
- The same task runs with the same model in two modes: `baseline`, without the plugin, and `plugin`, with the project enabled and already indexed, which is the steady state of a project in daily use.
- Token counts are the usage reported by the API for each session: input, cache reads, cache writes and output. They are exact and need no tokenizer. Most of them are Claude Code's own system prompt and tool definitions, read again at every turn, which the plugin cannot change.
- Exploration tokens are the size of what Read, Grep, Glob and the Context Wizard tools returned, at four bytes per token. They are what the plugin aims to reduce.
- In plugin mode, the Context Wizard MCP tools are allowed like the built-in read-only tools, as the plugin allows them when a project is enabled.
- Each task runs three times per mode, and the report uses medians.
- User-level plugins and settings of the machine running the benchmark load in both modes alike.

## Running

Requirements: Python 3.11 or later, Git, Bash, and an authenticated Claude Code. A full campaign runs 6 × 10 × 3 sessions per mode.

```sh
python bench/run.py --mode baseline --runs 3
python bench/run.py --mode plugin --runs 3 --plugin-dir plugin --wizard target/release/wizard
python bench/compare.py bench/results/<baseline>.json bench/results/<plugin>.json
```

`--tasks flask-01,flask-08` restricts a run to some tasks, and `--model` selects the model. Reports are written to `bench/results/`.
