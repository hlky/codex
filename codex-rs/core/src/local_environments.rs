use anyhow::Context;
use anyhow::Result;
use codex_config::types::LocalEnvironmentConfig;
use codex_config::types::LocalEnvironmentScriptConfig;
use codex_config::types::LocalEnvironmentScriptShell;
use codex_config::types::LocalEnvironmentSourceConfig;
use codex_protocol::config_types::ShellEnvironmentPolicy;
use codex_protocol::config_types::ShellEnvironmentPolicyInherit;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::SystemTime;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::Mutex;

const SNAPSHOT_MARKER: &str = "__CODEX_LOCAL_ENVIRONMENT_START__";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DynamicLocalEnvironmentCacheKey {
    script: LocalEnvironmentScriptConfig,
    modified_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CachedLocalEnvironment {
    key: DynamicLocalEnvironmentCacheKey,
    pub(crate) shell_environment_policy: ShellEnvironmentPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotFormat {
    NulSeparated,
    LineSeparated,
}

struct ScriptCaptureCommand {
    program: String,
    args: Vec<String>,
    format: SnapshotFormat,
}

pub(crate) async fn resolve_local_environment_shell_policy(
    cache: &Mutex<HashMap<String, CachedLocalEnvironment>>,
    name: &str,
    environment: &LocalEnvironmentConfig,
) -> Result<ShellEnvironmentPolicy> {
    match &environment.source {
        LocalEnvironmentSourceConfig::Static(shell_environment_policy) => {
            Ok(shell_environment_policy.clone())
        }
        LocalEnvironmentSourceConfig::Script(script) => {
            resolve_script_local_environment_shell_policy(cache, name, script).await
        }
    }
}

async fn resolve_script_local_environment_shell_policy(
    cache: &Mutex<HashMap<String, CachedLocalEnvironment>>,
    name: &str,
    script: &LocalEnvironmentScriptConfig,
) -> Result<ShellEnvironmentPolicy> {
    let cache_key = DynamicLocalEnvironmentCacheKey {
        script: script.clone(),
        modified_at: std::fs::metadata(script.script.as_path())
            .ok()
            .and_then(|metadata| metadata.modified().ok()),
    };
    if let Some(entry) = cache.lock().await.get(name)
        && entry.key == cache_key
    {
        return Ok(entry.shell_environment_policy.clone());
    }

    let shell_environment_policy =
        environment_snapshot_to_shell_environment_policy(capture_script_environment(script).await?);
    cache.lock().await.insert(
        name.to_string(),
        CachedLocalEnvironment {
            key: cache_key,
            shell_environment_policy: shell_environment_policy.clone(),
        },
    );
    Ok(shell_environment_policy)
}

async fn capture_script_environment(
    script: &LocalEnvironmentScriptConfig,
) -> Result<HashMap<String, String>> {
    if script.shell == LocalEnvironmentScriptShell::Cmd {
        return capture_cmd_script_environment(script).await;
    }
    let command = build_script_capture_command(script);
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = script.cwd.as_ref() {
        process.current_dir(cwd.as_path());
    }
    let output = process.output().await.with_context(|| {
        format!(
            "failed to run local environment script `{}`",
            script.script.display()
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!(
                "local environment script `{}` exited with status {}",
                script.script.display(),
                output.status
            )
        } else {
            format!(
                "local environment script `{}` exited with status {}: {stderr}",
                script.script.display(),
                output.status
            )
        };
        anyhow::bail!(message);
    }
    parse_environment_snapshot(&output.stdout, command.format)
}

async fn capture_cmd_script_environment(
    script: &LocalEnvironmentScriptConfig,
) -> Result<HashMap<String, String>> {
    let _wrapper_dir =
        TempDir::new().context("create temp wrapper dir for cmd local environment")?;
    let wrapper_path = _wrapper_dir
        .path()
        .join("codex-local-environment-wrapper.cmd");
    let quoted_script = cmd_quote(script.script.to_string_lossy().as_ref());
    let quoted_args = script
        .args
        .iter()
        .map(|arg| format!(" {}", cmd_quote(arg)))
        .collect::<String>();
    std::fs::write(
        &wrapper_path,
        format!(
            "@echo off\r\ncall {quoted_script}{quoted_args}\r\nif errorlevel 1 exit /b %errorlevel%\r\necho {SNAPSHOT_MARKER}\r\nset\r\n"
        )
        .as_bytes(),
    )
    .context("write temp wrapper for cmd local environment")?;

    let mut process = Command::new("cmd.exe");
    process
        .args(["/d", "/c", wrapper_path.to_string_lossy().as_ref()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = script.cwd.as_ref() {
        process.current_dir(cwd.as_path());
    }
    let output = process.output().await.with_context(|| {
        format!(
            "failed to run local environment script `{}`",
            script.script.display()
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!(
                "local environment script `{}` exited with status {}",
                script.script.display(),
                output.status
            )
        } else {
            format!(
                "local environment script `{}` exited with status {}: {stderr}",
                script.script.display(),
                output.status
            )
        };
        anyhow::bail!(message);
    }
    parse_environment_snapshot(&output.stdout, SnapshotFormat::LineSeparated)
}

fn build_script_capture_command(script: &LocalEnvironmentScriptConfig) -> ScriptCaptureCommand {
    match script.shell {
        LocalEnvironmentScriptShell::Sh
        | LocalEnvironmentScriptShell::Bash
        | LocalEnvironmentScriptShell::Zsh => {
            let shell_program = match script.shell {
                LocalEnvironmentScriptShell::Sh => "sh",
                LocalEnvironmentScriptShell::Bash => "bash",
                LocalEnvironmentScriptShell::Zsh => "zsh",
                LocalEnvironmentScriptShell::Cmd | LocalEnvironmentScriptShell::PowerShell => {
                    unreachable!()
                }
            };
            let quoted_script = posix_shell_quote(script.script.to_string_lossy().as_ref());
            let quoted_args = script
                .args
                .iter()
                .map(|arg| format!(" {}", posix_shell_quote(arg)))
                .collect::<String>();
            let command = format!(
                ". {quoted_script}{quoted_args}; status=$?; if [ $status -ne 0 ]; then exit $status; fi; printf '%s\\n' '{SNAPSHOT_MARKER}'; env -0"
            );
            ScriptCaptureCommand {
                program: shell_program.to_string(),
                args: vec!["-lc".to_string(), command],
                format: SnapshotFormat::NulSeparated,
            }
        }
        LocalEnvironmentScriptShell::Cmd => unreachable!(),
        LocalEnvironmentScriptShell::PowerShell => {
            let quoted_script = powershell_quote(script.script.to_string_lossy().as_ref());
            let quoted_args = script
                .args
                .iter()
                .map(|arg| format!(" '{}'", powershell_quote(arg)))
                .collect::<String>();
            let command = format!(
                "& '{quoted_script}'{quoted_args}; if (-not $?) {{ if ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }} else {{ exit 1 }} }}; Write-Output '{SNAPSHOT_MARKER}'; Get-ChildItem Env: | ForEach-Object {{ \"{{0}}={{1}}\" -f $_.Name, $_.Value }}"
            );
            ScriptCaptureCommand {
                program: powershell_program().to_string(),
                args: vec!["-NoProfile".to_string(), "-Command".to_string(), command],
                format: SnapshotFormat::LineSeparated,
            }
        }
    }
}

fn parse_environment_snapshot(
    stdout: &[u8],
    format: SnapshotFormat,
) -> Result<HashMap<String, String>> {
    match format {
        SnapshotFormat::NulSeparated => parse_nul_separated_environment_snapshot(stdout),
        SnapshotFormat::LineSeparated => parse_line_separated_environment_snapshot(stdout),
    }
}

fn parse_nul_separated_environment_snapshot(stdout: &[u8]) -> Result<HashMap<String, String>> {
    let marker = format!("{SNAPSHOT_MARKER}\n").into_bytes();
    let Some(start) = stdout
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        anyhow::bail!("local environment snapshot marker not found in shell output");
    };
    let snapshot = &stdout[start + marker.len()..];
    let mut env = HashMap::new();
    for entry in snapshot.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        if let Some((key, value)) = split_env_entry(entry)? {
            env.insert(key, value);
        }
    }
    Ok(env)
}

fn parse_line_separated_environment_snapshot(stdout: &[u8]) -> Result<HashMap<String, String>> {
    let output = String::from_utf8_lossy(stdout);
    let mut found_marker = false;
    let mut env = HashMap::new();
    for line in output.lines() {
        let line = line.trim_end_matches('\r');
        if !found_marker {
            found_marker = line == SNAPSHOT_MARKER;
            continue;
        }
        if let Some((key, value)) = split_env_line(line) {
            env.insert(key, value);
        }
    }
    if !found_marker {
        anyhow::bail!("local environment snapshot marker not found in shell output");
    }
    Ok(env)
}

fn split_env_entry(entry: &[u8]) -> Result<Option<(String, String)>> {
    let Some(index) = entry.iter().position(|byte| *byte == b'=') else {
        return Ok(None);
    };
    let key = String::from_utf8(entry[..index].to_vec())
        .context("local environment snapshot contained a non-UTF8 variable name")?;
    if key.is_empty() || key.starts_with('=') {
        return Ok(None);
    }
    let value = String::from_utf8(entry[index + 1..].to_vec())
        .context("local environment snapshot contained a non-UTF8 variable value")?;
    Ok(Some((key, value)))
}

fn split_env_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    if key.is_empty() || key.starts_with('=') {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

fn environment_snapshot_to_shell_environment_policy(
    env: HashMap<String, String>,
) -> ShellEnvironmentPolicy {
    ShellEnvironmentPolicy {
        inherit: ShellEnvironmentPolicyInherit::None,
        ignore_default_excludes: true,
        exclude: Vec::new(),
        r#set: env,
        path_prepend: Vec::new(),
        path_append: Vec::new(),
        include_only: Vec::new(),
        use_profile: false,
    }
}

fn posix_shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', r"'\''"))
}

fn powershell_quote(input: &str) -> String {
    input.replace('\'', "''")
}

fn cmd_quote(input: &str) -> String {
    let escaped = input
        .replace('^', "^^")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

fn powershell_program() -> &'static str {
    if cfg!(windows) {
        "powershell.exe"
    } else {
        "powershell"
    }
}

#[cfg(test)]
#[path = "local_environments_tests.rs"]
mod tests;
