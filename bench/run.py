"""Run the Context Wizard benchmark: the same frozen tasks with and without the plugin.

Each task runs in a fresh checkout of its repository at the pinned commit, through
`claude -p` with streamed JSON output. The runner records the tokens reported by the API
(input, cache reads and writes, output), the tool calls made before the first edit, the
duration, and whether the task succeeded. Results are written as JSON and summarised as a
Markdown table.

Usage:
    python bench/run.py --mode baseline --runs 3
    python bench/run.py --mode plugin --runs 3 --plugin-dir plugin --wizard path/to/wizard
    python bench/run.py --mode baseline --tasks flask-01,flask-02 --runs 1
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
import tomllib
from dataclasses import asdict, dataclass, field
from pathlib import Path

EDIT_TOOLS = {"Edit", "Write", "NotebookEdit", "MultiEdit"}
ALLOWED_TOOLS = "Read,Grep,Glob,Edit,Write,Bash(git diff:*),Bash(git status:*)"
PLUGIN_TOOLS = "mcp__plugin_context-wizard_context-wizard__*"
BYTES_PER_TOKEN = 4


@dataclass
class Task:
    repository: str
    id: str
    kind: str
    prompt: str
    answer_contains: list[str] = field(default_factory=list)
    check: str = ""


@dataclass
class Repository:
    name: str
    url: str
    sha: str
    stack: str


@dataclass
class Result:
    task: str
    repository: str
    kind: str
    mode: str
    run: int
    success: bool
    input_tokens: int
    cache_read_tokens: int
    cache_creation_tokens: int
    output_tokens: int
    tool_calls_before_edit: int
    tool_calls: int
    exploration_tokens: int
    turns: int
    duration_ms: int
    error: str = ""

    @property
    def context_tokens(self) -> int:
        return self.input_tokens + self.cache_read_tokens + self.cache_creation_tokens


def load_corpus(directory: Path) -> tuple[dict[str, Repository], list[Task]]:
    repositories: dict[str, Repository] = {}
    tasks: list[Task] = []
    for path in sorted(directory.glob("*.toml")):
        data = tomllib.loads(path.read_text(encoding="utf-8"))
        repository = Repository(**data["repository"])
        repositories[repository.name] = repository
        for entry in data.get("task", []):
            tasks.append(Task(repository=repository.name, **entry))
    return repositories, tasks


def git(*arguments: str, cwd: Path | None = None) -> None:
    subprocess.run(["git", "-c", "core.longpaths=true", *arguments], cwd=cwd, check=True, capture_output=True)


def find_bash() -> str:
    """Return Git Bash on Windows, where `bash` on PATH is usually the WSL launcher."""
    if os.name != "nt":
        return "bash"
    candidates = [
        os.environ.get("CLAUDE_CODE_GIT_BASH_PATH", ""),
        str(Path("C:/Program Files/Git/bin/bash.exe")),
        str(Path("C:/Program Files (x86)/Git/bin/bash.exe")),
    ]
    git_path = shutil.which("git")
    if git_path:
        candidates += [str(parent / "bin" / "bash.exe") for parent in Path(git_path).resolve().parents]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return candidate
    return "bash"


def prepare_checkout(repository: Repository, workdir: Path) -> Path:
    checkout = workdir / repository.name
    if not (checkout / ".git").is_dir():
        git("clone", "--filter=blob:none", "--quiet", repository.url, str(checkout))
    git("checkout", "--quiet", "--force", repository.sha, cwd=checkout)
    git("clean", "-fdq", "-e", ".wizard", "-e", ".claude", cwd=checkout)
    return checkout


def enable_plugin(checkout: Path, wizard: Path, learning: str) -> None:
    if learning == "cold":
        for learned in (checkout / ".wizard").glob("learn.db*"):
            learned.unlink()
    subprocess.run(
        [str(wizard), "decide", "on", "--project", str(checkout)],
        check=True,
        capture_output=True,
    )
    subprocess.run(
        [str(wizard), "index", "--project", str(checkout)],
        check=True,
        capture_output=True,
    )


def run_claude(task: Task, checkout: Path, arguments: argparse.Namespace) -> tuple[list[dict], str]:
    command = [
        arguments.claude,
        "-p",
        task.prompt,
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        arguments.model,
        "--max-turns",
        str(arguments.max_turns),
        "--permission-mode",
        "acceptEdits",
        "--allowedTools",
        ALLOWED_TOOLS if arguments.mode == "baseline" else f"{ALLOWED_TOOLS},{PLUGIN_TOOLS}",
    ]
    if arguments.mode == "plugin":
        command += ["--plugin-dir", str(Path(arguments.plugin_dir).resolve())]
    completed = subprocess.run(
        command,
        cwd=checkout,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=arguments.timeout,
    )
    events = []
    for line in completed.stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return events, completed.stderr


def tool_calls(events: list[dict]) -> tuple[int, int]:
    before_edit = 0
    total = 0
    edited = False
    for event in events:
        if event.get("type") != "assistant":
            continue
        for block in event.get("message", {}).get("content", []):
            if block.get("type") != "tool_use":
                continue
            total += 1
            if block.get("name") in EDIT_TOOLS:
                edited = True
            elif not edited:
                before_edit += 1
    return before_edit, total


def exploration_tokens(events: list[dict]) -> int:
    names: dict[str, str] = {}
    for event in events:
        if event.get("type") != "assistant":
            continue
        for block in event.get("message", {}).get("content", []):
            if block.get("type") == "tool_use":
                names[block.get("id", "")] = block.get("name", "")
    size = 0
    for event in events:
        if event.get("type") != "user":
            continue
        content = event.get("message", {}).get("content", [])
        if not isinstance(content, list):
            continue
        for block in content:
            if block.get("type") != "tool_result" or names.get(block.get("tool_use_id", "")) in EDIT_TOOLS:
                continue
            body = block.get("content", "")
            if isinstance(body, list):
                body = "".join(part.get("text", "") for part in body if isinstance(part, dict))
            size += len(str(body).encode("utf-8"))
    return size // BYTES_PER_TOKEN


def succeeded(task: Task, answer: str, checkout: Path) -> bool:
    if task.kind == "locate":
        folded = answer.casefold()
        return all(expected.casefold() in folded for expected in task.answer_contains)
    completed = subprocess.run([find_bash(), "-c", task.check], cwd=checkout, capture_output=True)
    return completed.returncode == 0


def measure(task: Task, repository: Repository, run: int, arguments: argparse.Namespace) -> Result:
    checkout = prepare_checkout(repository, Path(arguments.workdir))
    if arguments.mode == "plugin":
        enable_plugin(checkout, Path(arguments.wizard), arguments.learning)
    start = time.monotonic()
    try:
        events, stderr = run_claude(task, checkout, arguments)
    except subprocess.TimeoutExpired:
        events, stderr = [], "timeout"
    final = next((event for event in reversed(events) if event.get("type") == "result"), {})
    usage = final.get("usage", {})
    before_edit, total = tool_calls(events)
    answer = final.get("result", "") or ""
    return Result(
        task=task.id,
        repository=repository.name,
        kind=task.kind,
        mode=arguments.mode,
        run=run,
        success=bool(final) and succeeded(task, answer, checkout),
        input_tokens=usage.get("input_tokens", 0),
        cache_read_tokens=usage.get("cache_read_input_tokens", 0),
        cache_creation_tokens=usage.get("cache_creation_input_tokens", 0),
        output_tokens=usage.get("output_tokens", 0),
        tool_calls_before_edit=before_edit,
        tool_calls=total,
        exploration_tokens=exploration_tokens(events),
        turns=final.get("num_turns", 0),
        duration_ms=final.get("duration_ms", int((time.monotonic() - start) * 1000)),
        error="" if final else stderr.strip()[:500],
    )


def summary(results: list[Result]) -> str:
    lines = [
        "| Repository | Mode | Runs | Success | Median context tokens | Median calls before edit | Median duration (s) |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    groups: dict[tuple[str, str], list[Result]] = {}
    for result in results:
        groups.setdefault((result.repository, result.mode), []).append(result)
    for (repository, mode), group in sorted(groups.items()):
        success = sum(result.success for result in group) / len(group)
        tokens = statistics.median(result.context_tokens for result in group)
        calls = statistics.median(result.tool_calls_before_edit for result in group)
        duration = statistics.median(result.duration_ms for result in group) / 1000
        lines.append(
            f"| {repository} | {mode} | {len(group)} | {success:.0%} | {tokens:,.0f} | {calls:.1f} | {duration:.1f} |"
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", choices=["baseline", "plugin"], required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--tasks", default="", help="comma-separated task ids; all tasks when empty")
    parser.add_argument("--corpus", default="bench/tasks")
    parser.add_argument("--workdir", default=os.environ.get("WIZARD_BENCH_WORKDIR", ".bench-work"))
    parser.add_argument("--out", default="bench/results")
    parser.add_argument("--model", default="sonnet")
    parser.add_argument("--max-turns", type=int, default=30)
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--claude", default="claude")
    parser.add_argument("--plugin-dir", default="plugin")
    parser.add_argument("--wizard", default="", help="wizard binary used to enable and index projects in plugin mode")
    parser.add_argument(
        "--learning",
        choices=["cold", "warm"],
        default="cold",
        help="cold forgets what earlier sessions taught before each one; warm keeps learning across them",
    )
    arguments = parser.parse_args()
    if arguments.mode == "plugin" and not arguments.wizard:
        parser.error("--wizard is required in plugin mode")

    repositories, tasks = load_corpus(Path(arguments.corpus))
    selected = {task_id.strip() for task_id in arguments.tasks.split(",") if task_id.strip()}
    tasks = [task for task in tasks if not selected or task.id in selected]
    Path(arguments.workdir).mkdir(parents=True, exist_ok=True)

    out = Path(arguments.out)
    out.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    suffix = arguments.mode if arguments.mode == "baseline" else f"plugin-{arguments.learning}"
    report = out / f"{stamp}-{suffix}.json"
    results: list[Result] = []
    for task in tasks:
        for run in range(1, arguments.runs + 1):
            try:
                result = measure(task, repositories[task.repository], run, arguments)
            except (subprocess.CalledProcessError, OSError) as error:
                print(f"{task.id} run {run}: skipped, {error}", flush=True)
                continue
            results.append(result)
            status = "ok" if result.success else "failed"
            print(f"{task.id} run {run}: {status}, {result.context_tokens:,} context tokens, "
                  f"{result.exploration_tokens:,} exploration tokens, "
                  f"{result.tool_calls_before_edit} calls before edit", flush=True)
            report.write_text(
                json.dumps([asdict(result) | {"context_tokens": result.context_tokens} for result in results], indent=2) + "\n",
                encoding="utf-8",
            )
    (out / f"{stamp}-{suffix}.md").write_text(summary(results), encoding="utf-8")
    print(f"\nWrote {report}")
    print(summary(results))
    return 0


if __name__ == "__main__":
    sys.exit(main())
