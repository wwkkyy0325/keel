运行一条慢命令，等待运行中到达的额外消息。

1. 用 `shell` 运行：`program = "python"`，`args = ["-c", "import time; time.sleep(3)"]`。
2. 命令结束后，如果收到额外消息，就按那条消息执行；没有收到就回答 `no extra message`。
3. 不要猜测额外消息的内容，它只会出现在运行中。
