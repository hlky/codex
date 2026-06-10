# Named Local Environments

This directory captures a proposed implementation plan for first-class named
local environment overlays in Codex.

Problem statement:

- Codex today supports one effective `shell_environment_policy` per session.
- Profiles can switch config only at launch time.
- Experimental exec-server environments model execution backends, not local
  toolchain overlays on the same machine.

Target capability:

- Define multiple named local environments in config.
- Select one as the sticky thread default.
- Override it per turn.
- Optionally select it per shell-like tool call.
- Keep one session, one repo, one filesystem view, and one model context.

Cross-platform framing:

- This feature is for any project that needs more than one local command
  environment within the same session.
- Windows MSVC / ROCm is the clearest motivating example, but the same
  abstraction should also fit cases such as Python venv vs system env,
  minimal vs full dev env, and later generic derived environments on macOS
  and Linux.

MVP decisions:

- MVP supports static named local environments only.
- MVP config is user-level config only; project-local config is out of scope.
- Local environment selection is separate from existing remote environment
  selection.
- Selecting a local environment replaces the session-level
  `shell_environment_policy` for shell-like execution, except runtime-owned vars
  that Codex must always inject afterward.
- MVP includes app-server-visible thread/turn selectors behind experimental API
  where needed.

Primary motivating workflow:

- `rocm` for Python / ROCm / GPU tooling
- `msvc` for CPU / native builds via `vcvars*.bat`
- switching between them within the same active thread

Other examples this should conceptually support:

- Python virtualenv vs system environment
- minimal runtime env vs full development env
- later follow-up support for generic script/materialized environments

Important MVP limitation:

- the real MSVC `vcvars*.bat` procedural setup is a follow-up slice
- MVP only covers the static-policy form of the problem

Documents:

- `architecture-notes.md`: code touchpoints, data model, and design choices
- `implementation-plan.md`: bounded slices and suggested landing order
