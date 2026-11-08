#!/usr/bin/env python3
"""Measure how much of a turn's model input is tool results.

Reads the event stream kept by `run_case.py --keep` (`<state_home>/keel.db`)
and reports per turn:

- calls   model calls
- tools   tool calls
- input   total input tokens the provider reported
- inject  input(call n+1) - input(call n) - output(call n), summed
- share   inject / input, over the calls after the first

The delta estimates what the previous round's tool results added to the next
request: the request holds the earlier conversation, the assistant message
whose size is the previous output count, and the new tool results. A reminder
or an elided segment moves the number in either direction, so negative deltas
are counted and reported rather than clipped.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path

READ_TOOLS = {"read_file", "code_search", "list_dir"}


def unwrap(name: str, raw: str) -> dict:
    """The payload is stored bare; tolerate the tagged form too."""
    try:
        value = json.loads(raw)
    except (TypeError, ValueError):
        return {}
    if not isinstance(value, dict):
        return {}
    inner = value.get(name)
    return inner if isinstance(inner, dict) else value


def read_turns(db: Path) -> dict[int, list[tuple[str, dict]]]:
    connection = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        rows = connection.execute(
            "SELECT turn_seq, seq, name, payload_json FROM events ORDER BY turn_seq, seq"
        )
        turns: dict[int, list[tuple[str, dict]]] = {}
        for turn_seq, _seq, name, raw in rows:
            turns.setdefault(turn_seq, []).append((name, unwrap(name, raw)))
        return turns
    finally:
        connection.close()


def analyze_turn(events: list[tuple[str, dict]]) -> dict:
    calls: list[dict] = []
    tools_since = 0
    names_since: list[str] = []

    for name, payload in events:
        if name == "tool_started":
            tools_since += 1
            names_since.append(str(payload.get("tool", "")))
        elif name == "model_finished":
            usage = payload.get("usage") or {}
            calls.append(
                {
                    "input": int(usage.get("input_tokens", 0)),
                    "output": int(usage.get("output_tokens", 0)),
                    "cached": int(usage.get("cached_input_tokens", 0)),
                    "tools": tools_since,
                    "names": names_since,
                }
            )
            tools_since = 0
            names_since = []

    deltas = []
    for previous, current in zip(calls, calls[1:]):
        deltas.append(current["input"] - previous["input"] - previous["output"])

    base = sum(call["input"] for call in calls[1:])
    injected = sum(deltas)


    longest = run = 0
    for call in calls:
        for tool in call["names"]:
            if tool in READ_TOOLS:
                run += 1
                longest = max(longest, run)
            else:
                run = 0

    return {
        "calls": len(calls),
        "tools": sum(call["tools"] for call in calls),
        "input": sum(call["input"] for call in calls),
        "output": sum(call["output"] for call in calls),
        "cached": sum(call["cached"] for call in calls),
        "injected": injected,
        "base": base,
        "share": (injected / base) if base else 0.0,
        "negative_deltas": sum(1 for delta in deltas if delta < 0),
        "longest_read_chain": longest,
        "deltas": deltas,
        "tool_names": [name for call in calls for name in call["names"]],
    }


def summarize(turns: dict[int, dict]) -> dict:
    calls = sum(turn["calls"] for turn in turns.values())
    base = sum(turn["base"] for turn in turns.values())
    injected = sum(turn["injected"] for turn in turns.values())
    return {
        "turns": len(turns),
        "calls": calls,
        "rounds_per_turn": (calls / len(turns)) if turns else 0.0,
        "input": base,
        "output": sum(turn["output"] for turn in turns.values()),
        "cached": sum(turn["cached"] for turn in turns.values()),
        "injected": injected,
        "share": (injected / base) if base else 0.0,
        "longest_read_chain": max(
            (turn["longest_read_chain"] for turn in turns.values()), default=0
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("state", type=Path, nargs="+", help="a state dir or a keel.db")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--detail", action="store_true", help="one row per turn")
    args = parser.parse_args()

    payload = {}
    for state in args.state:
        db = state / "keel.db" if state.is_dir() else state
        if not db.exists():
            print(f"no event database at {db}", file=sys.stderr)
            return 2
        turns = {
            turn_seq: analyze_turn(events)
            for turn_seq, events in read_turns(db).items()
        }
        if args.detail:
            for turn_seq, turn in sorted(turns.items()):
                print(
                    f"  turn {turn_seq:>2}  calls {turn['calls']:>2}  "
                    f"tools {turn['tools']:>2}  input {turn['input']:>7,}  "
                    f"inject {turn['injected']:>7,}  share {turn['share']:>6.1%}  "
                    f"chain {turn['longest_read_chain']:>2}"
                )
        payload[state.name if state.is_dir() else db.stem] = {
            "summary": summarize(turns),
            "turns": {str(key): value for key, value in sorted(turns.items())},
        }

    if args.json:
        print(json.dumps(payload, indent=2))
        return 0

    for label, entry in payload.items():
        summary = entry["summary"]
        print(
            f"{label}: turns {summary['turns']}  calls {summary['calls']}  "
            f"rounds/turn {summary['rounds_per_turn']:.1f}  "
            f"input {summary['input']:,}  output {summary['output']:,}  "
            f"inject {summary['injected']:,}  share {summary['share']:.1%}  "
            f"cached {summary['cached']:,}  longest read chain "
            f"{summary['longest_read_chain']}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
