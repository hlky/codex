use codex_config::types::LocalEnvironmentConfig;
use codex_config::types::LocalEnvironmentScriptConfig;
use codex_config::types::LocalEnvironmentScriptShell;
use codex_config::types::LocalEnvironmentSourceConfig;
use codex_config::types::LocalEnvironmentToml;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;

pub(super) fn resolve_local_environment_configs(
    local_environments: BTreeMap<String, LocalEnvironmentToml>,
) -> io::Result<BTreeMap<String, LocalEnvironmentConfig>> {
    local_environments
        .into_iter()
        .map(|(name, environment)| {
            resolve_local_environment_config(name.as_str(), environment)
                .map(|config| (name, config))
        })
        .collect()
}

fn resolve_local_environment_config(
    name: &str,
    environment: LocalEnvironmentToml,
) -> io::Result<LocalEnvironmentConfig> {
    let LocalEnvironmentToml {
        description,
        shell_environment_policy,
        script,
    } = environment;
    match (shell_environment_policy, script) {
        (Some(shell_environment_policy), None) => Ok(LocalEnvironmentConfig {
            description,
            source: LocalEnvironmentSourceConfig::Static(shell_environment_policy.into()),
        }),
        (None, Some(script)) => Ok(LocalEnvironmentConfig {
            description,
            source: LocalEnvironmentSourceConfig::Script(LocalEnvironmentScriptConfig {
                shell: script
                    .shell
                    .unwrap_or_else(|| default_script_shell(script.script.as_path())),
                script: script.script,
                args: script.args,
                cwd: script.cwd,
            }),
        }),
        (Some(_), Some(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "local_environments.{name} must define exactly one source; found both `shell_environment_policy` and `script`"
            ),
        )),
        (None, None) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "local_environments.{name} must define exactly one source: `shell_environment_policy` or `script`"
            ),
        )),
    }
}

fn default_script_shell(script: &std::path::Path) -> LocalEnvironmentScriptShell {
    match script.extension().and_then(OsStr::to_str) {
        Some(ext) if ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd") => {
            LocalEnvironmentScriptShell::Cmd
        }
        Some(ext) if ext.eq_ignore_ascii_case("ps1") => LocalEnvironmentScriptShell::PowerShell,
        _ => LocalEnvironmentScriptShell::Sh,
    }
}
