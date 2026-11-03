Delegate one small task to a sub-agent and verify its work.

1. Call `spawn_agent` exactly once with `role = "proofreader"` and this task:
   "Read notes.txt in the current workspace, convert its content to upper
   case, write the result to alpha.txt, then report what you wrote."
2. After it returns, read `alpha.txt` yourself and confirm the content.
3. Do not write `alpha.txt` yourself and do not skip the sub-agent. If the
   sub-agent fails, report the failure instead of doing the work.
4. In your answer, state who wrote `alpha.txt` and the sub-agent's reported
   model and tool call counts.
