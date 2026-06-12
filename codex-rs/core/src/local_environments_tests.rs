use super::*;
use codex_config::types::LocalEnvironmentConfig;
use codex_config::types::LocalEnvironmentScriptConfig;
use codex_config::types::LocalEnvironmentScriptShell;
use codex_config::types::LocalEnvironmentSourceConfig;
use codex_protocol::config_types::ShellEnvironmentPolicyInherit;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tokio::time::Duration;

#[test]
fn parse_line_separated_environment_snapshot_ignores_preamble() {
    let stdout = b"before\r\n__CODEX_LOCAL_ENVIRONMENT_START__\r\n=ExitCode=00000000\r\nFOO=bar\r\nBAZ=qux\r\n";
    let parsed = parse_line_separated_environment_snapshot(stdout)
        .expect("line-separated parse should work");

    assert_eq!(
        parsed,
        HashMap::from([
            ("FOO".to_string(), "bar".to_string()),
            ("BAZ".to_string(), "qux".to_string()),
        ])
    );
}

#[test]
fn parse_nul_separated_environment_snapshot_reads_entries_after_marker() {
    let stdout = b"before\n__CODEX_LOCAL_ENVIRONMENT_START__\nFOO=bar\0BAZ=qux\0";
    let parsed =
        parse_nul_separated_environment_snapshot(stdout).expect("nul-separated parse should work");

    assert_eq!(
        parsed,
        HashMap::from([
            ("FOO".to_string(), "bar".to_string()),
            ("BAZ".to_string(), "qux".to_string()),
        ])
    );
}

#[tokio::test]
async fn resolve_script_local_environment_shell_policy_captures_script_environment() {
    let temp_dir = tempdir().expect("tempdir");
    let script_path = write_dynamic_test_script(temp_dir.path(), "one");
    let cache = Mutex::new(HashMap::new());
    let policy = resolve_local_environment_shell_policy(
        &cache,
        "dynamic",
        &LocalEnvironmentConfig {
            description: Some("dynamic".to_string()),
            source: LocalEnvironmentSourceConfig::Script(test_script_config(&script_path)),
        },
    )
    .await
    .expect("script local environment should resolve");

    assert_eq!(policy.inherit, ShellEnvironmentPolicyInherit::None);
    assert_eq!(
        policy.r#set.get("CODEX_DYNAMIC_TEST"),
        Some(&"one".to_string())
    );
}

#[tokio::test]
async fn resolve_script_local_environment_shell_policy_refreshes_when_script_changes() {
    let temp_dir = tempdir().expect("tempdir");
    let script_path = write_dynamic_test_script(temp_dir.path(), "one");
    let cache = Mutex::new(HashMap::new());
    let environment = LocalEnvironmentConfig {
        description: Some("dynamic".to_string()),
        source: LocalEnvironmentSourceConfig::Script(test_script_config(&script_path)),
    };

    let initial = resolve_local_environment_shell_policy(&cache, "dynamic", &environment)
        .await
        .expect("initial script local environment should resolve");
    assert_eq!(
        initial.r#set.get("CODEX_DYNAMIC_TEST"),
        Some(&"one".to_string())
    );

    tokio::time::sleep(Duration::from_millis(1100)).await;
    write_dynamic_test_script_with_value(&script_path, "two");

    let updated = resolve_local_environment_shell_policy(&cache, "dynamic", &environment)
        .await
        .expect("updated script local environment should resolve");
    assert_eq!(
        updated.r#set.get("CODEX_DYNAMIC_TEST"),
        Some(&"two".to_string())
    );
}

fn test_script_config(script_path: &std::path::Path) -> LocalEnvironmentScriptConfig {
    LocalEnvironmentScriptConfig {
        script: AbsolutePathBuf::from_absolute_path(script_path)
            .expect("script path should be absolute"),
        shell: test_script_shell(),
        args: Vec::new(),
        cwd: None,
    }
}

#[cfg(windows)]
fn write_dynamic_test_script(dir: &std::path::Path, value: &str) -> std::path::PathBuf {
    let script_path = dir.join("dynamic-env.bat");
    write_dynamic_test_script_with_value(&script_path, value);
    script_path
}

#[cfg(not(windows))]
fn write_dynamic_test_script(dir: &std::path::Path, value: &str) -> std::path::PathBuf {
    let script_path = dir.join("dynamic-env.sh");
    write_dynamic_test_script_with_value(&script_path, value);
    script_path
}

#[cfg(windows)]
fn write_dynamic_test_script_with_value(script_path: &std::path::Path, value: &str) {
    std::fs::write(
        script_path,
        format!("@echo off\r\nset CODEX_DYNAMIC_TEST={value}\r\n"),
    )
    .expect("write windows test script");
}

#[cfg(not(windows))]
fn write_dynamic_test_script_with_value(script_path: &std::path::Path, value: &str) {
    std::fs::write(script_path, format!("export CODEX_DYNAMIC_TEST={value}\n"))
        .expect("write unix test script");
}

#[cfg(windows)]
fn test_script_shell() -> LocalEnvironmentScriptShell {
    LocalEnvironmentScriptShell::Cmd
}

#[cfg(not(windows))]
fn test_script_shell() -> LocalEnvironmentScriptShell {
    LocalEnvironmentScriptShell::Sh
}
