# 02 — Profile and policy

A profile is data. The middle layer resolves it into an immutable
`RuntimeConfig` once per turn. The kernel reads that config; it never reads a
profile file and never sees a variant name.

## Resolution

```
default profile -> variant profile -> session overrides
```

- Later layers override earlier ones.
- The result is validated and frozen before the first model call.
- Unknown fields are errors, not silently ignored.
- Secrets are references (`env:NAME`, `keyring:NAME`), never inline values.

## Schema

```toml
[profile]
name = "coding"
extends = "default"

[model]
provider = "openai-compatible"
route = "coding"
thinking_budget = 1024
temperature = 0.2
max_output_tokens = 4096

[model.roles]
main = "coding"
compact = "cheap"
subagent = "coding"
verifier = "coding"
commit = "coding"

[context]
stages = ["system", "best_practices", "task_state", "memory", "history", "latest"]
budget_tokens = 16000
compaction = ["trim", "rolling_summary", "llm_compact"]

[tools]
visible = ["shell", "read_file", "write_file", "list_dir", "code_search", "todo"]
budget = 12
allow_parallel_reads = true

[verification]
mode = "tests"          # none | evidence | tests | custom
require_tests = true
max_reprompts = 2

[memory]
mode = "facts"          # off | facts | episodic
promotion = "strict"    # strict | soft
recall_limit = 8

[permission]
level = "semi_auto"
workspace_trusted = true
confirm_external_paths = true

[telemetry]
level = "metadata"      # off | metadata | content
sample_rate = 1.0
redact = ["paths", "secrets", "content"]
export = "local-jsonl"

[persona]
system = "prompts/coding.md"
style = "direct"
```

## Coding variant

The example above. Characteristics:

- shell and file tools visible;
- test-based verification;
- strict memory promotion;
- 1K thinking budget;
- higher tool budget.

## Companion variant

```toml
[profile]
name = "companion"
extends = "default"

[model]
provider = "local"
route = "companion"
thinking_budget = 0
temperature = 0.8

[context]
stages = ["persona", "memory", "history", "latest"]
budget_tokens = 12000
compaction = ["rolling_summary"]

[tools]
visible = []
budget = 0

[verification]
mode = "none"

[memory]
mode = "episodic"
promotion = "soft"
recall_limit = 12

[permission]
level = "read_only"

[telemetry]
level = "metadata"
sample_rate = 0.2

[persona]
system = "prompts/companion.md"
style = "warm"
```

The companion variant must run with **no kernel code changes**. If it needs an
`if companion` branch in the kernel, the kernel contract is missing a policy
question.

## Policy questions

The kernel may ask only these at runtime:

| Question | Return |
|---|---|
| `visible_tools` | tool schemas for this turn |
| `context_recipe` | stage names, priorities, budget, compaction steps |
| `thinking_budget` | tokens |
| `verification_mode` | none / evidence / tests / custom |
| `memory_policy` | mode, promotion, recall limit |
| `model_route` | provider + model per role |
| `before_commit` | commit / re-prompt / ask |

New questions require an ADR and a second consumer.

## Validation

- `visible_tools` must be a subset of registered tools.
- `context.stages` must reference registered stages.
- `budget_tokens` must be positive and <= the model context window.
- `thinking_budget` must be within the provider's supported range.
- `verification.mode = tests` requires a test runner tool.
- `permission.level` cannot be escalated by a plugin.
- A profile cannot grant a capability the host did not install.

## Non-goals

- No arbitrary code in profiles.
- No per-token policy evaluation.
- No hidden defaults: every default is documented in `default.toml`.
- No variant names in kernel code.
