#!/usr/bin/env python3
"""Import HumanEval+ rows into the local Keel eval layout.

The dataset is never vendored. Pass a locally downloaded JSONL/JSON file, or
use `--download` when the Hugging Face datasets server is reachable.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = REPO_ROOT / "evals" / "cases" / "public" / "humanevalplus"
DATASET = "evalplus/humanevalplus"
ROW_URL = (
    "https://datasets-server.huggingface.co/rows"
    f"?dataset={DATASET.replace('/', '%2F')}&config=default&split=test"
)

VERIFY = """#!/usr/bin/env bash
set -euo pipefail
workspace="${1:?usage: verify.sh WORKSPACE}"
case_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if command -v python >/dev/null 2>&1; then
  python_bin=python
else
  python_bin=python3
fi
cd "$workspace"
PYTHONPATH="$workspace:$case_dir/hidden" "$python_bin" -m unittest discover \\
  -s "$case_dir/hidden" -p 'test_*.py' -v
"""


def load_rows(path: Path) -> list[dict]:
    raw = path.read_text(encoding="utf-8")
    if path.suffix == ".jsonl":
        return [json.loads(line) for line in raw.splitlines() if line.strip()]
    value = json.loads(raw)
    if isinstance(value, list):
        return value
    if isinstance(value, dict) and isinstance(value.get("rows"), list):
        return [row.get("row", row) for row in value["rows"]]
    raise ValueError("expected a JSON array, JSONL, or a datasets-server response")


def download_rows(limit: int) -> list[dict]:
    rows: list[dict] = []
    offset = 0
    page = 100
    while len(rows) < limit:
        length = min(page, limit - len(rows))
        url = f"{ROW_URL}&offset={offset}&length={length}"
        with urllib.request.urlopen(url, timeout=30) as response:
            payload = json.load(response)
        batch = [item.get("row", item) for item in payload.get("rows", [])]
        if not batch:
            break
        rows.extend(batch)
        offset += len(batch)
        if len(batch) < length:
            break
    return rows


def entry_point(row: dict) -> str:
    if row.get("entry_point"):
        return row["entry_point"]
    prompt = row["prompt"]
    for line in prompt.splitlines():
        stripped = line.strip()
        if stripped.startswith("def "):
            return stripped[4:].split("(", 1)[0].strip()
    raise ValueError(f"cannot determine entry point for {row.get('task_id')}")


def write_case(row: dict, output: Path) -> Path:
    task_id = str(row["task_id"]).replace("/", "-")
    case = output / task_id
    (case / "setup").mkdir(parents=True, exist_ok=True)
    (case / "reference").mkdir(parents=True, exist_ok=True)
    (case / "hidden").mkdir(parents=True, exist_ok=True)
    entry = entry_point(row)

    prompt = row["prompt"]
    (case / "setup" / "solution.py").write_text(
        prompt + ("" if prompt.endswith("\n") else "\n") + "    pass\n",
        encoding="utf-8",
    )
    (case / "reference" / "solution.py").write_text(
        prompt + row["canonical_solution"],
        encoding="utf-8",
    )
    (case / "hidden" / "humaneval_test.py").write_text(row["test"], encoding="utf-8")
    (case / "hidden" / "test_runner.py").write_text(
        "\n".join(
            [
                "import unittest",
                "",
                "import solution",
                "from humaneval_test import check",
                "",
                "",
                "class HumanEvalCase(unittest.TestCase):",
                "    def test_case(self):",
                f"        check(solution.{entry})",
                "",
                "",
                'if __name__ == "__main__":',
                "    unittest.main()",
                "",
            ]
        ),
        encoding="utf-8",
    )
    (case / "verify.sh").write_text(VERIFY, encoding="utf-8")
    (case / "task.md").write_text(
        f"# {row['task_id']}\n\nComplete the function in `solution.py`.\n\n"
        "Do not change the tests.\n\n```python\n"
        f"{prompt}\n```\n",
        encoding="utf-8",
    )
    (case / "meta.toml").write_text(
        "\n".join(
            [
                "[case]",
                f'id = "{task_id}"',
                'kind = "public"',
                'source = "evalplus/humanevalplus"',
                f'source_id = "{row["task_id"]}"',
                "timeout_seconds = 300",
                'profile = "variants/coding/profile.eval.toml"',
                'model = "deepseek-flash"',
                "seeds = [1]",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return case


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, help="local JSON/JSONL dataset file")
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--limit", type=int, default=20)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    if args.input:
        rows = load_rows(args.input)
    elif args.download:
        rows = download_rows(args.limit)
    else:
        print("pass --input FILE or --download", file=sys.stderr)
        return 2

    if not rows:
        print("no rows found", file=sys.stderr)
        return 1

    for row in rows[: args.limit]:
        case = write_case(row, args.output)
        print(f"imported {case.relative_to(REPO_ROOT) if case.is_relative_to(REPO_ROOT) else case}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
