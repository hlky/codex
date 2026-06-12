use anyhow::Result;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use serde_json::json;
use std::collections::BTreeMap;
use tempfile::tempdir;

use codex_config::types::LocalEnvironmentConfig;
use codex_config::types::LocalEnvironmentScriptConfig;
use codex_config::types::LocalEnvironmentScriptShell;
use codex_config::types::LocalEnvironmentSourceConfig;
use codex_features::Feature;
use codex_utils_absolute_path::AbsolutePathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_uses_selected_dynamic_local_environment() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let temp_dir = tempdir()?;
    let script_path = write_dynamic_test_script(temp_dir.path(), "shell-dynamic");
    let harness = local_environment_harness(|builder| {
        builder.with_config(move |config| {
            config.local_environments = BTreeMap::from([(
                "dynamic".to_string(),
                LocalEnvironmentConfig {
                    description: Some("Dynamic script".to_string()),
                    source: LocalEnvironmentSourceConfig::Script(LocalEnvironmentScriptConfig {
                        script: AbsolutePathBuf::from_absolute_path(&script_path)
                            .expect("script path should be absolute"),
                        shell: test_script_shell(),
                        args: Vec::new(),
                        cwd: None,
                    }),
                },
            )]);
            config.default_local_environment = Some("dynamic".to_string());
        })
    })
    .await?;

    let call_id = "dynamic-local-environment-shell";
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    call_id,
                    "shell_command",
                    &json!({
                        "command": shell_print_command(),
                        "timeout_ms": 2_000,
                    })
                    .to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness
        .submit("print the dynamic local environment value")
        .await?;

    let output = harness.function_call_stdout(call_id).await;
    assert!(
        output.contains("shell-dynamic"),
        "unexpected shell output: {output}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unified_exec_uses_selected_dynamic_local_environment() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let temp_dir = tempdir()?;
    let script_path = write_dynamic_test_script(temp_dir.path(), "exec-dynamic");
    let harness = local_environment_harness(|builder| {
        builder.with_config(move |config| {
            config.use_experimental_unified_exec_tool = true;
            config
                .features
                .enable(Feature::UnifiedExec)
                .expect("test config should allow feature update");
            config.local_environments = BTreeMap::from([(
                "dynamic".to_string(),
                LocalEnvironmentConfig {
                    description: Some("Dynamic script".to_string()),
                    source: LocalEnvironmentSourceConfig::Script(LocalEnvironmentScriptConfig {
                        script: AbsolutePathBuf::from_absolute_path(&script_path)
                            .expect("script path should be absolute"),
                        shell: test_script_shell(),
                        args: Vec::new(),
                        cwd: None,
                    }),
                },
            )]);
            config.default_local_environment = Some("dynamic".to_string());
        })
    })
    .await?;

    let call_id = "dynamic-local-environment-exec";
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    call_id,
                    "exec_command",
                    &json!({
                        "cmd": shell_print_command(),
                        "yield_time_ms": 1_000,
                    })
                    .to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness
        .submit("print the dynamic local environment value with unified exec")
        .await?;

    let output = harness.function_call_stdout(call_id).await;
    assert!(
        output.contains("exec-dynamic"),
        "unexpected unified exec output: {output}"
    );

    Ok(())
}

async fn local_environment_harness(
    configure: impl FnOnce(TestCodexBuilder) -> TestCodexBuilder,
) -> Result<TestCodexHarness> {
    TestCodexHarness::with_builder(configure(test_codex())).await
}

#[cfg(windows)]
fn write_dynamic_test_script(dir: &std::path::Path, value: &str) -> std::path::PathBuf {
    let script_path = dir.join("dynamic-env.bat");
    std::fs::write(
        &script_path,
        format!("@echo off\r\nset CODEX_DYNAMIC_SHELL={value}\r\n"),
    )
    .expect("write windows test script");
    script_path
}

#[cfg(not(windows))]
fn write_dynamic_test_script(dir: &std::path::Path, value: &str) -> std::path::PathBuf {
    let script_path = dir.join("dynamic-env.sh");
    std::fs::write(
        &script_path,
        format!("export CODEX_DYNAMIC_SHELL={value}\n"),
    )
    .expect("write unix test script");
    script_path
}

#[cfg(windows)]
fn test_script_shell() -> LocalEnvironmentScriptShell {
    LocalEnvironmentScriptShell::Cmd
}

#[cfg(not(windows))]
fn test_script_shell() -> LocalEnvironmentScriptShell {
    LocalEnvironmentScriptShell::Sh
}

#[cfg(windows)]
fn shell_print_command() -> &'static str {
    "cmd /d /s /c echo %CODEX_DYNAMIC_SHELL%"
}

#[cfg(not(windows))]
fn shell_print_command() -> &'static str {
    "printf '%s' \"$CODEX_DYNAMIC_SHELL\""
}
