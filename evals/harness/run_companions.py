#!/usr/bin/env python3
"""Run the five companion personas for twenty turns each and check their rules.

Each persona lives in `variants/companion/personas/` with a profile beside it.
The scenario states three durable facts early, chats, then asks for them in
turns 17-19, so the same run covers persona adherence and multi-turn memory.

Usage:
    python evals/harness/run_companions.py [--max-total-tokens N] [--only NAME]
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ACCEPTANCE = """[[items]]
id = "companion-seal"
description = "compute one plus one"
kind = "compute"
expression = "1 + 1"
expected = "2"
"""

CHINESE_SCENARIO = [
    "你好，先认识一下。",
    "请记住：我的项目代号是蓝鲸。",
    "请记住：部署窗口是周二晚八点。",
    "请记住：测试数据库叫 starlight。",
    "今天有点累，随便聊两句。",
    "你平时会怎么安排一天的工作？",
    "如果今天只能做一件事，应该先做什么？",
    "给我一句鼓励。",
    "你会怎么记录重要的事情？",
    "周末你建议我做什么？",
    "我有点拖延，怎么办？",
    "用一句话总结我们刚才聊的。",
    "你觉得长期主义重要吗？",
    "给我一个今天就能做的小动作。",
    "你怎么看待失败？",
    "跟我说声晚安。",
    "不要查文件，直接回答：我的项目代号是什么？",
    "不要查文件，直接回答：部署窗口是什么时候？",
    "不要查文件，直接回答：测试数据库叫什么？",
    "用一句话介绍你自己。",
]

PIRATE_SCENARIO = [
    "Ahoy, let us get acquainted.",
    "Remember: my project code name is blue whale.",
    "Remember: my deploy window is Tuesday 8pm.",
    "Remember: my test database is starlight.",
    "I am a bit tired today, let us chat.",
    "How do you plan a day of work?",
    "If I can do only one thing today, what should it be?",
    "Give me one word of encouragement.",
    "How do you record important things?",
    "What do you suggest for the weekend?",
    "I keep procrastinating, what should I do?",
    "Sum up what we talked about in one sentence.",
    "Does long-term thinking matter?",
    "Give me one small action I can do today.",
    "What do you think about failure?",
    "Say goodnight to me.",
    "Do not read files, just answer: what is my project code name?",
    "Do not read files, just answer: when is my deploy window?",
    "Do not read files, just answer: what is my test database?",
    "Introduce yourself in one sentence.",
]

# Each entry is (zero-based turn, [alternatives for each required part]). A
# time can be written as "八点", "晚八点" or "20:00"; all are the same fact.
CHINESE_FACTS = [
    (16, [["蓝鲸"]]),
    (17, [["周二"], ["八点", "晚八点", "20:00", "8点", "8:00"]]),
    (18, [["starlight"]]),
]
PIRATE_FACTS = [
    (16, [["blue whale"]]),
    (17, [["tuesday"], ["8pm", "8 pm", "20:00", "eight"]]),
    (18, [["starlight"]]),
]


def has_emoji(text: str) -> bool:
    return any(
        0x1F300 <= ord(character) <= 0x1FAFF
        or 0x2600 <= ord(character) <= 0x27BF
        for character in text
    )


def sentence_count(text: str) -> int:
    parts = re.split(r"[.!?。！？]+", text)
    return len([part for part in parts if part.strip()])


def nonempty_lines(text: str) -> int:
    return len([line for line in text.strip().splitlines() if line.strip()])


def check_archivist(body: str) -> dict[str, bool]:
    return {
        "keyword": "档案" in body,
        "sentences": sentence_count(body) <= 4,
        "no_emoji": not has_emoji(body),
        "no_exclaim": "!" not in body and "！" not in body,
    }


def check_coach(body: str) -> dict[str, bool]:
    return {
        "ends_question": body.rstrip().endswith(("?", "？")),
        "sentences": sentence_count(body) <= 3,
        "no_emoji": not has_emoji(body),
    }


def check_pirate(body: str) -> dict[str, bool]:
    return {
        "keyword": bool(re.search(r"\b(arr|matey|ahoy|ye)\b", body.lower())),
        "sentences": sentence_count(body) <= 3,
        "no_emoji": not has_emoji(body),
    }


def check_poet(body: str) -> dict[str, bool]:
    return {
        "lines": nonempty_lines(body) <= 5,
        "no_bullets": not re.search(r"^\s*([-*•]|\d+[.)])\s", body, re.M),
        "imagery": any(
            word in body for word in ["月", "风", "海", "灯", "雨", "山", "云", "星"]
        ),
        "no_emoji": not has_emoji(body),
    }


def check_engineer(body: str) -> dict[str, bool]:
    return {
        "prefix": body.lstrip().startswith("TL;DR:"),
        "lines": nonempty_lines(body) <= 8,
        "no_emoji": not has_emoji(body),
    }


PERSONAS = [
    {
        "name": "archivist",
        "profile": "variants/companion/profile.archivist.toml",
        "scenario": CHINESE_SCENARIO,
        "facts": CHINESE_FACTS,
        "check": check_archivist,
        "thresholds": {"keyword": 18, "sentences": 18, "no_emoji": 20, "no_exclaim": 20},
    },
    {
        "name": "coach",
        "profile": "variants/companion/profile.coach.toml",
        "scenario": CHINESE_SCENARIO,
        "facts": CHINESE_FACTS,
        "check": check_coach,
        "thresholds": {"ends_question": 18, "sentences": 17, "no_emoji": 20},
    },
    {
        "name": "pirate",
        "profile": "variants/companion/profile.pirate.toml",
        "scenario": PIRATE_SCENARIO,
        "facts": PIRATE_FACTS,
        "check": check_pirate,
        "thresholds": {"keyword": 15, "sentences": 17, "no_emoji": 20},
    },
    {
        "name": "poet",
        "profile": "variants/companion/profile.poet.toml",
        "scenario": CHINESE_SCENARIO,
        "facts": CHINESE_FACTS,
        "check": check_poet,
        "thresholds": {"lines": 18, "no_bullets": 20, "imagery": 15, "no_emoji": 20},
    },
    {
        "name": "engineer",
        "profile": "variants/companion/profile.engineer.toml",
        "scenario": CHINESE_SCENARIO,
        "facts": CHINESE_FACTS,
        "check": check_engineer,
        "thresholds": {"prefix": 19, "lines": 18, "no_emoji": 20},
    },
]


def binary_path() -> Path:
    name = "keel-cli.exe" if os.name == "nt" else "keel-cli"
    return REPO_ROOT / "target" / "debug" / name


def run_persona(persona: dict, max_total_tokens: int) -> dict:
    workdir = Path(tempfile.mkdtemp(prefix=f"keel-companion-{persona['name']}-"))
    workspace = workdir / "workspace"
    workspace.mkdir()
    store = workdir / "memory.db"
    acceptance = workdir / "acceptance.toml"
    acceptance.write_text(ACCEPTANCE, encoding="utf-8")

    command = [
        str(binary_path()),
        "--profile",
        persona["profile"],
        "--workspace",
        str(workspace),
        "--store",
        str(store),
        "--session-id",
        persona["name"],
        "--session",
        "--no-todo",
        "--acceptance",
        str(acceptance),
    ]
    started = dt.datetime.now(dt.timezone.utc)
    process = subprocess.Popen(
        command,
        cwd=REPO_ROOT,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    try:
        process.stdin.write("\n".join(persona["scenario"]) + "\n")
        process.stdin.close()
    except BrokenPipeError:
        pass

    total_tokens = 0
    truncated = False
    for line in process.stdout:
        if line.startswith("usage:"):
            match = re.search(r"total=(\d+)", line)
            if match:
                total_tokens += int(match.group(1))
            if max_total_tokens and total_tokens >= max_total_tokens:
                truncated = True
                process.terminate()
                break
    stderr = process.stderr.read()
    process.wait()

    turns = []
    if store.exists():
        connection = sqlite3.connect(store)
        connection.row_factory = sqlite3.Row
        for row in connection.execute(
            "SELECT seq, state, answer_json FROM turns ORDER BY seq"
        ):
            answer = {}
            try:
                answer = json.loads(row["answer_json"])
            except (TypeError, json.JSONDecodeError):
                pass
            turns.append(
                {
                    "seq": row["seq"],
                    "state": row["state"],
                    "body": answer.get("body", ""),
                }
            )
        connection.close()

    per_turn = []
    failures = []
    for turn in turns:
        result = persona["check"](turn["body"])
        result["_seq"] = turn["seq"]
        per_turn.append(result)
        if turn["state"] != "Done":
            failures.append(f"turn {turn['seq']} ended {turn['state']}")

    counts = {
        key: sum(1 for result in per_turn if result.get(key))
        for key in persona["thresholds"]
    }
    for key, minimum in persona["thresholds"].items():
        if counts.get(key, 0) < minimum:
            failures.append(f"{key}: {counts.get(key, 0)} < {minimum}")

    memory = []
    for index, groups in persona["facts"]:
        body = turns[index]["body"].lower() if index < len(turns) else ""
        hit = all(
            any(alternative.lower() in body for alternative in group)
            for group in groups
        )
        memory.append({"turn": index + 1, "expected": groups, "hit": hit})
        if not hit:
            failures.append(f"memory at turn {index + 1} missed {groups}")

    return {
        "name": persona["name"],
        "started_at": started.isoformat(),
        "turns": len(turns),
        "done": sum(1 for turn in turns if turn["state"] == "Done"),
        "total_tokens": total_tokens,
        "truncated": truncated,
        "counts": counts,
        "thresholds": persona["thresholds"],
        "memory": memory,
        "failures": failures,
        "passed": len(turns) == len(persona["scenario"]) and not failures,
        "stderr_tail": stderr[-800:],
        "turns_detail": [
            {"seq": turn["seq"], "state": turn["state"], "body": turn["body"][:400]}
            for turn in turns
            if turn["state"] != "Done" or not persona["check"](turn["body"]).get(
                next(iter(persona["thresholds"]))
            )
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--max-total-tokens", type=int, default=2_000_000)
    parser.add_argument("--only", help="run one persona by name")
    args = parser.parse_args()

    selected = [
        persona
        for persona in PERSONAS
        if not args.only or persona["name"] == args.only
    ]
    if not selected:
        print(f"unknown persona `{args.only}`")
        return 1

    subprocess.run(
        ["cargo", "build", "-q", "-p", "keel-cli"],
        cwd=REPO_ROOT,
        check=True,
    )

    results = []
    for persona in selected:
        result = run_persona(persona, args.max_total_tokens)
        results.append(result)
        print(
            f"{persona['name']}: turns={result['turns']} done={result['done']} "
            f"tokens={result['total_tokens']} "
            f"{'PASS' if result['passed'] else 'FAIL'}"
        )
        for failure in result["failures"]:
            print(f"  - {failure}")

    REPORTS_ROOT = REPO_ROOT / "evals" / "reports" / "runs"
    REPORTS_ROOT.mkdir(parents=True, exist_ok=True)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    report_path = REPORTS_ROOT / f"companion-{stamp}.json"
    report_path.write_text(
        json.dumps(
            {
                "started_at": dt.datetime.now(dt.timezone.utc).isoformat(),
                "results": results,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"report: {report_path.relative_to(REPO_ROOT)}")
    return 0 if all(result["passed"] for result in results) else 1


if __name__ == "__main__":
    sys.exit(main())
