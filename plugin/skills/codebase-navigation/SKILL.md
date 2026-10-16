---
name: codebase-navigation
description: Navigate a project indexed by Context Wizard with its MCP tools instead of broad searches and bulk reads. Use when locating code, starting a task in an unfamiliar area, planning a change, or checking what a change affects.
---

# Navigating with Context Wizard

The project overview injected at session start already names the stack, entry points, commands and folders. Start from it instead of listing directories. When a prompt arrives with predicted files, read those first: they come from the code structure and from what past sessions edited for similar tasks.

Prefer the Context Wizard MCP tools to Grep, Glob and bulk Read when the question is about code structure:

| Question | Tool |
| --- | --- |
| Where is `name` defined? | `find_symbol` with the name |
| Where is `name` used, before renaming or changing its signature? | `refs` with the name, or `exact_refs` when unrelated symbols share the name |
| Which files matter for this task? | `slice` with the task in a few specific words |
| What does this file contain, without reading all of it? | `explain` with the path |
| What could break if I change this file? | `impact` with the path |
| Where does the important code live? | `map` |
| Which routes, models, schema, components, pages or screens exist? | `facts`, optionally with a kind such as `routes` |

Then read only the lines you need, with an offset and a limit around the line numbers the tools give.

Keep Grep for text that is not a symbol name: messages, comments, configuration values, string literals.

The tools refresh the index before answering, so their paths and line numbers match the current files. If a tool says Context Wizard is not enabled, continue with the usual tools.
