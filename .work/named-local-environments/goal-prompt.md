# Goal Prompt

Implement first-class named local environment overlays in Codex using the work
pack in [README.md](H:/openai/codex/.work/named-local-environments/README.md),
[architecture-notes.md](H:/openai/codex/.work/named-local-environments/architecture-notes.md),
and [implementation-plan.md](H:/openai/codex/.work/named-local-environments/implementation-plan.md).

Keep the abstraction cross-platform. Windows MSVC / ROCm is a motivating
example, but the MVP should land as a generic local-environment selection
feature for projects that need more than one local command environment in the
same session.

## Objective

Ship the MVP slices:

1. Config model for static named local environments.
2. Thread and turn state for local environment selection.
3. Shell and unified-exec honoring the selected local environment.
4. Compact `<environment_context>` showing available/current local environments.

## Locked MVP decisions

- Static named local environments only.
- User-level config only.
- Local environment selection is separate from remote environment selection.
- A selected local environment replaces session `shell_environment_policy` for
  shell-like execution, except runtime-owned vars injected afterward.
- Thread and turn selectors are part of MVP and must be exposed through
  app-server protocol/request plumbing, gated as experimental if needed.

## Scope

- Reuse existing local shell and unified exec backends.
- Do not route this through remote/stdio exec-server environments.
- Do not change worktree `.codex/environments/environment.toml`.
- Keep remote environment behavior unchanged.
- Prefer minimal changes near existing shell environment and session plumbing.

## Relevant areas

- `codex-rs/config/src/config_toml.rs`
- `codex-rs/config/src/types.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/protocol/src/shell_environment.rs`
- `codex-rs/core/src/session/*`
- `codex-rs/core/src/tools/handlers/shell*`
- `codex-rs/core/src/tools/handlers/unified_exec/*`
- `codex-rs/core/src/context/environment_context.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/turn.rs`
- `codex-rs/app-server/src/request_processors/*`

## Constraints

- Follow `AGENTS.md` and repo Rust conventions.
- Keep slices bounded and reviewable.
- Avoid coupling local overlays to remote exec-server environment ids.
- Add tests for user-visible behavior changes.
- Regenerate schema artifacts if config schema changes.
- Run `just fmt` in `codex-rs` after edits.
- Run targeted tests for changed crates.
- Ask before running full `just test` if shared-crate changes would require it.

## Completion criteria

- Config supports multiple named local environments with static
  `shell_environment_policy` definitions and a default selection.
- Session state stores a sticky selected local environment and a turn override.
- App-server thread/turn plumbing supports selecting the current local
  environment via experimental fields if needed.
- Shell/unified-exec uses the selected local environment when present and keeps
  existing behavior when unset.
- `<environment_context>` compactly shows available/current local environments.
- Tests cover config parsing, session/turn behavior, protocol round trips where
  touched, command env application, and environment-context rendering.
- Code is formatted and targeted tests pass.

## Non-goals

- Per-tool explicit local environment override.
- Script-derived environment snapshots.
- Windows `vcvars` helper.
- Project-local config support.
- Remote environment redesign.

## Final output

Report:

- what was implemented
- what remains for later slices
- any open design questions encountered
- exact files changed
