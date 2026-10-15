# 4. In-house graph ranking and token estimator

- Status: Accepted
- Date: 2026-10-15

## Context

Context Wizard ranks files and symbols with PageRank over a graph of imports, references, git co-change and learned co-access, then selects what fits a token budget. Two existing crates were evaluated for these needs.

- **`petgraph` 0.8.3.** Its `page_rank` takes no personalisation vector, so it cannot bias the ranking towards the current task, recent files or learned weights. Its implementation is O(n·V²·E), which does not scale to large repositories.
- **`tiktoken-rs`.** It implements OpenAI tokenizers. Claude's tokenizer is not public, and the gap between OpenAI counts and Claude counts is larger on source code. Recent Claude models also produce about 30% more tokens than older ones for the same text.

Exact Claude token counts are only available from Anthropic's `count_tokens` endpoint, which requires an API key and a network call.

## Decision

Both components are built in-house, with no dependency.

**Ranking.** The graph is stored in compressed sparse row (CSR) form. PageRank runs as a personalised power iteration that stops at convergence or at an iteration cap. The personalisation vector combines task keywords, recently touched files and learned weights. The code stays around a hundred lines and is covered by property tests on small graphs.

**Token estimator.** Tokens are estimated from bytes, with bytes-per-token ratios per language and per model family. The target accuracy is ±10%. Users with an Anthropic API key can calibrate the ratios through `count_tokens`. This is optional and is the only network call besides the binary bootstrap, grammar downloads and the update check.

The benchmark never uses the estimator: it counts tokens with `count_tokens`.

## Consequences

- Ranking can use task context, which is what makes the map relevant to the work at hand.
- Memory and time stay linear in the number of edges per iteration.
- Budgets are approximate. The estimator error is bounded by the ±10% target and is checked against `count_tokens` in the benchmark.
- The ratios need maintenance when new model families change tokenisation.
