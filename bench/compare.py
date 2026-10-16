"""Compare a baseline benchmark report with a plugin report.

Usage:
    python bench/compare.py bench/results/<baseline>.json bench/results/<plugin>.json
"""

from __future__ import annotations

import json
import statistics
import sys
from pathlib import Path


def load(path: str) -> list[dict]:
    return json.loads(Path(path).read_text(encoding="utf-8"))


def medians(results: list[dict]) -> tuple[float, float, float, float]:
    return (
        statistics.median(result.get("exploration_tokens", 0) for result in results),
        statistics.median(result["context_tokens"] for result in results),
        statistics.median(result["tool_calls_before_edit"] for result in results),
        sum(result["success"] for result in results) / len(results),
    )


def change(before: float, after: float) -> str:
    if before == 0:
        return "n/a"
    return f"{(after - before) / before:+.0%}"


def row(name: str, baseline: list[dict], plugin: list[dict]) -> str:
    base_explored, base_tokens, base_calls, base_success = medians(baseline)
    explored, tokens, calls, success = medians(plugin)
    return (
        f"| {name} | {base_explored:,.0f} | {explored:,.0f} | {change(base_explored, explored)} "
        f"| {base_tokens:,.0f} | {tokens:,.0f} | {change(base_tokens, tokens)} "
        f"| {base_calls:.1f} | {calls:.1f} | {change(base_calls, calls)} "
        f"| {base_success:.0%} | {success:.0%} |"
    )


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    baseline, plugin = load(sys.argv[1]), load(sys.argv[2])
    print("| Repository | Exploration without | Exploration with | Change | Context without | Context with | Change | Calls without | Calls with | Change | Success without | Success with |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for name in sorted({result["repository"] for result in baseline}):
        before = [result for result in baseline if result["repository"] == name]
        after = [result for result in plugin if result["repository"] == name]
        if before and after:
            print(row(name, before, after))
    print(row("**All**", baseline, plugin))
    return 0


if __name__ == "__main__":
    sys.exit(main())
