use super::*;
use crate::session::tests::make_session_and_context;
use crate::session::tests::make_session_configuration_for_tests;
use codex_config::types::LocalEnvironmentConfig;
use codex_protocol::config_types::ShellEnvironmentPolicy;
use codex_protocol::config_types::ShellEnvironmentPolicyInherit;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::test]
async fn resolve_turn_local_environment_state_prefers_turn_override() {
    let mut session_configuration = make_session_configuration_for_tests().await;
    let mut config = (*session_configuration.original_config_do_not_use).clone();
    config.local_environments = BTreeMap::from([
        (
            "msvc".to_string(),
            LocalEnvironmentConfig {
                description: Some("MSVC toolchain".to_string()),
                shell_environment_policy: ShellEnvironmentPolicy {
                    inherit: ShellEnvironmentPolicyInherit::None,
                    ignore_default_excludes: true,
                    include_only: Vec::new(),
                    exclude: Vec::new(),
                    r#set: HashMap::from([("CC".to_string(), "cl.exe".to_string())]),
                    use_profile: false,
                },
            },
        ),
        (
            "rocm".to_string(),
            LocalEnvironmentConfig {
                description: Some("ROCm toolchain".to_string()),
                shell_environment_policy: ShellEnvironmentPolicy {
                    inherit: ShellEnvironmentPolicyInherit::Core,
                    ignore_default_excludes: true,
                    include_only: Vec::new(),
                    exclude: Vec::new(),
                    r#set: HashMap::from([("ACCELERATOR".to_string(), "rocm".to_string())]),
                    use_profile: false,
                },
            },
        ),
    ]);
    session_configuration.original_config_do_not_use = Arc::new(config);

    let state = resolve_turn_local_environment_state(
        session_configuration.original_config_do_not_use.as_ref(),
        Some("msvc".to_string()),
        Some(Some("rocm".to_string())),
    )
    .expect("turn override should resolve");

    assert_eq!(
        state,
        TurnLocalEnvironmentState {
            available: vec!["msvc".to_string(), "rocm".to_string()],
            selected: Some("rocm".to_string()),
        }
    );
}

#[tokio::test]
async fn new_turn_context_from_configuration_applies_selected_local_environment_shell_policy() {
    let (session, initial_turn_context) = make_session_and_context().await;
    let mut session_configuration = make_session_configuration_for_tests().await;
    let mut config = (*session_configuration.original_config_do_not_use).clone();
    config.local_environments = BTreeMap::from([
        (
            "msvc".to_string(),
            LocalEnvironmentConfig {
                description: Some("MSVC toolchain".to_string()),
                shell_environment_policy: ShellEnvironmentPolicy {
                    inherit: ShellEnvironmentPolicyInherit::None,
                    ignore_default_excludes: true,
                    include_only: Vec::new(),
                    exclude: Vec::new(),
                    r#set: HashMap::from([("CC".to_string(), "cl.exe".to_string())]),
                    use_profile: false,
                },
            },
        ),
        (
            "rocm".to_string(),
            LocalEnvironmentConfig {
                description: Some("ROCm toolchain".to_string()),
                shell_environment_policy: ShellEnvironmentPolicy {
                    inherit: ShellEnvironmentPolicyInherit::Core,
                    ignore_default_excludes: true,
                    include_only: Vec::new(),
                    exclude: Vec::new(),
                    r#set: HashMap::from([("ACCELERATOR".to_string(), "rocm".to_string())]),
                    use_profile: false,
                },
            },
        ),
    ]);
    session_configuration.local_environment = Some("msvc".to_string());
    session_configuration.original_config_do_not_use = Arc::new(config);

    let local_environment_state = resolve_turn_local_environment_state(
        session_configuration.original_config_do_not_use.as_ref(),
        session_configuration.local_environment.clone(),
        Some(Some("rocm".to_string())),
    )
    .expect("turn local environment should resolve");

    let turn_context = session
        .new_turn_context_from_configuration(
            "turn-local-environment".to_string(),
            session_configuration,
            initial_turn_context.environments.clone(),
            TurnBuildOptions {
                final_output_json_schema: None,
                turn_local_environment: Some(Some("rocm".to_string())),
                local_environment_state,
                multi_agent_runtime: TurnMultiAgentRuntime::Preview,
            },
        )
        .await;

    assert_eq!(turn_context.local_environment.as_deref(), Some("rocm"));
    assert_eq!(
        turn_context.available_local_environments,
        vec!["msvc".to_string(), "rocm".to_string()]
    );
    assert_eq!(
        turn_context.shell_environment_policy.inherit,
        ShellEnvironmentPolicyInherit::Core
    );
    assert_eq!(
        turn_context.shell_environment_policy.r#set,
        HashMap::from([("ACCELERATOR".to_string(), "rocm".to_string())])
    );
}
