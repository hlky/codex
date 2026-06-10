# Architecture Notes

## Desired abstraction

Add a first-class config concept for named local environment overlays. This is
separate from remote or stdio-backed exec-server environments.

The abstraction should remain cross-platform. Windows-specific procedural
toolchain setup is one important motivating case, but not the defining shape of
the feature.

Conceptually:

- local environment = env map derivation strategy + metadata
- selection = sticky thread default with turn/tool-call overrides
- execution = existing local shell / unified exec paths with a different env map

## Locked MVP decisions

These decisions are fixed for the MVP and should not be reopened during
implementation unless they prove technically impossible.

- Static named local environments only.
- User config only. Project-local config may be revisited later.
- Local environment selection is stored separately from existing remote
  environment selection.
- The selected local environment replaces the session-level
  `shell_environment_policy` for shell/unified-exec env construction, except
  for runtime-owned vars that must still be injected by Codex afterward.
- Thread and turn selection must be visible through app-server protocol fields,
  gated as experimental if needed.

## Proposed config model

Illustrative only:

```toml
[local_environments.rocm]
description = "ROCm Python/toolchain environment"

[local_environments.rocm.shell_environment_policy]
inherit = "all"
exclude = ["PATH"]
set = { VIRTUAL_ENV = "H:\\dinoml_v2\\.venv\\rocm", Path = "..." }

[local_environments.msvc]
description = "MSVC build environment"

[local_environments.msvc.vcvars]
script = "C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\VC\\Auxiliary\\Build\\vcvars64.bat"
host = "x64"
target = "x64"
```

Likely internal shape:

- `LocalEnvironmentToml`
- `LocalEnvironmentSourceToml`
  - `ShellEnvironmentPolicy`
  - script-derived env snapshot
  - Windows `vcvars` helper

For MVP, only the static `ShellEnvironmentPolicy` source is in scope. The other
source kinds are follow-up work and should not distort the core model.

## Key code touchpoints

### Config parsing and schema

- `codex-rs/config/src/config_toml.rs`
- `codex-rs/config/src/types.rs`
- `codex-rs/core/config.schema.json`

Why:

- add user-facing config
- preserve schema generation
- avoid overloading remote `environments.toml`

### Env construction

- `codex-rs/protocol/src/shell_environment.rs`
- `codex-rs/core/src/tools/handlers/shell/shell_command.rs`
- `codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs`
- `codex-rs/core/src/tools/handlers/shell.rs`

Why:

- these already create the env map used by shell-like tools
- this is the natural insertion point for local overlay resolution

### Session / turn state

- `codex-rs/core/src/session/session.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/turn_context.rs`

Why:

- sticky thread setting belongs here
- turn-level override belongs here
- tool call resolution already uses turn context

### Model-visible context

- `codex-rs/core/src/context/environment_context.rs`
- `codex-rs/core/src/context/environment_context_tests.rs`

Why:

- expose available/current local environments compactly
- avoid large prompt churn

### App-server protocol and request plumbing

- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/turn.rs`
- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/src/request_processors/turn_processor.rs`

Why:

- thread/turn overrides are part of MVP completion criteria
- use a separate local-environment selector rather than reusing remote
  environment ids

## Design choices

### Keep local overlays separate from exec-server environments

Reason:

- exec-server environments model execution backends
- this feature needs multiple local process env maps on one machine
- reusing remote environment selection would confuse "where command runs" with
  "which env vars are injected"

### Keep `shell_environment_policy` as a building block

Reason:

- existing behavior and tests remain useful
- local named environments can compile down to an effective policy or env map
- minimizes churn in command execution paths

For MVP:

- the selected local environment is authoritative for shell-like env
  construction
- session-level `shell_environment_policy` remains the fallback only when no
  local environment is selected

### Add first-class procedural env derivation

Reason:

- some local environments are procedural rather than purely declarative
- users should not need to hand-encode `PATH`, `INCLUDE`, `LIB`, and SDK vars

Examples:

- Windows MSVC `vcvars`
- later generic script/materialized environment sources on other platforms

Likely supported derivation modes:

- static policy
- generic script -> env snapshot
- ergonomic `vcvars` helper

### Cache derived snapshots

Reason:

- running `vcvars*.bat` for every command would be too slow
- heavy environment setup should resolve once per session or until invalidated

Possible cache key:

- source kind
- script path
- args / host / target
- cwd if relevant
- file mtime

## Prompt / tool semantics

Selection precedence should be:

1. tool-call explicit local environment
2. turn override
3. thread sticky selection
4. config default
5. existing behavior

For MVP, thread and turn selectors should use a distinct field name such as
`local_environment` rather than overloading remote `environment_id`
semantics.

Tool-call UX may mirror current `environment_id` style later, but that is not
part of MVP.

## Risks

- prompt churn if environment context becomes verbose
- complexity if remote environments and local environments share one selector
- Windows command quoting if script-derived env setup shells out through
  `cmd.exe /c` or `powershell -Command`
- hidden performance regressions if derived env snapshots are not cached

## Recommended boundary

The first implementation should support:

- config-defined named local environments
- one current local environment selection
- shell/unified exec honoring that selection
- compact prompt visibility
- app-server thread/turn plumbing for selecting the current local environment

The first implementation explicitly does not support:

- script-derived local environments
- `vcvars` helpers
- per-tool explicit local environment override

That limitation is about keeping the MVP bounded, not about constraining the
feature to Windows-specific use cases.

It should not try to unify:

- remote exec-server environments
- MCP remote environment selection
- worktree `Local environments` app feature

## Remaining questions after MVP

1. Should tool-call selection reuse `environment_id` or introduce a distinct
   `local_environment_id`?
2. Should project-local `.codex/config.toml` later be allowed to define local
   environment names while user config provides machine-specific source details?
3. How should approval prompts display the selected local environment so users
   can verify command context?
