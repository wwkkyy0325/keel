"""A minimal MCP stdio server over SQLite, used to test a non-Node runtime.

It implements the handshake, tools/list, tools/call and one server-initiated
`roots/list` request. The database is the file passed as the first argument.
Standard library only.
"""

import json
import os
import sqlite3
import sys


def respond(message):
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


ROOTS_ID = 900


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    if len(sys.argv) > 2:
        with open(sys.argv[2], "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    database = sqlite3.connect(sys.argv[1] if len(sys.argv) > 1 else ":memory:")
    roots = {"uri": None}
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "method" not in message:
            # A response to the roots request this server sent after
            # initialize. `roots_seen` exposes what the client answered.
            if message.get("id") == ROOTS_ID:
                entries = (message.get("result") or {}).get("roots") or [{}]
                roots["uri"] = entries[0].get("uri")
            continue
        if "id" not in message:
            continue
        method = message.get("method")
        if method == "initialize":
            result = {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "keel-sqlite-fixture", "version": "0.1.0"},
            }
        elif method == "tools/list":
            result = {
                "tools": [
                    {
                        "name": "sqlite_query",
                        "description": "Run one SQL statement and return its rows as JSON.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"sql": {"type": "string"}},
                            "required": ["sql"],
                        },
                        "annotations": {"readOnlyHint": False},
                    },
                    {
                        "name": "roots_seen",
                        "description": "Return the workspace URI the client answered.",
                        "inputSchema": {"type": "object", "properties": {}},
                        "annotations": {"readOnlyHint": True},
                    },
                ]
            }
        elif method == "tools/call":
            name = message.get("params", {}).get("name")
            if name == "roots_seen":
                result = {
                    "content": [{"type": "text", "text": roots["uri"] or "none"}],
                    "isError": False,
                }
            else:
                arguments = message.get("params", {}).get("arguments", {})
                sql = arguments.get("sql", "")
                try:
                    cursor = database.execute(sql)
                    rows = cursor.fetchall() if cursor.description else []
                    database.commit()
                    result = {
                        "content": [
                            {"type": "text", "text": json.dumps(rows, ensure_ascii=False)}
                        ],
                        "isError": False,
                    }
                except Exception as error:  # noqa: BLE001 - report the message to the client
                    result = {
                        "content": [{"type": "text", "text": str(error)}],
                        "isError": True,
                    }
        else:
            respond(
                {
                    "jsonrpc": "2.0",
                    "id": message["id"],
                    "error": {"code": -32601, "message": "method not found"},
                }
            )
            continue
        if method == "initialize":
            # Sent before the initialize response, so the client answers it in
            # the same read loop and the answer is stored before tools/list.
            respond(
                {"jsonrpc": "2.0", "id": ROOTS_ID, "method": "roots/list", "params": {}}
            )
        respond({"jsonrpc": "2.0", "id": message["id"], "result": result})


if __name__ == "__main__":
    main()
