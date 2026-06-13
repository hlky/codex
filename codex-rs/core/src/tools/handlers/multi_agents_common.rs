use crate::agent::AgentStatus;
use crate::config::Config;
use crate::config::ConfigBuilder;
use crate::config::ConfigOverrides;
use crate::config::DEFAULT_MULTI_AGENT_V2_MIN_WAIT_TIMEOUT_MS;
use crate::config::HARD_MAX_MULTI_AGENT_V2_TIMEOUT_MS;
use crate::config::LoaderOverrides;
use crate::function_tool::FunctionCallError;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use codex_app_server_protocol::ConfigLayerSource;
use codex_exec_server::LOCAL_FS;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::models::BaseInstructions;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::protocol::CollabAgentRef;
use codex_protocol::protocol::CollabAgentStatusEntry;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Minimum wait timeout to prevent tight polling loops from burning CPU.
pub(crate) const MIN_WAIT_TIMEOUT_MS: i64 = DEFAULT_MULTI_AGENT_V2_MIN_WAIT_TIMEOUT_MS;
pub(crate) const DEFAULT_WAIT_TIMEOUT_MS: i64 = 30_000;
pub(crate) const MAX_WAIT_TIMEOUT_MS: i64 = HARD_MAX_MULTI_AGENT_V2_TIMEOUT_MS;

pub(crate) fn function_arguments(payload: ToolPayload) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => Ok(arguments),
        _ => Err(FunctionCallError::RespondToModel(
            "collab handler received unsupported payload".to_string(),
        )),
    }
}

pub(crate) fn tool_output_json_text<T>(value: &T, tool_name: &str) -> String
where
    T: Serialize,
{
    serde_json::to_string(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}")).to_string()
    })
}

pub(crate) fn tool_output_response_item<T>(
    call_id: &str,
    payload: &ToolPayload,
    value: &T,
    success: Option<bool>,
    tool_name: &str,
) -> ResponseInputItem
where
    T: Serialize,
{
    FunctionToolOutput::from_text(tool_output_json_text(value, tool_name), success)
        .to_response_item(call_id, payload)
}

pub(crate) fn tool_output_code_mode_result<T>(value: &T, tool_name: &str) -> JsonValue
where
    T: Serialize,
{
    serde_json::to_value(value).unwrap_or_else(|err| {
        JsonValue::String(format!("failed to serialize {tool_name} result: {err}"))
    })
}

pub(crate) fn build_wait_agent_statuses(
    statuses: &HashMap<ThreadId, AgentStatus>,
    receiver_agents: &[CollabAgentRef],
) -> Vec<CollabAgentStatusEntry> {
    if statuses.is_empty() {
        return Vec::new();
    }

    let mut entries = Vec::with_capacity(statuses.len());
    let mut seen = HashMap::with_capacity(receiver_agents.len());
    for receiver_agent in receiver_agents {
        seen.insert(receiver_agent.thread_id, ());
        if let Some(status) = statuses.get(&receiver_agent.thread_id) {
            entries.push(CollabAgentStatusEntry {
                thread_id: receiver_agent.thread_id,
                agent_nickname: receiver_agent.agent_nickname.clone(),
                agent_role: receiver_agent.agent_role.clone(),
                status: status.clone(),
            });
        }
    }

    let mut extras = statuses
        .iter()
        .filter(|(thread_id, _)| !seen.contains_key(thread_id))
        .map(|(thread_id, status)| CollabAgentStatusEntry {
            thread_id: *thread_id,
            agent_nickname: None,
            agent_role: None,
            status: status.clone(),
        })
        .collect::<Vec<_>>();
    extras.sort_by_key(|entry| entry.thread_id.to_string());
    entries.extend(extras);
    entries
}

pub(crate) fn collab_spawn_error(err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::UnsupportedOperation(message) if message == "thread manager dropped" => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        CodexErr::UnsupportedOperation(message) => FunctionCallError::RespondToModel(message),
        err => FunctionCallError::RespondToModel(format!("collab spawn failed: {err}")),
    }
}

pub(crate) fn collab_agent_error(agent_id: ThreadId, err: CodexErr) -> FunctionCallError {
    match err {
        CodexErr::ThreadNotFound(id) => {
            FunctionCallError::RespondToModel(format!("agent with id {id} not found"))
        }
        CodexErr::InternalAgentDied => {
            FunctionCallError::RespondToModel(format!("agent with id {agent_id} is closed"))
        }
        CodexErr::UnsupportedOperation(_) => {
            FunctionCallError::RespondToModel("collab manager unavailable".to_string())
        }
        err => FunctionCallError::RespondToModel(format!("collab tool failed: {err}")),
    }
}

pub(crate) fn thread_spawn_source(
    parent_thread_id: ThreadId,
    parent_session_source: &SessionSource,
    depth: i32,
    agent_role: Option<&str>,
    task_name: Option<String>,
) -> Result<SessionSource, FunctionCallError> {
    let agent_path = task_name
        .as_deref()
        .map(|task_name| {
            parent_session_source
                .get_agent_path()
                .unwrap_or_else(AgentPath::root)
                .join(task_name)
                .map_err(FunctionCallError::RespondToModel)
        })
        .transpose()?;
    Ok(SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
        parent_thread_id,
        depth,
        agent_path,
        agent_nickname: None,
        agent_role: agent_role.map(str::to_string),
    }))
}

