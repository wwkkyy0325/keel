# Keyring credentials

The spec allowed `.env` or the OS keyring; only `.env` was wired. This adds a
keyring fallback and a small CLI to manage entries.

## Steps

- [x] `RuntimeConfig` and the profile schema carry `api_key_keyring`
      (`service:account`), a name only. Verify:
      `anthropic_profile_loads_and_validates`.
- [x] `resolve_api_key` tries the profile's `api_key_env` (or the provider
      default) first, then the keyring entry (or `keel:<provider>`). A missing
      entry names both sources and the `--keyring-set` command. Verify:
      `keyring_names_need_a_service_and_an_account` plus the smoke.
- [x] `--keyring-set service:account` reads the secret from stdin and never
      prints it; `--keyring-delete` removes the entry and reports a missing
      entry. Verify: the smoke.
- [x] Per-platform backends: Windows Credential Manager, macOS Keychain,
      Linux kernel keyutils (`keyring` target-specific dependencies).
- [x] Real smoke on Windows: stored `keel-smoke:test`; a temporary profile
      with a nonexistent env var and that keyring entry pointed at a local
      Anthropic SSE server. The CLI finished `state: Done`, the server saw
      `x-api-key: smoke-secret`, and the entry was deleted; a second delete
      reported "does not exist".

## Boundaries

- Linux uses the kernel keyring, which is session-scoped; it is not a Secret
  Service replacement. A product on Linux should add the secret-service
  backend.
- A terminal echoes typed input; `--keyring-set` warns and expects a pipe
  from a password manager. Hidden input is a UI feature, not wired here.
- Environment variables win over the keyring, so an exported key always takes
  precedence during debugging.
- The secret is never logged or serialized; existing secret-pattern redaction
  still applies to input and events.
