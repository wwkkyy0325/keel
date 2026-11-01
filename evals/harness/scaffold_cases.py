#!/usr/bin/env python3
"""Scaffold the self-authored hardening cases.

The generated files are committed; this script is a maintenance helper for
adding cases with the same layout.
"""

from __future__ import annotations

import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CASES_ROOT = REPO_ROOT / "evals" / "cases" / "coding"

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

CASES = [
    {
        "id": "task-002-json-merge",
        "title": "Merge nested configuration without mutating the base",
        "description": (
            "`config.merge(base, override)` currently mutates `base` and only "
            "shallow-merges nested dictionaries. Return a new merged mapping, "
            "keep nested keys that are not overridden, and do not mutate either "
            "input."
        ),
        "setup": {
            "config.py": """def merge(base, override):
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(base.get(key), dict):
            base[key].update(value)
        else:
            base[key] = value
    return base
"""
        },
        "reference": {
            "config.py": """import copy


def merge(base, override):
    result = copy.deepcopy(base)
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            result[key] = merge(result[key], value)
        else:
            result[key] = copy.deepcopy(value)
    return result
"""
        },
        "tests": """import unittest

from config import merge


class MergeTests(unittest.TestCase):
    def test_does_not_mutate_base(self):
        base = {"a": {"x": 1, "y": 2}}
        merged = merge(base, {"a": {"y": 3}})
        self.assertEqual(base, {"a": {"x": 1, "y": 2}})
        self.assertEqual(merged, {"a": {"x": 1, "y": 3}})

    def test_adds_new_keys(self):
        self.assertEqual(merge({"a": 1}, {"b": 2}), {"a": 1, "b": 2})

    def test_override_scalar(self):
        self.assertEqual(merge({"a": {"x": 1}}, {"a": 3}), {"a": 3})


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from config import merge; "
            "base={'a': {'x': 1, 'y': 2}}; "
            "assert merge(base, {'a': {'y': 3}}) == {'a': {'x': 1, 'y': 3}}; "
            "assert base == {'a': {'x': 1, 'y': 2}}"
        ),
    },
    {
        "id": "task-003-pagination",
        "title": "Fix one-based pagination",
        "description": (
            "`pagination.page(items, number, size)` uses one-based page numbers. "
            "It currently starts page 1 at index `size`. Fix the offset and "
            "reject page or size values below 1 with `ValueError`."
        ),
        "setup": {
            "pagination.py": """def page(items, number, size):
    start = number * size
    return items[start:start + size]
"""
        },
        "reference": {
            "pagination.py": """def page(items, number, size):
    if number < 1 or size < 1:
        raise ValueError("page and size must be positive")
    start = (number - 1) * size
    return items[start:start + size]
"""
        },
        "tests": """import unittest

from pagination import page


class PaginationTests(unittest.TestCase):
    def test_first_page(self):
        self.assertEqual(page([1, 2, 3], 1, 2), [1, 2])

    def test_second_page(self):
        self.assertEqual(page([1, 2, 3], 2, 2), [3])

    def test_past_end_is_empty(self):
        self.assertEqual(page([1, 2, 3], 5, 2), [])

    def test_invalid_values_raise(self):
        with self.assertRaises(ValueError):
            page([1], 0, 1)
        with self.assertRaises(ValueError):
            page([1], 1, 0)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from pagination import page; "
            "assert page([1, 2, 3], 1, 2) == [1, 2]; "
            "assert page([1, 2, 3], 2, 2) == [3]"
        ),
    },
    {
        "id": "task-004-cli-args",
        "title": "Validate CLI arguments",
        "description": (
            "`cli.parse_args(argv)` accepts `--name value` pairs. It currently "
            "crashes on a missing value and silently accepts positional "
            "arguments. Raise `ValueError` for missing values, unknown "
            "positional arguments, and flags without a value. Convert hyphens "
            "in option names to underscores."
        ),
        "setup": {
            "cli.py": """def parse_args(argv):
    result = {}
    index = 1
    while index < len(argv):
        key = argv[index].lstrip("-")
        result[key] = argv[index + 1]
        index += 2
    return result
"""
        },
        "reference": {
            "cli.py": """def parse_args(argv):
    result = {}
    index = 1
    while index < len(argv):
        arg = argv[index]
        if not arg.startswith("--") or arg == "--":
            raise ValueError(f"unexpected argument: {arg}")
        key = arg[2:].replace("-", "_")
        if index + 1 >= len(argv) or argv[index + 1].startswith("--"):
            raise ValueError(f"missing value for {arg}")
        result[key] = argv[index + 1]
        index += 2
    return result
"""
        },
        "tests": """import unittest

from cli import parse_args


class CliTests(unittest.TestCase):
    def test_parses_pairs(self):
        self.assertEqual(
            parse_args(["prog", "--name", "keel", "--count", "2"]),
            {"name": "keel", "count": "2"},
        )

    def test_hyphen_becomes_underscore(self):
        self.assertEqual(parse_args(["prog", "--dry-run", "yes"]), {"dry_run": "yes"})

    def test_missing_value(self):
        with self.assertRaises(ValueError):
            parse_args(["prog", "--name"])

    def test_positional_is_rejected(self):
        with self.assertRaises(ValueError):
            parse_args(["prog", "oops"])


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from cli import parse_args; "
            "assert parse_args(['p', '--name', 'keel']) == {'name': 'keel'}; "
            "assert parse_args(['p', '--dry-run', 'yes']) == {'dry_run': 'yes'}"
        ),
    },
    {
        "id": "task-005-retry",
        "title": "Retry only the intended exceptions",
        "description": (
            "`retry.retry(operation, attempts)` retries every exception and "
            "loses the last error. Retry only `ValueError` by default, return "
            "the first success, re-raise the last matching error after the "
            "final attempt, and reject `attempts < 1` with `ValueError`."
        ),
        "setup": {
            "retry.py": """def retry(operation, attempts):
    for _ in range(attempts):
        try:
            return operation()
        except Exception:
            pass
    raise RuntimeError("operation failed")
"""
        },
        "reference": {
            "retry.py": """def retry(operation, attempts, retry_on=(ValueError,)):
    if attempts < 1:
        raise ValueError("attempts must be positive")
    for attempt in range(attempts):
        try:
            return operation()
        except retry_on:
            if attempt == attempts - 1:
                raise
    raise AssertionError("unreachable")
"""
        },
        "tests": """import unittest

from retry import retry


class RetryTests(unittest.TestCase):
    def test_returns_first_success(self):
        self.assertEqual(retry(lambda: 7, 3), 7)

    def test_retries_value_error(self):
        calls = {"count": 0}

        def operation():
            calls["count"] += 1
            if calls["count"] < 3:
                raise ValueError("not yet")
            return "ok"

        self.assertEqual(retry(operation, 3), "ok")
        self.assertEqual(calls["count"], 3)

    def test_non_retryable_error_propagates(self):
        with self.assertRaises(RuntimeError):
            retry(lambda: (_ for _ in ()).throw(RuntimeError("boom")), 3)

    def test_exhaustion_reraises_last_error(self):
        with self.assertRaises(ValueError):
            retry(lambda: (_ for _ in ()).throw(ValueError("nope")), 2)

    def test_zero_attempts_is_rejected(self):
        with self.assertRaises(ValueError):
            retry(lambda: 1, 0)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from retry import retry; "
            "assert retry(lambda: 7, 3) == 7"
        ),
    },
    {
        "id": "task-006-ranking",
        "title": "Deterministic leaderboard ordering",
        "description": (
            "`ranking.rank(players)` sorts players by score descending but "
            "leaves ties in input order. Sort by score descending, then by name "
            "ascending, without mutating the input list."
        ),
        "setup": {
            "ranking.py": """def rank(players):
    return sorted(players, key=lambda player: player["score"], reverse=True)
"""
        },
        "reference": {
            "ranking.py": """def rank(players):
    return sorted(players, key=lambda player: (-player["score"], player["name"]))
"""
        },
        "tests": """import unittest

from ranking import rank


class RankingTests(unittest.TestCase):
    def test_score_then_name(self):
        players = [
            {"name": "zoe", "score": 10},
            {"name": "amy", "score": 10},
            {"name": "bob", "score": 12},
        ]
        self.assertEqual(
            rank(players),
            [
                {"name": "bob", "score": 12},
                {"name": "amy", "score": 10},
                {"name": "zoe", "score": 10},
            ],
        )

    def test_input_not_mutated(self):
        players = [{"name": "zoe", "score": 1}, {"name": "amy", "score": 2}]
        rank(players)
        self.assertEqual(players[0]["name"], "zoe")

    def test_empty(self):
        self.assertEqual(rank([]), [])


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from ranking import rank; "
            "assert rank([{'name': 'zoe', 'score': 10}, {'name': 'amy', 'score': 10}])"
            " == [{'name': 'amy', 'score': 10}, {'name': 'zoe', 'score': 10}]"
        ),
    },
    {
        "id": "task-007-storage",
        "title": "Safe JSON storage",
        "description": (
            "`store.load(path)` raises when the file is missing, and "
            "`store.save(path, value)` fails when the parent directory does not "
            "exist. Make `load` return the supplied default (or `{}`), make "
            "`save` create parent directories, write UTF-8, and keep keys "
            "deterministic."
        ),
        "setup": {
            "store.py": """import json


def load(path):
    with open(path) as handle:
        return json.load(handle)


def save(path, value):
    with open(path, "w") as handle:
        json.dump(value, handle)
"""
        },
        "reference": {
            "store.py": """import json
import os


def load(path, default=None):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except FileNotFoundError:
        return {} if default is None else default


def save(path, value):
    parent = os.path.dirname(path)
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        json.dump(value, handle, sort_keys=True)
"""
        },
        "tests": """import os
import tempfile
import unittest

from store import load, save


class StorageTests(unittest.TestCase):
    def test_missing_file_returns_default(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(load(os.path.join(directory, "missing.json")), {})
            self.assertEqual(
                load(os.path.join(directory, "missing.json"), {"x": 1}), {"x": 1}
            )

    def test_save_creates_parent_and_round_trips(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "nested", "data.json")
            save(path, {"b": 2, "a": 1})
            self.assertEqual(load(path), {"a": 1, "b": 2})
            with open(path, encoding="utf-8") as handle:
                self.assertTrue(handle.read().startswith('{"a"'))


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "import tempfile, os; from store import save, load; "
            "d=tempfile.mkdtemp(); p=os.path.join(d, 'n', 'x.json'); "
            "save(p, {'b': 2, 'a': 1}); assert load(p) == {'a': 1, 'b': 2}"
        ),
    },
    {
        "id": "task-008-greeting",
        "title": "Fix greeting across two modules",
        "description": (
            "The `app` package formats names in `app/format.py` and builds a "
            "greeting in `app/main.py`. The greeting currently swaps the names "
            "and leaves a trailing space when one part is empty. Fix both "
            "modules so the full name is ordered correctly. `greeting` must "
            "return `Hello, <first> <last>!`, or `Hello!` when the name is "
            "empty. Treat `None` and blank parts as absent."
        ),
        "setup": {
            "app/__init__.py": "",
            "app/format.py": """def format_name(first, last):
    return f"{first} {last}".strip()
""",
            "app/main.py": """from app.format import format_name


def greeting(first, last):
    return "Hello, " + format_name(last, first)
""",
        },
        "reference": {
            "app/__init__.py": "",
            "app/format.py": """def format_name(first, last):
    parts = [part.strip() for part in (first, last) if part and part.strip()]
    return " ".join(parts)
""",
            "app/main.py": """from app.format import format_name


def greeting(first, last):
    name = format_name(first, last)
    return f"Hello, {name}!" if name else "Hello!"
""",
        },
        "tests": """import unittest

from app.format import format_name
from app.main import greeting


class GreetingTests(unittest.TestCase):
    def test_name_order(self):
        self.assertEqual(format_name("Ada", "Lovelace"), "Ada Lovelace")

    def test_missing_part(self):
        self.assertEqual(format_name("Ada", "  "), "Ada")
        self.assertEqual(format_name(None, "Lovelace"), "Lovelace")

    def test_greeting(self):
        self.assertEqual(greeting("Ada", "Lovelace"), "Hello, Ada Lovelace!")

    def test_empty_greeting(self):
        self.assertEqual(greeting("", " "), "Hello!")


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from app.main import greeting; "
            "assert greeting('Ada', 'Lovelace') == 'Hello, Ada Lovelace!'; "
            "assert greeting('', '') == 'Hello!'"
        ),
    },
    {
        "id": "task-009-unique",
        "title": "Stable de-duplication",
        "description": (
            "`unique.unique(items)` returns a list from a set, so the original "
            "order is lost. Return the first occurrence of each item in input "
            "order. Do not mutate the input."
        ),
        "setup": {
            "unique.py": """def unique(items):
    return sorted(set(items))
"""
        },
        "reference": {
            "unique.py": """def unique(items):
    seen = set()
    result = []
    for item in items:
        if item not in seen:
            seen.add(item)
            result.append(item)
    return result
"""
        },
        "tests": """import unittest

from unique import unique


class UniqueTests(unittest.TestCase):
    def test_preserves_first_occurrence_order(self):
        self.assertEqual(unique(["b", "a", "b", "c", "a"]), ["b", "a", "c"])

    def test_empty(self):
        self.assertEqual(unique([]), [])

    def test_input_not_mutated(self):
        items = ["b", "a", "b"]
        unique(items)
        self.assertEqual(items, ["b", "a", "b"])


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": "from unique import unique; assert unique(['b', 'a', 'b']) == ['b', 'a']",
    },
    {
        "id": "task-010-report",
        "title": "Robust record totals",
        "description": (
            "`report.total(path)` reads JSON Lines records and sums `amount`. "
            "It currently crashes on a blank or malformed line and accepts "
            "negative amounts. Skip blank and malformed lines, sum only "
            "positive numeric amounts, and ignore missing or non-numeric "
            "amounts."
        ),
        "setup": {
            "report.py": """import json


def total(path):
    total = 0
    with open(path) as handle:
        for line in handle:
            total += json.loads(line)["amount"]
    return total
""",
            "data/records.jsonl": "",
        },
        "reference": {
            "report.py": """import json


def total(path):
    total = 0
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            amount = record.get("amount")
            if isinstance(amount, (int, float)) and amount > 0:
                total += amount
    return total
"""
        },
        "tests": """import json
import os
import tempfile
import unittest

from report import total


class ReportTests(unittest.TestCase):
    def _write(self, lines):
        handle, path = tempfile.mkstemp(suffix=".jsonl")
        os.close(handle)
        with open(path, "w", encoding="utf-8") as stream:
            for line in lines:
                stream.write(line + "\\n")
        return path

    def test_skips_bad_lines_and_negative(self):
        path = self._write(
            [
                json.dumps({"amount": 3}),
                "",
                "not json",
                json.dumps({"amount": -5}),
                json.dumps({"amount": "4"}),
                json.dumps({"amount": 2}),
                json.dumps({"other": 9}),
            ]
        )
        try:
            self.assertEqual(total(path), 5)
        finally:
            os.unlink(path)

    def test_empty_file(self):
        path = self._write([])
        try:
            self.assertEqual(total(path), 0)
        finally:
            os.unlink(path)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "import json, tempfile, os, pathlib; from report import total; "
            "fd, p = tempfile.mkstemp(); os.close(fd); "
            "pathlib.Path(p).write_text("
            "json.dumps({'amount': 2}) + '\\n' + 'bad' + '\\n', encoding='utf-8'); "
            "assert total(p) == 2; os.unlink(p)"
        ),
    },
    {
        "id": "task-011-csv",
        "title": "Parse quoted CSV fields",
        "description": (
            "`csvlite.parse(text)` splits every line on commas, which breaks "
            "quoted fields. Parse the text into rows with quoted fields, "
            "doubled quotes inside a quoted field, and commas or newlines "
            "inside quotes. A trailing newline does not add a row."
        ),
        "setup": {
            "csvlite.py": """def parse(text):
    rows = []
    for line in text.splitlines():
        if line.strip():
            rows.append(line.split(","))
    return rows
"""
        },
        "reference": {
            "csvlite.py": """def parse(text):
    rows = []
    row = []
    field = []
    quoted = False
    index = 0
    while index < len(text):
        char = text[index]
        if quoted:
            if char == '"':
                if index + 1 < len(text) and text[index + 1] == '"':
                    field.append('"')
                    index += 1
                else:
                    quoted = False
            else:
                field.append(char)
        elif char == '"':
            quoted = True
        elif char == ",":
            row.append("".join(field))
            field = []
        elif char in "\\r\\n":
            if char == "\\r" and index + 1 < len(text) and text[index + 1] == "\\n":
                index += 1
            row.append("".join(field))
            field = []
            rows.append(row)
            row = []
        else:
            field.append(char)
        index += 1
    if field or row:
        row.append("".join(field))
        rows.append(row)
    return rows
"""
        },
        "tests": """import unittest

from csvlite import parse


class ParseTests(unittest.TestCase):
    def test_simple_rows(self):
        self.assertEqual(parse("a,b\\nc,d\\n"), [["a", "b"], ["c", "d"]])

    def test_quoted_comma(self):
        self.assertEqual(parse('a,"b,c"\\n'), [["a", "b,c"]])

    def test_doubled_quote(self):
        self.assertEqual(parse('"a""b",c\\n'), [['a"b', "c"]])

    def test_newline_inside_quotes(self):
        self.assertEqual(parse('"line1\\nline2",x\\n'), [["line1\\nline2", "x"]])

    def test_no_trailing_newline(self):
        self.assertEqual(parse("a,b"), [["a", "b"]])


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from csvlite import parse; "
            "assert parse('a,\"b,c\"\\n') == [['a', 'b,c']]"
        ),
    },
    {
        "id": "task-012-lru",
        "title": "Make the LRU cache evict the least recent key",
        "description": (
            "`cache.LRUCache` stores values but never evicts and `get` does "
            "not refresh recency. Keep at most `capacity` keys, evict the "
            "least recently used (a read counts as use), and reject a "
            "capacity that is not a positive integer."
        ),
        "setup": {
            "cache.py": """class LRUCache:
    def __init__(self, capacity):
        self.capacity = capacity
        self.data = {}

    def get(self, key):
        return self.data.get(key)

    def put(self, key, value):
        self.data[key] = value
        while len(self.data) > self.capacity:
            self.data.pop(next(iter(self.data)))
"""
        },
        "reference": {
            "cache.py": """from collections import OrderedDict


class LRUCache:
    def __init__(self, capacity):
        if not isinstance(capacity, int) or isinstance(capacity, bool) or capacity <= 0:
            raise ValueError("capacity must be positive")
        self.capacity = capacity
        self.data = OrderedDict()

    def get(self, key):
        if key not in self.data:
            return None
        self.data.move_to_end(key)
        return self.data[key]

    def put(self, key, value):
        if key in self.data:
            self.data.move_to_end(key)
        self.data[key] = value
        while len(self.data) > self.capacity:
            self.data.popitem(last=False)
"""
        },
        "tests": """import unittest

from cache import LRUCache


class CacheTests(unittest.TestCase):
    def test_evicts_least_recent(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("b", 2)
        cache.put("c", 3)
        self.assertIsNone(cache.get("a"))
        self.assertEqual(cache.get("b"), 2)
        self.assertEqual(cache.get("c"), 3)

    def test_get_refreshes_recency(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("b", 2)
        self.assertEqual(cache.get("a"), 1)
        cache.put("c", 3)
        self.assertIsNone(cache.get("b"))
        self.assertEqual(cache.get("a"), 1)

    def test_update_keeps_one_entry(self):
        cache = LRUCache(2)
        cache.put("a", 1)
        cache.put("a", 2)
        self.assertEqual(cache.get("a"), 2)
        self.assertEqual(len(cache.data), 1)

    def test_invalid_capacity(self):
        for value in (0, -1, 1.5, True):
            with self.assertRaises(ValueError):
                LRUCache(value)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from cache import LRUCache; c = LRUCache(2); c.put('a', 1); "
            "c.put('b', 2); c.put('c', 3); assert c.get('a') is None and c.get('c') == 3"
        ),
    },
    {
        "id": "task-013-toc",
        "title": "Build a table of contents outside code fences",
        "description": (
            "`docs.toc(markdown)` treats every line starting with `#` as a "
            "heading, including lines inside fenced code blocks and tags such "
            "as `#hashtag`. Return `(level, title)` for headings of level 1-3 "
            "that have a space after the markers; a heading whose title is "
            "empty after the markers is ignored. Skip fenced blocks."
        ),
        "setup": {
            "docs.py": """def toc(markdown):
    entries = []
    for line in markdown.splitlines():
        if line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            entries.append((level, line[level:].strip()))
    return entries
"""
        },
        "reference": {
            "docs.py": """def toc(markdown):
    entries = []
    fenced = False
    for line in markdown.splitlines():
        if line.strip().startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        level = 0
        while level < len(line) and line[level] == "#":
            level += 1
        if 1 <= level <= 3 and level < len(line) and line[level] == " ":
            title = line[level:].strip()
            if title:
                entries.append((level, title))
    return entries
"""
        },
        "tests": """import unittest

from docs import toc


class TocTests(unittest.TestCase):
    def test_levels(self):
        text = "# Title\\n## Part\\ntext\\n### Detail\\n"
        self.assertEqual(toc(text), [(1, "Title"), (2, "Part"), (3, "Detail")])

    def test_code_fence_is_skipped(self):
        text = "# Title\\n```\\n# not a heading\\n```\\n## Real\\n"
        self.assertEqual(toc(text), [(1, "Title"), (2, "Real")])

    def test_hashtag_is_not_a_heading(self):
        self.assertEqual(toc("#hashtag\\n# Real\\n"), [(1, "Real")])

    def test_level_four_is_ignored(self):
        self.assertEqual(toc("#### Deep\\n## Shallow\\n"), [(2, "Shallow")])

    def test_empty_title_is_ignored(self):
        self.assertEqual(toc("# \\n# Real\\n"), [(1, "Real")])


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from docs import toc; "
            "assert toc('# Title\\n```\\n# skip\\n```\\n## Part\\n') == [(1, 'Title'), (2, 'Part')]"
        ),
    },
    {
        "id": "task-014-semver",
        "title": "Compare dotted versions numerically",
        "description": (
            "`versions.compare(left, right)` compares the strings themselves, "
            "so `1.2.10` sorts before `1.2.9`. Compare the first three "
            "numeric components of each version numerically, treat missing "
            "components as zero, and ignore non-numeric suffixes."
        ),
        "setup": {
            "versions.py": """def compare(left, right):
    return (left > right) - (left < right)
"""
        },
        "reference": {
            "versions.py": """def compare(left, right):
    def parts(version):
        values = []
        for piece in version.split("."):
            digits = "".join(character for character in piece if character.isdigit())
            values.append(int(digits) if digits else 0)
        while len(values) < 3:
            values.append(0)
        return values[:3]

    left_parts = parts(left)
    right_parts = parts(right)
    return (left_parts > right_parts) - (left_parts < right_parts)
"""
        },
        "tests": """import unittest

from versions import compare


class CompareTests(unittest.TestCase):
    def test_numeric_not_lexical(self):
        self.assertEqual(compare("1.2.10", "1.2.9"), 1)
        self.assertEqual(compare("1.9.9", "1.10.0"), -1)

    def test_equal(self):
        self.assertEqual(compare("1.2.3", "1.2.3"), 0)

    def test_missing_component_is_zero(self):
        self.assertEqual(compare("1.2", "1.2.0"), 0)

    def test_suffix_ignored(self):
        self.assertEqual(compare("2.0.0-beta", "2.0.0"), 0)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": "from versions import compare; assert compare('1.2.10', '1.2.9') == 1",
    },
    {
        "id": "task-015-wrap",
        "title": "Wrap text on word boundaries",
        "description": (
            "`textwrap_lite.wrap(text, width)` cuts every line at a fixed "
            "width, splitting words. Wrap at spaces instead, keep words "
            "longer than the width intact, preserve explicit newlines, and "
            "raise `ValueError` for a non-positive width."
        ),
        "setup": {
            "textwrap_lite.py": """def wrap(text, width):
    if width <= 0:
        raise ValueError("width must be positive")
    return "\\n".join(
        text[index : index + width] for index in range(0, len(text), width)
    )
"""
        },
        "reference": {
            "textwrap_lite.py": """def wrap(text, width):
    if width <= 0:
        raise ValueError("width must be positive")
    lines = []
    for paragraph in text.split("\\n"):
        words = paragraph.split()
        if not words:
            lines.append("")
            continue
        current = words[0]
        for word in words[1:]:
            if len(current) + 1 + len(word) <= width:
                current += " " + word
            else:
                lines.append(current)
                current = word
        lines.append(current)
    return "\\n".join(lines)
"""
        },
        "tests": """import unittest

from textwrap_lite import wrap


class WrapTests(unittest.TestCase):
    def test_wraps_on_spaces(self):
        self.assertEqual(wrap("one two three", 7), "one two\\nthree")

    def test_long_word_stays_intact(self):
        self.assertEqual(wrap("abcdefghij", 3), "abcdefghij")

    def test_explicit_newlines_are_kept(self):
        self.assertEqual(wrap("a\\nb", 10), "a\\nb")

    def test_fits_on_one_line(self):
        self.assertEqual(wrap("a b c", 10), "a b c")

    def test_invalid_width(self):
        with self.assertRaises(ValueError):
            wrap("a", 0)


if __name__ == "__main__":
    unittest.main()
""",
        "acceptance": (
            "from textwrap_lite import wrap; "
            "assert wrap('one two three', 7) == 'one two\\nthree'"
        ),
    },
]


def write_case(spec: dict) -> None:
    case_dir = CASES_ROOT / spec["id"]
    (case_dir / "setup").mkdir(parents=True, exist_ok=True)
    (case_dir / "hidden").mkdir(parents=True, exist_ok=True)
    (case_dir / "reference").mkdir(parents=True, exist_ok=True)

    (case_dir / "task.md").write_text(
        f"# {spec['title']}\n\n{spec['description']}\n\n"
        "Do not change the tests. Run the test suite to confirm the fix.\n",
        encoding="utf-8",
    )
    for name, content in spec["setup"].items():
        path = case_dir / "setup" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    for name, content in spec["reference"].items():
        path = case_dir / "reference" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    (case_dir / "hidden" / "test_case.py").write_text(spec["tests"], encoding="utf-8")
    (case_dir / "verify.sh").write_text(VERIFY, encoding="utf-8")
    # Keep the data file small in git; it exists only to create a larger
    # workspace surface for search and context-pressure cases.
    data = case_dir / "setup" / "data" / "records.jsonl"
    if data.exists():
        data.write_text(
            "\n".join(
                json.dumps({"amount": (index % 7) + 1, "note": f"row {index}"})
                for index in range(1200)
            )
            + "\n",
            encoding="utf-8",
        )

    (case_dir / "meta.toml").write_text(
        "\n".join(
            [
                "[case]",
                f'id = "{spec["id"]}"',
                'kind = "coding"',
                "timeout_seconds = 300",
                'profile = "variants/coding/profile.eval.toml"',
                'model = "deepseek-flash"',
                'acceptance = "acceptance.toml"',
                "seeds = [1]",
                "",
            ]
        ),
        encoding="utf-8",
    )
    (case_dir / "acceptance.toml").write_text(
        "\n".join(
            [
                "[[items]]",
                f'id = "{spec["id"]}-acceptance"',
                f'description = {json.dumps(spec["title"])}',
                "required = true",
                'kind = "command"',
                'program = "python"',
                f"args = [\"-c\", {json.dumps(spec['acceptance'])}]",
                "expected_exit = 0",
                "",
            ]
        ),
        encoding="utf-8",
    )


def main() -> None:
    for spec in CASES:
        write_case(spec)
        print(f"scaffolded {spec['id']}")


if __name__ == "__main__":
    main()
