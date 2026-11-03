在后台启动一个命令，然后在同一个回合里继续工作，等它的结果。

1. 用 `shell` 启动后台命令：`program = "python"`，`args = ["-c", "import time,pathlib; time.sleep(3); pathlib.Path('bg.txt').write_text('bg done')"]`，`background = "true"`。不要等它结束。
2. 立刻用 `write_file` 写 `main.txt`，内容一行 `main`。
3. 然后运行一个前台命令等待后台完成：`program = "python"`，`args = ["-c", "import time; time.sleep(4)"]`。
4. 后台结果出现后，读取 `bg.txt` 确认内容，并在回答里说明后台命令的退出码和输出路径。
