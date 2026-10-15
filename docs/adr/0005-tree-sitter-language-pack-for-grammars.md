# 5. tree-sitter-language-pack for grammars

- Status: Accepted
- Date: 2026-10-15

## Context

Context Wizard must add value on any project, whatever its technology. Syntax-level extraction (definitions, references, imports, signatures) needs a parser per language. tree-sitter is the standard choice: incremental, fast, with tags queries already written for most languages. The question is how to obtain grammars.

Options evaluated on 2026-10-15:

| Option | Assessment |
| --- | --- |
| Individual grammar crates (`tree-sitter-php`, `tree-sitter-typescript`, ...) | One crate per language, compiled into the binary. Uneven maintenance: `tree-sitter-typescript` has had no release since November 2024. Covering any technology would mean compiling in many separate grammars |
| `arborium` 2.12.4 | Depends on a patched fork of tree-sitter, about 70 languages |
| `tree-sitter-language-pack` 1.20.0 | 371 languages, permissive grammars only, grammars downloaded on first use and cached |

## Decision

Grammars come from `tree-sitter-language-pack`, on top of the official `tree-sitter` runtime (0.27.1).

- A grammar is downloaded the first time a language is met, then cached in `${CLAUDE_PLUGIN_DATA}`, which survives plugin updates.
- Without network access, or for a language without a grammar, the file falls back to the in-house lexical extractor (identifiers, headings, top-level keys).
- The pack version is pinned in `Cargo.toml` once integration tests pass on the fixture repositories. Its documentation build failed on docs.rs for 1.20.0, so the pinned version is chosen by tests, not by documentation.

## Consequences

- The binary stays small, and support for hundreds of languages comes without compiling each grammar in.
- Coverage degrades gracefully: syntax when a grammar is available, lexical otherwise, universal signals always.
- The first use of a language needs network access. This is the main runtime network call and is documented as such.
- Updating the pack can change extraction output. Snapshot tests on the fixtures catch such changes before they reach users.
