use crate::config::Config;
use crate::config::ConfigBuilder;
use crate::config::ConfigOverrides;
use crate::config::LoaderOverrides;
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_app_server_protocol::ConfigLayerSource;
use codex_exec_server::LOCAL_FS;
use codex_protocol::config_types::ShellEnvironmentPolicy;
use codex_utils_absolute_path::AbsolutePathBuf;

pub(crate) async fn refreshed_config_for_cwd(
    base_config: &Config,
    cwd: &AbsolutePathBuf,
) -> Result<Option<Config>, FunctionCallError> {
    if !cwd_requires_config_refresh(base_config, cwd) {
        return Ok(None);
    }

    let loader_overrides = LoaderOverrides {
        user_config_path: base_config
            .config_layer_stack
            .get_user_config_file()
            .cloned(),
        user_config_profile: base_config
            .config_layer_stack
            .get_active_user_layer()
            .and_then(|layer| match &layer.name {
                ConfigLayerSource::User {
                    profile: Some(profile),
                    ..
                } => profile.parse().ok(),
                _ => None,
            }),
        ..Default::default()
    };

    let refreshed_config = ConfigBuilder::default()
        .codex_home(base_config.codex_home.to_path_buf())
        .loader_overrides(loader_overrides)
        .fallback_cwd(Some(cwd.to_path_buf()))
        .build()
        .await
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to load project config for {}: {err}",
                cwd.display()
            ))
        })?;

    let mut config = base_config
        .rebuild_preserving_session_layers(&refreshed_config)
        .await
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to rebuild project config for {}: {err}",
                cwd.display()
            ))
        })?;

    if let (Some(user_config_file), Some(active_user_layer)) = (
        base_config.config_layer_stack.get_user_config_file(),
        base_config.config_layer_stack.get_active_user_layer(),
    ) {
        let config_layer_stack = config
            .config_layer_stack
            .with_user_config(user_config_file, active_user_layer.config.clone());
        let config_toml = config_layer_stack
            .effective_config()
            .try_into()
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
        let default_zsh_path = config
            .zsh_path
            .clone()
            .map(codex_utils_absolute_path::AbsolutePathBuf::try_from)
            .transpose()
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
        config = Config::load_config_with_layer_stack(
            LOCAL_FS.as_ref(),
            config_toml,
            ConfigOverrides {
                cwd: Some(base_config.cwd.to_path_buf()),
                default_zsh_path,
                ..Default::default()
            },
            config.codex_home.clone(),
            config_layer_stack,
        )
        .await
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
    }

    config.features = base_config.features.clone();
    config.notify = base_config.notify.clone();
    config.bypass_hook_trust = base_config.bypass_hook_trust;

    Ok(Some(config))
}

pub(crate) async fn shell_environment_policy_for_command_cwd(
    session: &Session,
    turn: &TurnContext,
    cwd: &AbsolutePathBuf,
) -> Result<ShellEnvironmentPolicy, FunctionCallError> {
    let Some(config) = refreshed_config_for_cwd(turn.config.as_ref(), cwd).await? else {
        return Ok(turn.shell_environment_policy.clone());
    };

    let selected_local_environment = turn
        .local_environment
        .as_ref()
        .filter(|name| config.local_environments.contains_key(*name))
        .cloned()
        .or(config.default_local_environment.clone());

    if let Some(selected_local_environment) = selected_local_environment.as_ref()
        && let Some(local_environment) = config.local_environments.get(selected_local_environment)
    {
        return session
            .resolve_local_environment_shell_policy(
            selected_local_environment,
            local_environment,
        )
        .await
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to resolve local environment `{selected_local_environment}` for {}: {err}",
                cwd.display()
            ))
        });
    }

    Ok(config.permissions.shell_environment_policy.clone())
}

fn cwd_requires_config_refresh(base_config: &Config, cwd: &AbsolutePathBuf) -> bool {
    if *cwd == base_config.cwd {
        return false;
    }

    !base_config
        .config_layer_stack
        .get_layers(
            codex_config::ConfigLayerStackOrdering::LowestPrecedenceFirst,
            /*include_disabled*/ true,
        )
        .into_iter()
        .filter_map(|layer| match &layer.name {
            ConfigLayerSource::Project { dot_codex_folder } => dot_codex_folder.parent(),
            _ => None,
        })
        .any(|project_root| cwd.as_path().starts_with(project_root))
}

#[cfg(test)]
#[path = "workdir_shell_environment_tests.rs"]
mod tests;
