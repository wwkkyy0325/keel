#!/usr/bin/env python3
"""Run every eval case and summarize the results."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from run_case import REPORTS_ROOT, REPO_ROOT, run_case  # noqa: E402


def discover(filter_text: str | None) -> list[Path]:
    cases = sorted((REPO_ROOT / "evals" / "cases").rglob("meta.toml"))
    selected = []
    for meta in cases:
        case_dir = meta.parent
        relative = case_dir.relative_to(REPO_ROOT).as_posix()
        if filter_text and filter_text not in relative:
            continue
        selected.append(case_dir)
    return selected


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--filter", help="only run paths containing this text")
    parser.add_argument("--profile", help="override every case profile")
    parser.add_argument("--keep", action="store_true")
    parser.add_argument(
        "--max-total-tokens",
        type=int,
        default=300_000,
        help="stop before the next case when cumulative usage reaches this; 0 disables",
    )
    args = parser.parse_args()

    cases = discover(args.filter)
    if not cases:
        print("no cases found")
        return 1

    results = []
    skipped = []
    total_tokens = 0
    for case_dir in cases:
        if args.max_total_tokens and total_tokens >= args.max_total_tokens:
            skipped.append(str(case_dir.relative_to(REPO_ROOT)))
            print(f"SKIP {case_dir.relative_to(REPO_ROOT)} (token budget)")
            continue
        try:
            result = run_case(case_dir, args.profile, args.keep)
        except Exception as error:  # noqa: BLE001 - report harness failure
            result = {
                "case": str(case_dir.relative_to(REPO_ROOT)),
                "passed": False,
                "error": str(error),
            }
        results.append(result)
        usage = result.get("usage") or {}
        total_tokens += int(usage.get("total_tokens") or 0)
        print(f"{'PASS' if result.get('passed') else 'FAIL'} {result['case']}")

    passed = sum(1 for result in results if result.get("passed"))
    summary = {
        "started_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "cases": len(results),
        "passed": passed,
        "failed": len(results) - passed,
        "skipped": skipped,
        "total_tokens": total_tokens,
        "max_total_tokens": args.max_total_tokens,
        "results": results,
    }
    REPORTS_ROOT.mkdir(parents=True, exist_ok=True)
    path = REPORTS_ROOT / f"summary-{dt.datetime.now().strftime('%Y%m%dT%H%M%SZ')}.json"
    path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"summary: {passed}/{len(results)} passed, {total_tokens} tokens")
    if skipped:
        print(f"skipped {len(skipped)} case(s) after the token budget")
    print(f"report: {path.relative_to(REPO_ROOT)}")
    return 0 if passed == len(results) and not skipped else 1


if __name__ == "__main__":
    sys.exit(main())
