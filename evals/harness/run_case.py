#!/usr/bin/env python3
"""Run one self-contained eval case and write a JSON report."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CASES_ROOT = REPO_ROOT / "evals" / "cases"
REPORTS_ROOT = REPO_ROOT / "evals" / "reports" / "runs"
MAX_OUTPUT = 4000


def parse_field(text: str, name: str) -> str | None:
    prefix = f"{name}: "
    for line in text.splitlines():
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    return None


def parse_usage(text: str) -> dict[str, int] | None:
    match = re.search(
        r"^usage: (?:calls=\d+ )?input=(\d+) output=(\d+)"
        r"(?: cached=(\d+) hit=[\d.]+%)? total=(\d+)$",
        text,
        re.M,
    )
    if not match:
        return None
    return {
        "input_tokens": int(match.group(1)),
        "output_tokens": int(match.group(2)),
        "cached_tokens": int(match.group(3) or 0),
        "total_tokens": int(match.group(4)),
    }


def parse_call_tools(text: str) -> list[int]:
    return [int(match) for match in re.findall(r"^call: \d+ .* tools=(\d+)$", text, re.M)]


def collect_mechanisms(state_home: Path, timeout: int) -> dict:
    """Read the persisted event stream and report what the run exercised."""
    store = state_home / "keel.db"
    if not store.exists():
        return {}
    command = [
        "cargo", "run", "-q", "-p", "keel-cli", "--",
        "--store", str(store), "--view", "events",
    ]
    try:
        cli = subprocess.run(
            command,
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except (OSError, subprocess.SubprocessError):
        return {}
    if cli.returncode != 0 or not cli.stdout.strip():
        return {}
    try:
        rows = json.loads(cli.stdout)
    except json.JSONDecodeError:
        return {}

    counts = {
        "model_calls": 0,
        "tool_calls": 0,
        "continuations": 0,
        "budget_exhausted": False,
        "wrap_ups": 0,
        "final_calls": 0,
        "permission_allow": 0,
        "permission_confirm": 0,
        "permission_deny": 0,
    }
    for row in rows:
        name = row.get("name")
        if name == "model_started":
            counts["model_calls"] += 1
        elif name == "tool_started":
            counts["tool_calls"] += 1
        elif name == "turn_continued":
            counts["continuations"] += 1
        elif name == "turn_budget_exhausted":
            counts["budget_exhausted"] = True
        elif name == "wrap_up_injected":
            counts["wrap_ups"] += 1
        elif name == "final_call_forced":
            counts["final_calls"] += 1
        elif name == "permission_decided":
            decision = (row.get("payload") or {}).get("decision")
            key = f"permission_{decision}"
            if key in counts:
                counts[key] += 1
    return counts


def load_meta(case_dir: Path) -> dict:
    with (case_dir / "meta.toml").open("rb") as handle:
        return tomllib.load(handle)


def posix_path(path: Path) -> str:
    """Convert a Windows path for Git Bash; fall back to a forward-slash path."""
    try:
        return subprocess.check_output(
            ["cygpath", "-u", str(path)],
            text=True,
            encoding="utf-8",
        ).strip()
    except (OSError, subprocess.SubprocessError):
        return path.as_posix()


def run_case(case_dir: Path, profile: str | None = None, keep: bool = False) -> dict:
    case_dir = case_dir.resolve()
    meta = load_meta(case_dir)
    case_id = meta.get("case", {}).get("id", case_dir.name)
    selected_profile = profile or meta.get("case", {}).get(
        "profile", "variants/coding/profile.toml"
    )
    timeout = int(meta.get("case", {}).get("timeout_seconds", 300)) + 60

    workspace = Path(tempfile.mkdtemp(prefix=f"keel-{case_id}-"))
    # Keep eval state out of the developer's home directory.
    state_home = Path(tempfile.mkdtemp(prefix=f"keel-{case_id}-state-"))
    if (case_dir / "setup").is_dir():
        shutil.copytree(case_dir / "setup", workspace, dirs_exist_ok=True)

    command = [
        "cargo",
        "run",
        "-q",
        "-p",
        "keel-cli",
        "--",
        "--workspace",
        str(workspace),
        "--profile",
        selected_profile,
        "--input",
        (case_dir / "task.md").read_text(encoding="utf-8"),
    ]
    acceptance = meta.get("case", {}).get("acceptance")
    if acceptance:
        command.extend(["--acceptance", str(case_dir / acceptance)])
    max_calls = meta.get("case", {}).get("max_model_calls")
    if max_calls:
        command.extend(["--max-model-calls", str(max_calls)])
    tool_timeout = meta.get("case", {}).get("tool_timeout_ms")
    if tool_timeout:
        command.extend(["--tool-timeout-ms", str(tool_timeout)])
    todo_file = meta.get("case", {}).get("todo_file")
    if todo_file:
        command.extend(["--todo-file", str(case_dir / todo_file)])
    max_tokens = meta.get("case", {}).get("max_turn_tokens")
    if max_tokens:
        command.extend(["--max-turn-tokens", str(max_tokens)])
    interject = meta.get("case", {}).get("interject")
    if interject:
        command.extend(["--interject", interject])
        delay = meta.get("case", {}).get("interject_after_ms")
        if delay:
            command.extend(["--interject-after-ms", str(delay)])

    started = dt.datetime.now(dt.timezone.utc)
    timed_out = False
    env = os.environ.copy()
    env["KEEL_HOME"] = str(state_home)
    try:
        cli = subprocess.run(
            command,
            cwd=REPO_ROOT,
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        cli_exit = cli.returncode
        stdout = cli.stdout or ""
        stderr = cli.stderr or ""
    except subprocess.TimeoutExpired as error:
        timed_out = True
        cli_exit = 124
        stdout = (error.stdout or "") if isinstance(error.stdout, str) else ""
        stderr = (error.stderr or "") if isinstance(error.stderr, str) else ""

    mechanisms = collect_mechanisms(state_home, timeout)
    per_round = parse_call_tools(stdout)
    mechanisms["max_tools_per_round"] = max(per_round) if per_round else 0
    mechanisms["multi_tool_rounds"] = sum(1 for count in per_round if count > 1)

    rules = meta.get("mechanisms", {})
    failures = []
    if mechanisms.get("tool_calls", 0) < int(rules.get("min_tool_calls", 0)):
        failures.append(
            f"tool_calls {mechanisms.get('tool_calls', 0)} < {rules['min_tool_calls']}"
        )
    if mechanisms.get("model_calls", 0) < int(rules.get("min_model_calls", 0)):
        failures.append(
            f"model_calls {mechanisms.get('model_calls', 0)} < {rules['min_model_calls']}"
        )
    if rules.get("require_continuation") and mechanisms.get("continuations", 0) < 1:
        failures.append("no turn continuation")
    if rules.get("require_budget_exhaustion") and not mechanisms.get("budget_exhausted"):
        failures.append("the turn budget was not reached")
    if rules.get("forbid_budget_exhaustion") and mechanisms.get("budget_exhausted"):
        failures.append("the turn budget was exhausted")
    if mechanisms.get("permission_deny", 0) > int(rules.get("max_permission_deny", 10**9)):
        failures.append("a permission was denied")
    mechanisms["failures"] = failures

    bash_bin = shutil.which("bash") or "bash"
    verify = subprocess.run(
        [bash_bin, "verify.sh", posix_path(workspace)],
        cwd=case_dir,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )
    verify_stdout = verify.stdout or ""
    verify_stderr = verify.stderr or ""

    state = parse_field(stdout, "state")
    error = None
    for line in stderr.splitlines():
        if line.startswith("error: "):
            error = line[len("error: ") :]
    passed = (
        not timed_out
        and cli_exit == 0
        and verify.returncode == 0
        and state == "Done"
        and not mechanisms.get("failures")
    )
    report = {
        "case": case_id,
        "started_at": started.isoformat(),
        "profile": selected_profile,
        "command": command,
        "state": state,
        "error": error,
        "cli_exit": cli_exit,
        "verify_exit": verify.returncode,
        "passed": passed,
        "timed_out": timed_out,
        "events": parse_field(stdout, "events"),
        "reasoning": parse_field(stdout, "reasoning"),
        "verification": parse_field(stdout, "verification"),
        "duration_ms": parse_field(stdout, "duration_ms"),
        "usage": parse_usage(stdout),
        "mechanisms": mechanisms,
        "workspace": str(workspace) if keep else None,
        "stdout_tail": stdout[-MAX_OUTPUT:],
        "stderr_tail": stderr[-MAX_OUTPUT:],
        "verify_tail": verify_stdout[-MAX_OUTPUT:] + verify_stderr[-MAX_OUTPUT:],
    }

    REPORTS_ROOT.mkdir(parents=True, exist_ok=True)
    report_path = REPORTS_ROOT / f"{started.strftime('%Y%m%dT%H%M%SZ')}-{case_id}.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    report["report_path"] = str(report_path.relative_to(REPO_ROOT))

    if not keep:
        shutil.rmtree(workspace, ignore_errors=True)
        shutil.rmtree(state_home, ignore_errors=True)
    return report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("case", type=Path)
    parser.add_argument("--profile", help="override the profile in meta.toml")
    parser.add_argument("--keep", action="store_true", help="keep the workspace")
    args = parser.parse_args()

    report = run_case(args.case, args.profile, args.keep)
    print(f"{'PASS' if report['passed'] else 'FAIL'} {report['case']}")
    print(f"  state={report['state']} verify_exit={report['verify_exit']}")
    print(f"  usage={report['usage']} duration_ms={report['duration_ms']}")
    print(f"  mechanisms={report.get('mechanisms')}")
    print(f"  report={report['report_path']}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
