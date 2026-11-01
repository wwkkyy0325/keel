#!/usr/bin/env python3
"""Run one self-contained eval case and write a JSON report."""

from __future__ import annotations

import argparse
import datetime as dt
import json
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

    started = dt.datetime.now(dt.timezone.utc)
    timed_out = False
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
        cli_exit = cli.returncode
        stdout = cli.stdout or ""
        stderr = cli.stderr or ""
    except subprocess.TimeoutExpired as error:
        timed_out = True
        cli_exit = 124
        stdout = (error.stdout or "") if isinstance(error.stdout, str) else ""
        stderr = (error.stderr or "") if isinstance(error.stderr, str) else ""

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
    print(f"  report={report['report_path']}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
