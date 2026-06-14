use std::time::Duration;

use anyhow::Result;
use codex_features::Feature;
use core_test_support::assert_regex_match;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::skip_if_windows;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::TestCodexBuilder;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;
use test_case::test_case;
use tokio::time::sleep;

#[cfg(windows)]
const DEFAULT_SHELL_TIMEOUT_MS: i64 = 7_000;
#[cfg(not(windows))]
const DEFAULT_SHELL_TIMEOUT_MS: i64 = 2_000;

#[cfg(windows)]
const MEDIUM_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(windows))]
const MEDIUM_TIMEOUT: Duration = Duration::from_secs(5);

fn shell_responses_with_timeout(
    call_id: &str,
    command: &str,
    login: Option<bool>,
    timeout_ms: i64,
) -> Vec<String> {
    let args = json!({
        "command": command,
        "timeout_ms": timeout_ms,
        "login": login,
    });

    #[allow(clippy::expect_used)]
    let arguments = serde_json::to_string(&args).expect("serialize shell command arguments");

    vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call(call_id, "shell_command", &arguments),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-2"),
        ]),
    ]
}

fn shell_responses(call_id: &str, command: &str, login: Option<bool>) -> Vec<String> {
    shell_responses_with_timeout(call_id, command, login, DEFAULT_SHELL_TIMEOUT_MS)
}

async fn shell_command_harness_with(
    configure: impl FnOnce(TestCodexBuilder) -> TestCodexBuilder,
) -> Result<TestCodexHarness> {
    let builder = configure(test_codex());
    TestCodexHarness::with_builder(builder).await
}

async fn mount_shell_responses(
    harness: &TestCodexHarness,
    call_id: &str,
    command: &str,
    login: Option<bool>,
) {
    mount_sse_sequence(harness.server(), shell_responses(call_id, command, login)).await;
}

async fn mount_shell_responses_with_timeout(
    harness: &TestCodexHarness,
    call_id: &str,
    command: &str,
    login: Option<bool>,
    timeout: Duration,
) {
    mount_sse_sequence(
        harness.server(),
        shell_responses_with_timeout(call_id, command, login, timeout.as_millis() as i64),
    )
    .await;
}

fn assert_shell_command_output(output: &str, expected: &str) -> Result<()> {
    let normalized_output = output
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end_matches('\n')
        .to_string();

    let expected_pattern = format!(
        r"(?s)^Exit code: 0\nWall time: [0-9]+(?:\.[0-9]+)? seconds\nOutput:\n{expected}\n?$"
    );

    assert_regex_match(&expected_pattern, &normalized_output);
    Ok(())
}

fn env_print_command() -> &'static str {
    if cfg!(windows) {
        "Write-Output $env:DINOML_CACHE_DIR"
    } else {
        "printf '%s\\n' \"$DINOML_CACHE_DIR\""
    }
}

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .is_ok_and(|body| body.to_string().contains(text))
}

fn has_function_call_output(request: &wiremock::Request, call_id: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(&request.body).is_ok_and(|body| {
        body.get("input")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("type").and_then(serde_json::Value::as_str)
                        == Some("function_call_output")
                        && item.get("call_id").and_then(serde_json::Value::as_str) == Some(call_id)
                })
            })
    })
}

