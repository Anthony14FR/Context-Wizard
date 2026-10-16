"""Measure the files Context Wizard predicts for locate tasks, with no Claude session.

Each locate task lists the files of its answer, so the prediction for its prompt can be graded
directly: recall@5 is the share of those files among the five predicted ones.

Usage:
    python bench/eval_predictions.py --wizard target/release/wizard [--learning] [--tasks koel-01,...]
"""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import tomllib
from pathlib import Path


def predict(wizard: str, checkout: Path, prompt: str, learning: bool) -> dict:
    command = [wizard, "predict", "--project", str(checkout), prompt]
    if not learning:
        command.insert(2, "--no-learning")
    completed = subprocess.run(command, capture_output=True, text=True, encoding="utf-8")
    if completed.returncode != 0 or not completed.stdout.strip():
        return {"confidence": 0.0, "shown": False, "files": []}
    return json.loads(completed.stdout)


def load_tasks(arguments: argparse.Namespace) -> list[tuple[str, list[dict]]]:
    if arguments.history:
        return [
            (path.stem, json.loads(path.read_text(encoding="utf-8")))
            for path in sorted(Path(arguments.history).glob("*.json"))
        ]
    loaded = []
    for path in sorted(Path(arguments.corpus).glob("*.toml")):
        data = tomllib.loads(path.read_text(encoding="utf-8"))
        loaded.append(
            (data["repository"]["name"], [task for task in data["task"] if task["kind"] == "locate"])
        )
    return loaded


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--wizard", required=True)
    parser.add_argument("--corpus", default="bench/tasks")
    parser.add_argument("--workdir", default=".bench-work")
    parser.add_argument("--tasks", default="")
    parser.add_argument("--learning", action="store_true", help="use what past sessions taught")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--history", default="", help="folder of tasks mined by mine_history.py, graded instead of the corpus")
    arguments = parser.parse_args()
    selected = {task.strip() for task in arguments.tasks.split(",") if task.strip()}

    rows = []
    for name, tasks in load_tasks(arguments):
        checkout = Path(arguments.workdir) / name
        data = {"repository": {"name": name}}
        for task in tasks:
            if selected and task["id"] not in selected:
                continue
            prediction = predict(arguments.wizard, checkout, task["prompt"], arguments.learning)
            predicted = [file["path"] for file in prediction["files"]]
            expected = task["answer_contains"]
            hits = sum(1 for file in expected if file in predicted)
            rows.append((data["repository"]["name"], task["id"], hits / len(expected), hits > 0,
                         prediction["shown"], prediction["confidence"]))
            if arguments.verbose:
                print(f"{task['id']}: {hits}/{len(expected)} shown={prediction['shown']} "
                      f"confidence={prediction['confidence']:.2f}")
                for file in predicted:
                    print(f"    {'+' if file in expected else ' '} {file}")

    print("| Repository | Tasks | Recall@5 | Any hit | Shown | Recall@5 when shown |")
    print("| --- | --- | --- | --- | --- | --- |")
    for name in sorted({row[0] for row in rows}) + ["**All**"]:
        group = [row for row in rows if name == "**All**" or row[0] == name]
        shown = [row for row in group if row[4]]
        recall_shown = statistics.mean(row[2] for row in shown) if shown else 0.0
        print(f"| {name} | {len(group)} | {statistics.mean(row[2] for row in group):.0%} "
              f"| {sum(row[3] for row in group) / len(group):.0%} | {len(shown) / len(group):.0%} "
              f"| {recall_shown:.0%} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
