"""Turn the git history of the benchmark repositories into locate tasks, for offline grading.

Each commit before a repository's pinned commit is a real task: its message says what changed,
in the words of the people who work on the project, and the files it modified are the answer.
Commits that are merges, releases, dependency bumps or formatting, or that touch fewer than two
or more than six source files still present at the pinned commit, are left out.

Usage:
    python bench/mine_history.py --out .bench-work/history [--per-repository 300]
Then:
    python bench/eval_predictions.py --wizard <binary> --history .bench-work/history
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import tomllib
from pathlib import Path

SKIP = re.compile(
    r"^(merge|revert|release|bump|chore|ci|build|deps|style|format|lint|typo|version|v?\d+\.\d+)",
    re.IGNORECASE,
)
TEST = re.compile(r"(test|spec|__mocks__|fixtures?)", re.IGNORECASE)
SOURCE = re.compile(
    r"\.(py|go|php|js|jsx|ts|tsx|vue|java|kt|rb|rs|cs|ps1|psm1|swift|c|cc|cpp|h|hpp|scala|ex|exs|dart|sql|html|properties|yml|yaml)$"
)


def git(checkout: Path, *arguments: str) -> str:
    return subprocess.run(
        ["git", *arguments], cwd=checkout, capture_output=True, text=True, encoding="utf-8", check=True
    ).stdout


def mine(checkout: Path, sha: str, limit: int) -> list[dict]:
    present = set(git(checkout, "ls-tree", "-r", "--name-only", sha).splitlines())
    log = git(checkout, "log", "--no-merges", "--format=%x1e%H%x1f%s%x1f%b", "--name-only", f"{sha}~1", "-n", str(limit * 8))
    tasks = []
    for record in log.split("\x1e"):
        if not record.strip():
            continue
        header, _, files = record.partition("\n\n") if "\n\n" in record else (record, "", "")
        parts = header.split("\x1f")
        if len(parts) < 3:
            continue
        commit, subject, body = parts[0].strip(), parts[1].strip(), parts[2].strip()
        subject = re.sub(r"^\w+(\([^)]*\))?!?:\s*", "", subject)
        if len(subject) < 20 or SKIP.match(subject):
            continue
        changed = [
            name.strip()
            for name in (files or body).splitlines()
            if SOURCE.search(name.strip()) and name.strip() in present and not TEST.search(name.strip())
        ]
        changed = sorted(set(changed))
        if not 2 <= len(changed) <= 6:
            continue
        prompt = subject
        first = next((line.strip() for line in body.splitlines() if line.strip() and not line.startswith(("Signed-off-by", "Co-authored-by"))), "")
        if first and len(first) < 300 and not SOURCE.search(first):
            prompt = f"{subject}. {first}"
        tasks.append({"id": commit[:10], "prompt": prompt, "answer_contains": changed})
        if len(tasks) == limit:
            break
    return tasks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", default="bench/tasks")
    parser.add_argument("--workdir", default=".bench-work")
    parser.add_argument("--out", default=".bench-work/history")
    parser.add_argument("--per-repository", type=int, default=300)
    arguments = parser.parse_args()
    out = Path(arguments.out)
    out.mkdir(parents=True, exist_ok=True)
    for path in sorted(Path(arguments.corpus).glob("*.toml")):
        repository = tomllib.loads(path.read_text(encoding="utf-8"))["repository"]
        checkout = Path(arguments.workdir) / repository["name"]
        tasks = mine(checkout, repository["sha"], arguments.per_repository)
        (out / f"{repository['name']}.json").write_text(json.dumps(tasks, indent=1) + "\n", encoding="utf-8")
        print(f"{repository['name']}: {len(tasks)} tasks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