async fn wait_for_matching_request<F>(
    mock: &ResponseMock,
    label: &str,
    mut predicate: F,
) -> Result<ResponsesRequest>
where
    F: FnMut(&ResponsesRequest) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(request) = mock
            .requests()
            .into_iter()
            .find(|request| predicate(request))
        {
            return Ok(request);
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for {label}");
        }
        sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_works() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call";
    mount_shell_responses(
        &harness,
        call_id,
        "echo 'hello, world'",
        /*login*/ None,
    )
    .await;
    harness.submit("run the echo command").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "hello, world")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_with_login() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call-login-true";
    mount_shell_responses(&harness, call_id, "echo 'hello, world'", Some(true)).await;
    harness.submit("run the echo command with login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "hello, world")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_without_login() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call-login-false";
    mount_shell_responses(&harness, call_id, "echo 'hello, world'", Some(false)).await;
    harness.submit("run the echo command without login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "hello, world")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_line_output_with_login() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call-first-extra-login";
    mount_shell_responses(
        &harness,
        call_id,
        "echo 'first line\nsecond line'",
        Some(true),
    )
    .await;
    harness.submit("run the command with login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "first line\nsecond line")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pipe_output_with_login() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_windows!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call-second-extra-no-login";
    mount_shell_responses(
        &harness,
        call_id,
        "echo 'hello, world' | cat",
        /*login*/ None,
    )
    .await;
    harness.submit("run the command without login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "hello, world")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pipe_output_without_login() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    skip_if_windows!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;

    let call_id = "shell-command-call-third-extra-login-false";
    mount_shell_responses(&harness, call_id, "echo 'hello, world' | cat", Some(false)).await;
    harness.submit("run the command without login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "hello, world")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_times_out_with_timeout_ms() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.4")).await?;
    let call_id = "shell-command-timeout";
    let command = if cfg!(windows) {
        "timeout /t 5"
    } else {
        "sleep 5"
    };
    mount_shell_responses_with_timeout(
        &harness,
        call_id,
        command,
        /*login*/ None,
        Duration::from_millis(200),
    )
    .await;
    harness
        .submit("run a long command with a short timeout")
        .await?;

    let output = harness.function_call_stdout(call_id).await;
    let normalized_output = output
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end_matches('\n')
        .to_string();
    let expected_pattern = r"(?s)^Exit code: 124\nWall time: [0-9]+(?:\.[0-9]+)? seconds\nOutput:\ncommand timed out after [0-9]+ milliseconds\n?$";
    assert_regex_match(expected_pattern, &normalized_output);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_command_uses_project_shell_environment_for_workdir() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let home = Arc::new(TempDir::new()?);
    let harness = shell_command_harness_with({
        let home = Arc::clone(&home);
        move |builder| {
            builder.with_home(Arc::clone(&home)).with_workspace_setup({
                let home = Arc::clone(&home);
                move |cwd, _fs| async move {
                    let project_root = cwd.join("worktree-shell-config");
                    std::fs::create_dir_all(project_root.join(".codex"))?;
                    std::fs::write(project_root.join(".git"), "gitdir: here\n")?;
                    std::fs::write(
                        project_root.join(".codex/config.toml"),
                        r#"
[shell_environment_policy]
inherit = "all"
set = { DINOML_CACHE_DIR = "worktree-cache" }
"#,
                    )?;
                    std::fs::write(
                        home.path().join("config.toml"),
                        format!(
                            r#"[projects.{:?}]
trust_level = "trusted"
"#,
                            project_root.display().to_string()
                        ),
                    )?;
                    Ok(())
                }
            })
        }
    })
    .await?;

    let call_id = "shell-command-workdir-project-config";
    let workdir = harness.path("worktree-shell-config");
    let arguments = serde_json::to_string(&json!({
        "command": env_print_command(),
        "workdir": workdir.to_string_lossy().to_string(),
        "timeout_ms": DEFAULT_SHELL_TIMEOUT_MS,
    }))?;
    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &arguments),
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
        .submit("run the shell command in the worktree")
        .await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "worktree-cache")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawned_subagent_shell_command_uses_project_shell_environment_for_workdir() -> Result<()> {
    skip_if_no_network!(Ok(()));

    const PARENT_PROMPT: &str = "spawn a worker for the project worktree";
    const CHILD_PROMPT: &str = "print the worktree cache env";
    const SPAWN_CALL_ID: &str = "spawn-child-call";
    const CHILD_SHELL_CALL_ID: &str = "child-shell-call";
    const CHILD_TASK_NAME: &str = "worker";

    let server = start_mock_server().await;
    let home = Arc::new(TempDir::new()?);
    let project_root = home.path().join("spawn-worktree-shell-config");
    let project_root_for_setup = project_root.clone();
    let spawn_args = serde_json::to_string(&json!({
        "message": CHILD_PROMPT,
        "task_name": CHILD_TASK_NAME,
        "workdir": project_root.display().to_string(),
    }))?;
    let child_shell_args = serde_json::to_string(&json!({
        "command": env_print_command(),
        "timeout_ms": DEFAULT_SHELL_TIMEOUT_MS,
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, PARENT_PROMPT),
        sse(vec![
            ev_response_created("parent-response-1"),
            ev_function_call(SPAWN_CALL_ID, "spawn_agent", &spawn_args),
            ev_completed("parent-response-1"),
        ]),
    )
    .await;
    let _child_request = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, CHILD_PROMPT) && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("child-response-1"),
            ev_function_call(CHILD_SHELL_CALL_ID, "shell_command", &child_shell_args),
            ev_completed("child-response-1"),
        ]),
    )
    .await;
    let child_result = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| has_function_call_output(request, CHILD_SHELL_CALL_ID),
        sse(vec![
            ev_response_created("child-response-2"),
            ev_assistant_message("child-message-2", "child done"),
            ev_completed("child-response-2"),
        ]),
    )
    .await;
    let _parent_followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| has_function_call_output(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("parent-response-2"),
            ev_assistant_message("parent-message-2", "parent done"),
            ev_completed("parent-response-2"),
        ]),
    )
    .await;

    let builder = test_codex()
        .with_home(Arc::clone(&home))
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("test config should allow feature update");
        });
    let test: TestCodex = builder
        .with_workspace_setup({
            let home = Arc::clone(&home);
            let project_root = project_root_for_setup.clone();
            move |cwd, _fs| async move {
                std::fs::create_dir_all(project_root.join(".codex"))?;
                std::fs::write(project_root.join(".git"), "gitdir: here\n")?;
                std::fs::write(
                    project_root.join(".codex/config.toml"),
                    r#"
[shell_environment_policy]
inherit = "all"
set = { DINOML_CACHE_DIR = "worktree-cache" }
"#,
                )?;
                std::fs::write(
                    home.path().join("config.toml"),
                    format!(
                        r#"[projects.{:?}]
trust_level = "trusted"
"#,
                        project_root.display().to_string()
                    ),
                )?;
                std::fs::create_dir_all(cwd.as_path())?;
                Ok(())
            }
        })
        .build(&server)
        .await?;

    test.submit_turn(PARENT_PROMPT).await?;

    let child_result_request =
        wait_for_matching_request(&child_result, "child shell output", |request| {
            request
                .function_call_output_text(CHILD_SHELL_CALL_ID)
                .is_some()
                && request.header("x-openai-subagent").as_deref() == Some("collab_spawn")
        })
        .await?;
    let output_item = child_result_request.function_call_output(CHILD_SHELL_CALL_ID);
    let output = output_item
        .get("output")
        .and_then(serde_json::Value::as_str)
        .expect("child shell output should be present");
    assert_shell_command_output(output, "worktree-cache")?;

    Ok(())
}

