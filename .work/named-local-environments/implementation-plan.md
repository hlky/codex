# Implementation Plan

## Goal

Implement named local environment overlays that are selectable within the same
 thread, without routing command execution through remote-style exec-server
 environments.

The MVP should remain platform-neutral. Windows-specific MSVC support is a
follow-up helper layered on a generic local-environment model, not the defining
shape of the feature.

## Boundaries

- Reuse existing local shell and unified exec backends.
- Keep remote exec-server environments unchanged.
- Keep worktree `.codex/environments/environment.toml` unchanged.
- Use experimental API/config gating where protocol surface must change.
- MVP is static-policy only. Script-derived envs and `vcvars` are follow-up
  slices.
- MVP is user-config-only.
- Frame examples and naming so the core feature reads as cross-platform.

## Locked MVP decisions

- Local environment selection uses distinct state and protocol fields rather
  than reusing remote environment selection.
- A selected local environment replaces session `shell_environment_policy` for
  shell-like env construction, except runtime-owned vars injected afterward.
- Thread and turn selectors are part of MVP and must be plumbed through
  app-server protocol/request handling as experimental fields if required.

## Slice 0: Terminology and scope guard

Purpose:

- separate "local environment overlays" from existing remote environments
- avoid conflating feature scope during implementation

Changes:

- add internal design comments / naming
- reserve a dedicated config and state model for local environment selection

Files likely touched:

- `codex-rs/config/src/types.rs`
- `codex-rs/core/src/session/*`

Expected size:

- under 150 LoC

Validation:

- config/unit tests only

## Slice 1: Config model for static named local environments

Purpose:

- support multiple named local environments that each wrap a static
  `shell_environment_policy`

Examples this slice should be able to express:

- Python venv vs system env
- minimal env vs full dev env
- Windows ROCm/static MSVC-like env approximations

Changes:

- add `local_environments.<name>` config schema
- parse into runtime config
- support optional `description`
- add optional default selector
- user-config only; reject or ignore project-local definitions for MVP

Files likely touched:

- `codex-rs/config/src/config_toml.rs`
- `codex-rs/config/src/types.rs`
- `codex-rs/core/src/config/mod.rs`
- generated schema

Expected size:

- 250 to 400 LoC

Validation:

- config parsing tests
- schema update / schema tests

Notes:

- this slice does not yet change execution behavior
- no script-derived sources in this slice

## Slice 2: Thread and turn state for local environment selection

Purpose:

- store one selected local environment for the session/thread
- allow turn-level override

Changes:

- add selected local environment to session configuration
- plumb turn override through thread/turn settings
- preserve default behavior when unset
- use distinct local-environment fields instead of remote environment ids

Files likely touched:

- `codex-rs/core/src/session/session.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/turn_context.rs`
- app-server v2 thread/turn protocol types
- thread/turn request processors

Expected size:

- 300 to 450 LoC

Validation:

- session state tests
- app-server protocol round-trip tests
- thread/turn processor tests

Notes:

- keep protocol fields behind experimental API if needed
- this slice is required for MVP; do not defer app-server plumbing

## Slice 3: Command execution uses selected static local environment

Purpose:

- make shell-like tools honor the selected local environment

Changes:

- resolve selected local environment to an effective env map
- merge runtime-owned vars after environment resolution
- preserve existing `shell_environment_policy` behavior when no local selection
  exists
- when a local selection exists, do not merge the session policy on top of it

Files likely touched:

- `codex-rs/protocol/src/shell_environment.rs`
- `codex-rs/core/src/tools/handlers/shell/shell_command.rs`
- `codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs`
- possibly helper module under `core/src/exec_env*`

Expected size:

- 250 to 400 LoC

Validation:

- integration tests for shell and unified exec
- Windows-sensitive env merge tests where possible

Notes:

- this slice should not yet add tool-call explicit selection
- replacement semantics are fixed for MVP

## Slice 4: Model-visible environment context

Purpose:

- show the model which local environments exist and which one is current

Changes:

- render available/current local environments in `<environment_context>`
- keep representation compact to minimize prompt churn

Files likely touched:

- `codex-rs/core/src/context/environment_context.rs`
- `codex-rs/core/src/context/environment_context_tests.rs`
- prompt snapshot tests

Expected size:

- 150 to 250 LoC

Validation:

- environment context unit tests
- prompt snapshots

Notes:

- add only ids and current marker first; avoid verbose env details

## Slice 5: Tool-call explicit local environment override

Purpose:

- let the model choose `rocm` vs `msvc` per command without changing sticky
  thread state

Changes:

- add optional local environment selector to shell and unified exec tool args
- resolve tool explicit selection before turn/thread/default selection
- include selector in tool specs

Files likely touched:

- `codex-rs/core/src/tools/handlers/shell_spec.rs`
- `codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs`
- shell/unified exec request types
- tests for tool argument resolution

Expected size:

- 200 to 350 LoC

Validation:

- tool spec tests
- shell and unified exec integration tests

Notes:

- choose a name that does not collide with remote `environment_id` semantics,
  or explicitly document the split

## Slice 6: Generic script-derived local environments

Purpose:

- support environments that are materialized by running a command and capturing
  the resulting env map

This slice is where the design becomes the clear home for cross-platform
procedural/materialized environments.

Changes:

- add config source kind for script-derived env
- implement environment snapshot capture
- cache results

Files likely touched:

- config types
- new helper module for env derivation
- shell env resolution path

Expected size:

- 350 to 500 LoC

Validation:

- unit tests for snapshot parsing and cache invalidation
- integration tests for script-derived env use

Notes:

- this is the first slice with notable quoting and platform risk

## Slice 7: Windows `vcvars` helper

Purpose:

- make MSVC setup ergonomic and robust

This is intentionally a platform-specific helper layered on top of the generic
derived-environment model, not the core abstraction itself.

Changes:

- add `vcvars` source helper
- translate helper config into a script-derived env snapshot
- cache by script path + args + mtime

Files likely touched:

- config types
- Windows-specific env derivation helper
- tests

Expected size:

- 250 to 450 LoC

Validation:

- Windows unit tests
- best-effort integration tests guarded for environment availability

Notes:

- avoid broad platform churn
- keep helper layered on top of generic script derivation if possible

## Slice 8: Polishing and docs

Purpose:

- stabilize UX and reduce confusion with other environment concepts

Changes:

- app-server docs
- config docs
- examples
- deconflict naming with worktree `Local environments`

Expected size:

- docs only

Validation:

- doc review

## Suggested landing order

Recommended MVP:

1. Slice 0
2. Slice 1
3. Slice 2
4. Slice 3
5. Slice 4

That yields:

- config-defined named local environments
- one selected environment per thread/turn
- real command execution impact
- model-visible awareness
- explicit, stable MVP semantics around layering and protocol shape

Recommended second phase:

5. Slice 5
6. Slice 6
7. Slice 7

That yields:

- per-command switching
- procedural env derivation
- Windows MSVC support

## Testing strategy

Minimum required:

- config parsing tests
- protocol round-trip tests
- session state tests
- shell/unified exec integration tests
- environment context snapshot tests

Windows-specific:

- isolate `vcvars` behavior behind helpers
- prefer deterministic unit tests over depending on installed VS setups

## Open questions

1. Should tool-call selection reuse `environment_id` or introduce a distinct
   `local_environment_id`?
2. How should approval prompts display the selected local environment so users
   can verify command context?
