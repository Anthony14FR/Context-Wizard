# Writing a recipe

A recipe teaches Context Wizard the conventions of a framework: where the routes are, how models relate, which files are pages. It turns them into one-line facts with a file and a line, which Claude reads through the `facts` MCP tool and finds in the project overview. A recipe is plain data: a TOML file and, when needed, tree-sitter query files. Adding one never requires recompiling.

Context Wizard ships recipes for Laravel, React, Next and Expo in [`recipes/`](../recipes). You can add your own for a single project, or contribute one for everybody.

## Where recipes live

| Location | Scope |
| --- | --- |
| `recipes/<name>/` in this repository | Built into the binary, available in every project |
| `.claude/context-wizard/recipes/<name>/` in a project | That project only. Commit it to share it with your team |

A project recipe with the same `name` as a built-in one replaces it. Recipes are read again at every index refresh, so an edited recipe applies to the next answer without restarting anything.

## Format

```toml
name = "laravel"
description = "Routes, Eloquent relations and the database schema from migrations"

[detect]
files = ["artisan"]
contains = [{ file = "composer.json", text = "laravel/framework" }]

[[extract]]
name = "routes"
title = "Routes"
glob = ["routes/**/*.php"]
language = "php"
query = "routes.scm"
template = "{verb} {uri} -> {action}"
```

### `[detect]`

The recipe applies to the project root and to every sub-project root, the folders holding a manifest such as `package.json` or `composer.json`, where all the conditions hold:

- `files`: paths that must exist, relative to that root.
- `contains`: files that must contain a text, such as a dependency name.

At least one condition is required.

### `[[extract]]`

Each extraction selects files and produces facts from them.

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Identifier, unique within the recipe, such as `routes` |
| `title` | no | Heading shown to Claude, such as `Routes`. Defaults to `name` |
| `glob` | yes | Patterns of the files to read, relative to the detected root. `**` crosses folders, `{a,b}` is an alternative |
| `exclude` | no | Patterns of files to skip among those |
| `language` | with `query` | Tree-sitter language of the query, such as `php`, `tsx`, `typescript` or `javascript` |
| `query` | one of the two | Query file next to `recipe.toml`. Each match of the query is one fact |
| `path_pattern` | one of the two | Regular expression on the file path. Its named groups become values, and each matching file is one fact |
| `template` | yes | The fact, with `{name}` replaced by the capture or group `name` |

A template can use the captures of the query, the named groups of `path_pattern`, and `{file}`, the path of the file. `{name?}` is optional: the fact is kept with an empty value when the capture is missing, while a missing `{name}` drops the fact. Captures whose name starts with `_` serve the query's predicates and are not available to the template. Values lose their surrounding quotes and their line breaks, and secrets in them are redacted.

## Worked example: Symfony console commands

Say a project uses Symfony and you want Claude to know its console commands. A command looks like this:

```php
#[AsCommand(name: 'app:sync-catalog', description: 'Synchronise the catalogue')]
final class SyncCatalogCommand extends Command
{
}
```

### 1. Create the recipe

Create `.claude/context-wizard/recipes/symfony/recipe.toml`:

```toml
name = "symfony"
description = "Console commands"

[detect]
files = ["bin/console"]
contains = [{ file = "composer.json", text = "symfony/console" }]

[[extract]]
name = "commands"
title = "Console commands"
glob = ["src/**/*Command.php"]
language = "php"
query = "commands.scm"
template = "{command} -> {class}"
```

### 2. Write the query

Tree-sitter queries match the syntax tree of a file. To see the tree of a sample, paste it into the [tree-sitter playground](https://tree-sitter.github.io/tree-sitter/7-playground.html) with the PHP grammar. The attribute above becomes an `attribute` node holding `arguments`, and the class a `class_declaration` with a `name`.

Create `.claude/context-wizard/recipes/symfony/commands.scm`:

```scheme
(class_declaration
  attributes: (attribute_list
    (attribute_group
      (attribute
        (name) @_attribute (#eq? @_attribute "AsCommand")
        parameters: (arguments
          (argument
            name: (name) @_key (#eq? @_key "name")
            (string) @command)))))
  name: (name) @class)
```

`@command` and `@class` feed the template. `@_attribute` and `@_key` only serve the `#eq?` predicates, which keep the matches where the attribute is `AsCommand` and the argument is `name`. Tree-sitter also supports `#match?` with a regular expression, and `#not-match?`.

### 3. Check it

```bash
wizard recipes --project .
```

The command lists every recipe, where it applies and what it extracts, and fails with the exact problem when a recipe is broken:

```text
recipe `symfony`, extract "commands", template: `{comand}` is neither a query capture nor a path group
```

Run it again until it reports no problem. Every problem is reported at once, including unknown fields, invalid globs, queries that do not compile and placeholders that nothing provides.

### 4. See the facts

The next index refresh applies the recipe, and the overview gains a line such as:

```text
Framework facts (the `facts` tool lists them):
- symfony: 12 console commands
```

Claude can list them with the `facts` tool and the kind `commands`:

```text
## Console commands (symfony), 12 found
- src/Command/SyncCatalogCommand.php:4 app:sync-catalog -> SyncCatalogCommand
```

## Recipes from paths

Some conventions live in the file layout rather than in the code. File routes are one example. `path_pattern` covers them without a query:

```toml
[[extract]]
name = "app-pages"
title = "Pages (app router)"
glob = ["app/**/page.{js,jsx,ts,tsx,mdx}"]
path_pattern = '^app/(?P<route>(?:.+/)?)page\.[a-z]+$'
template = "page /{route}"
```

Use single quotes in TOML for regular expressions, so backslashes stay as they are.

## Contributing a recipe

1. Add `recipes/<name>/` with `recipe.toml` and its query files.
2. List the files in `crates/wizard-recipes/src/builtin.rs`.
3. Add a test in `crates/wizard-recipes/src/tests.rs` with a small sample of each construct the recipe extracts.
4. Open a pull request using the recipe request template, with a public project where the recipe was tried.

A good recipe extracts what a developer would look for first in an unfamiliar project of that framework, in a form short enough to list hundreds of facts in one answer.