/// This test verifies that a shell, particularly PowerShell, can correctly
/// handle unicode output when the UTF-8 BOM is used. See
/// https://github.com/openai/codex/pull/7902 for more context.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[test_case(true ; "with_login")]
#[test_case(false ; "without_login")]
async fn unicode_output(login: bool) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.2")).await?;

    let call_id = "unicode_output";
    let command = if cfg!(windows) {
        // We use a child process on Windows instead of a PowerShell command
        // like `Write-Output` to ensure that the Powershell config is set
        // correctly.
        "cmd.exe /c echo naïve_café"
    } else {
        "echo \"naïve_café\""
    };
    mount_shell_responses_with_timeout(&harness, call_id, command, Some(login), MEDIUM_TIMEOUT)
        .await;
    harness.submit("run the command without login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "naïve_café")?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[test_case(true ; "with_login")]
#[test_case(false ; "without_login")]
async fn unicode_output_with_newlines(login: bool) -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let harness = shell_command_harness_with(|builder| builder.with_model("gpt-5.2")).await?;

    let call_id = "unicode_output";
    mount_shell_responses_with_timeout(
        &harness,
        call_id,
        "echo 'line1\nnaïve café\nline3'",
        Some(login),
        MEDIUM_TIMEOUT,
    )
    .await;
    harness.submit("run the command without login").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert_shell_command_output(&output, "line1\\nnaïve café\\nline3")?;

    Ok(())
}