pub(crate) fn parse_collab_input(
    message: Option<String>,
    items: Option<Vec<UserInput>>,
) -> Result<Op, FunctionCallError> {
    match (message, items) {
        (Some(_), Some(_)) => Err(FunctionCallError::RespondToModel(
            "Provide either message or items, but not both".to_string(),
        )),
        (None, None) => Err(FunctionCallError::RespondToModel(
            "Provide one of: message or items".to_string(),
        )),
        (Some(message), None) => {
            if message.trim().is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Empty message can't be sent to an agent".to_string(),
                ));
            }
            Ok(vec![UserInput::Text {
                text: message,
                text_elements: Vec::new(),
            }]
            .into())
        }
        (None, Some(items)) => {
            if items.is_empty() {
                return Err(FunctionCallError::RespondToModel(
                    "Items can't be empty".to_string(),
                ));
            }
            Ok(items.into())
        }
    }
}

/// Builds the base config snapshot for a newly spawned sub-agent.
///
/// The returned config starts from the parent's effective config and then refreshes the
/// runtime-owned fields carried on `turn`, including model selection, reasoning settings,
/// approval policy, sandbox, and cwd. Role-specific overrides are layered after this step;
/// skipping this helper and cloning stale config state directly can send the child agent out with
/// the wrong provider or runtime policy.
pub(crate) async fn build_agent_spawn_config(
    base_instructions: &BaseInstructions,
    turn: &TurnContext,
) -> Result<Config, FunctionCallError> {
    let mut config = build_spawn_agent_shared_config(turn).await?;
    config.base_instructions = Some(base_instructions.text.clone());
    Ok(config)
}

pub(crate) fn build_agent_resume_config(turn: &TurnContext) -> Result<Config, FunctionCallError> {
    let mut config = build_agent_shared_config((*turn.config).clone(), turn)?;
    // For resume, keep base instructions sourced from rollout/session metadata.
    config.base_instructions = None;
    Ok(config)
}

async fn build_spawn_agent_shared_config(turn: &TurnContext) -> Result<Config, FunctionCallError> {
    #[allow(deprecated)]
    let turn_cwd = turn.cwd.clone();
    if !spawn_cwd_requires_config_refresh(turn.config.as_ref(), &turn_cwd) {
        return build_agent_shared_config((*turn.config).clone(), turn);
    }
    let refreshed_config = build_refreshed_spawn_config(turn.config.as_ref(), &turn_cwd).await?;
    build_agent_shared_config(refreshed_config, turn)
}

fn spawn_cwd_requires_config_refresh(
    base_config: &Config,
    cwd: &codex_utils_absolute_path::AbsolutePathBuf,
) -> bool {
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

fn build_agent_shared_config(
    mut config: Config,
    turn: &TurnContext,
) -> Result<Config, FunctionCallError> {
    config.agent_roles = turn.config.agent_roles.clone();
    config.user_instructions = turn.config.user_instructions.clone();
    config.model = Some(turn.model_info.slug.clone());
    config.model_provider_id = turn.config.model_provider_id.clone();
    config.model_provider = turn.provider.info().clone();
    config.model_reasoning_effort = turn
        .reasoning_effort
        .clone()
        .or_else(|| turn.model_info.default_reasoning_level.clone());
    config.model_reasoning_summary = Some(turn.reasoning_summary);
    config.developer_instructions = turn.developer_instructions.clone();
    config.compact_prompt = turn.compact_prompt.clone();
    apply_spawn_agent_runtime_overrides(&mut config, turn)?;

    Ok(config)
}

async fn build_refreshed_spawn_config(
    base_config: &Config,
    cwd: &codex_utils_absolute_path::AbsolutePathBuf,
) -> Result<Config, FunctionCallError> {
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
                "failed to load subagent config for {}: {err}",
                cwd.display()
            ))
        })?;

    let mut config = base_config
        .rebuild_preserving_session_layers(&refreshed_config)
        .await
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to rebuild subagent config for {}: {err}",
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

    Ok(config)
}

pub(crate) fn reject_full_fork_spawn_overrides(
    agent_type: Option<&str>,
    model: Option<&str>,
    reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), FunctionCallError> {
    if agent_type.is_some() || model.is_some() || reasoning_effort.is_some() {
        return Err(FunctionCallError::RespondToModel(
            "Full-history forked agents inherit the parent agent type, model, and reasoning effort; omit agent_type, model, and reasoning_effort, or spawn without a full-history fork.".to_string(),
        ));
    }
    Ok(())
}

