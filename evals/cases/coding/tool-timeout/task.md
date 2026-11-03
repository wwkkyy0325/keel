验证静默命令会被活动超时停止。

1. 用 `shell` 运行：`program = "python"`，`args = ["-c", "import time; time.sleep(5)"]`。这个命令五秒内没有任何输出，预期会被工具的超时窗口停止。
2. 读取工具返回的错误，然后用 `write_file` 写 `timeout.txt`，内容包含 `timed out` 这个词，并在回答里引用工具实际的错误文字。
3. 不要重试这条命令，也不要换成别的命令绕过它。
