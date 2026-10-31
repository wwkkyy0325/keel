#!/usr/bin/env python3
"""Import HumanEval+ or MBPP+ rows into the local Keel eval layout.

The dataset is never vendored. Pass a locally downloaded JSONL/JSON file, or
use `--download` when the Hugging Face datasets server is reachable.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DATASETS = {
    "humaneval": "evalplus/humanevalplus",
    "mbpp": "evalplus/mbppplus",
}
DEFAULT_OUTPUTS = {
    "humaneval": REPO_ROOT / "evals" / "cases" / "public" / "humanevalplus",
    "mbpp": REPO_ROOT / "evals" / "cases" / "public" / "mbppplus",
}

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


def download_rows(dataset: str, limit: int) -> list[dict]:
    rows: list[dict] = []
    offset = 0
    page = 100
    row_url = (
        "https://datasets-server.huggingface.co/rows"
        f"?dataset={DATASETS[dataset].replace('/', '%2F')}"
        "&config=default&split=test"
    )
    while len(rows) < limit:
        length = min(page, limit - len(rows))
        url = f"{row_url}&offset={offset}&length={length}"
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


def function_defs(text: str) -> list[str]:
    return re.findall(r"^def\s+([A-Za-z_]\w*)\s*\(", text, re.M)


def entry_point(row: dict) -> str:
    if row.get("entry_point"):
        return row["entry_point"]
    canonical = row.get("canonical_solution") or row.get("code") or ""
    defs = function_defs(canonical) or function_defs(row.get("prompt", ""))
    if not defs:
        raise ValueError(f"cannot determine entry point for {row.get('task_id')}")
    test = row.get("test", "") + "\n" + "\n".join(row.get("test_list") or [])
    for name in reversed(defs):
        if re.search(rf"\b{re.escape(name)}\s*\(", test):
            return name
    return defs[-1]


def signature_for(text: str, entry: str) -> str | None:
    match = re.search(
        rf"^def\s+{re.escape(entry)}\s*\([^)]*\)\s*:",
        text,
        re.M | re.S,
    )
    return match.group(0) if match else None


def write_case(row: dict, output: Path, dataset: str) -> Path:
    task_id = str(row["task_id"]).replace("/", "-")
    case = output / task_id
    (case / "setup").mkdir(parents=True, exist_ok=True)
    (case / "reference").mkdir(parents=True, exist_ok=True)
    (case / "hidden").mkdir(parents=True, exist_ok=True)
    entry = entry_point(row)

    prompt = row.get("prompt", "")
    canonical = row.get("canonical_solution", row.get("code", ""))
    if "def " in prompt:
        setup = prompt + ("" if prompt.endswith("\n") else "\n") + "    pass\n"
        reference = prompt + canonical if not canonical.lstrip().startswith("def ") else canonical
    else:
        reference = canonical
        if reference and not reference.endswith("\n"):
            reference += "\n"
        signature = signature_for(reference, entry)
        if signature:
            setup = f"# {prompt}\n{signature}\n    pass\n"
        else:
            setup = f"# {prompt}\n# Implement `{entry}` in this file.\n"
    (case / "setup" / "solution.py").write_text(setup, encoding="utf-8")
    (case / "reference" / "solution.py").write_text(reference, encoding="utf-8")

    test_code = row.get("test", "")
    imports = row.get("test_imports") or []
    if "def check(" in test_code:
        (case / "hidden" / "humaneval_test.py").write_text(test_code, encoding="utf-8")
        runner = [
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
    else:
        lines = list(imports)
        lines.append(f"from solution import {entry}")
        lines.append(test_code.strip() or f"assert callable({entry})")
        (case / "hidden" / "mbpp_test.py").write_text(
            "\n".join(lines) + "\n", encoding="utf-8"
        )
        runner = [
            "import unittest",
            "",
            "",
            "class MbppCase(unittest.TestCase):",
            "    def test_case(self):",
            "        import mbpp_test  # noqa: F401",
            "",
            "",
            'if __name__ == "__main__":',
            "    unittest.main()",
            "",
        ]
    (case / "hidden" / "test_runner.py").write_text(
        "\n".join(runner), encoding="utf-8"
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
                f'source = "{DATASETS[dataset]}"',
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
    parser.add_argument("--dataset", choices=sorted(DATASETS), default="humaneval")
    parser.add_argument("--input", type=Path, help="local JSON/JSONL dataset file")
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--limit", type=int, default=20)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    output = args.output or DEFAULT_OUTPUTS[args.dataset]

    if args.input:
        rows = load_rows(args.input)
    elif args.download:
        rows = download_rows(args.dataset, args.limit)
    else:
        print("pass --input FILE or --download", file=sys.stderr)
        return 2

    if not rows:
        print("no rows found", file=sys.stderr)
        return 1

    for row in rows[: args.limit]:
        case = write_case(row, output, args.dataset)
        print(
            f"imported {case.relative_to(REPO_ROOT) if case.is_relative_to(REPO_ROOT) else case}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