/// Copies runtime-only turn state onto a child config before it is handed to `AgentControl`.
///
/// These values are chosen by the live turn rather than persisted config, so leaving them stale
/// can make a child agent disagree with its parent about approval policy, cwd, or sandboxing.
pub(crate) fn apply_spawn_agent_runtime_overrides(
    config: &mut Config,
    turn: &TurnContext,
) -> Result<(), FunctionCallError> {
    config
        .permissions
        .approval_policy
        .set(turn.approval_policy.value())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("approval_policy is invalid: {err}"))
        })?;
    config.approvals_reviewer = turn.config.approvals_reviewer;
    config.permissions.shell_environment_policy = turn.shell_environment_policy.clone();
    config.codex_linux_sandbox_exe = turn.codex_linux_sandbox_exe.clone();
    #[allow(deprecated)]
    let turn_cwd = turn.cwd.clone();
    config.cwd = turn_cwd;
    config
        .permissions
        .set_permission_profile(turn.permission_profile())
        .map_err(|err| {
            FunctionCallError::RespondToModel(format!("permission_profile is invalid: {err}"))
        })?;
    Ok(())
}

pub(crate) async fn apply_requested_spawn_agent_model_overrides(
    session: &Session,
    turn: &TurnContext,
    config: &mut Config,
    requested_model: Option<&str>,
    requested_reasoning_effort: Option<ReasoningEffort>,
) -> Result<(), FunctionCallError> {
    if requested_model.is_none() && requested_reasoning_effort.is_none() {
        return Ok(());
    }

    if let Some(requested_model) = requested_model {
        let available_models = session
            .services
            .models_manager
            .list_models(RefreshStrategy::Offline)
            .await;
        let selected_model_name = find_spawn_agent_model_name(&available_models, requested_model)?;
        let selected_model_info = session
            .services
            .models_manager
            .get_model_info(&selected_model_name, &config.to_models_manager_config())
            .await;

        config.model = Some(selected_model_name.clone());
        if let Some(reasoning_effort) = requested_reasoning_effort {
            validate_spawn_agent_reasoning_effort(
                &selected_model_name,
                &selected_model_info.supported_reasoning_levels,
                &reasoning_effort,
            )?;
            config.model_reasoning_effort = Some(reasoning_effort);
        } else {
            config.model_reasoning_effort = selected_model_info.default_reasoning_level;
        }

        return Ok(());
    }

    if let Some(reasoning_effort) = requested_reasoning_effort {
        validate_spawn_agent_reasoning_effort(
            &turn.model_info.slug,
            &turn.model_info.supported_reasoning_levels,
            &reasoning_effort,
        )?;
        config.model_reasoning_effort = Some(reasoning_effort);
    }

    Ok(())
}

pub(crate) async fn apply_spawn_agent_service_tier(
    session: &Session,
    config: &mut Config,
    parent_service_tier: Option<&str>,
    requested_service_tier: Option<&str>,
) -> Result<(), FunctionCallError> {
    let candidate_service_tiers = [
        config.service_tier.clone(),
        requested_service_tier.map(str::to_string),
        parent_service_tier.map(str::to_string),
    ];
    if candidate_service_tiers.iter().all(Option::is_none) {
        config.service_tier = None;
        return Ok(());
    }

    let model = config.model.clone().ok_or_else(|| {
        FunctionCallError::RespondToModel(
            "spawn_agent could not resolve the child model for service tier validation".to_string(),
        )
    })?;
    let model_info = session
        .services
        .models_manager
        .get_model_info(model.as_str(), &config.to_models_manager_config())
        .await;

    if let Some(requested_service_tier) = requested_service_tier
        && !model_info.supports_service_tier(requested_service_tier)
    {
        let supported_service_tiers = if model_info.service_tiers.is_empty() {
            "none".to_string()
        } else {
            model_info
                .service_tiers
                .iter()
                .map(|tier| tier.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(FunctionCallError::RespondToModel(format!(
            "Service tier `{requested_service_tier}` is not supported for model `{model}`. Supported service tiers: {supported_service_tiers}"
        )));
    }

    config.service_tier =
        candidate_service_tiers
            .into_iter()
            .flatten()
            .find(|candidate_service_tier| {
                model_info.supports_service_tier(candidate_service_tier.as_str())
            });
    Ok(())
}

fn find_spawn_agent_model_name(
    available_models: &[codex_protocol::openai_models::ModelPreset],
    requested_model: &str,
) -> Result<String, FunctionCallError> {
    available_models
        .iter()
        .find(|model| model.model == requested_model)
        .map(|model| model.model.clone())
        .ok_or_else(|| {
            let available = available_models
                .iter()
                .map(|model| model.model.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            FunctionCallError::RespondToModel(format!(
                "Unknown model `{requested_model}` for spawn_agent. Available models: {available}"
            ))
        })
}

fn validate_spawn_agent_reasoning_effort(
    model: &str,
    supported_reasoning_levels: &[ReasoningEffortPreset],
    requested_reasoning_effort: &ReasoningEffort,
) -> Result<(), FunctionCallError> {
    if supported_reasoning_levels
        .iter()
        .any(|preset| &preset.effort == requested_reasoning_effort)
    {
        return Ok(());
    }

    let supported = supported_reasoning_levels
        .iter()
        .map(|preset| preset.effort.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(FunctionCallError::RespondToModel(format!(
        "Reasoning effort `{requested_reasoning_effort}` is not supported for model `{model}`. Supported reasoning efforts: {supported}"
    )))
}
