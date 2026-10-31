# 08 — Glossary

Use these terms consistently. Do not import vocabulary from another project.

| Term | Meaning |
|---|---|
| Kernel | Mechanisms and invariants; no variant knowledge |
| Middle layer | Policy resolution and composition |
| Plugin | Optional capability: tools, channels, memory backends, UI |
| Profile | Declarative variant configuration |
| RuntimeConfig | Immutable resolved policy for one turn |
| Turn | One user request and the loop that handles it |
| Stage | A context fragment producer |
| Fragment | A piece of prompt content with a priority and token estimate |
| Tool | A typed capability the model can call |
| Tool schema | Name, description, JSON schema and risk level |
| Policy question | A fixed question the kernel asks the middle layer |
| Composition | The plugin set and stage set selected by a profile |
| Audit | Always-on, append-only security record |
| Telemetry | Sampled operational events |
| Eval trace | Full trajectory captured for evaluation |
| Variant | A product shape: profile + plugins + data + evals |
