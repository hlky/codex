use super::shell_environment_policy_for_command_cwd;
use crate::session::tests::make_session_and_context;
use crate::tools::handlers::workdir_shell_environment::refreshed_config_for_cwd;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn refreshed_config_for_cwd_loads_trusted_project_config() {
    let (_session, mut turn) = make_session_and_context().await;
    let codex_home = tempdir().expect("tempdir");
    let parent_cwd = tempdir().expect("tempdir");
    let project_root = codex_home.path().join("worktree");
    let dot_codex = project_root.join(".codex");
    std::fs::create_dir_all(&dot_codex).expect("create project config dir");
    std::fs::write(project_root.join(".git"), "gitdir: here\n").expect("write .git");
    std::fs::write(
        dot_codex.join("config.toml"),
        r#"
[shell_environment_policy]
inherit = "all"
set = { DINOML_CACHE_DIR = "worktree-cache" }
"#,
    )
    .expect("write project config");
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!(
            r#"[projects.{:?}]
trust_level = "trusted"
"#,
            project_root.display().to_string()
        ),
    )
    .expect("write user config");

    let parent_config = crate::config::ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(parent_cwd.path().to_path_buf()))
        .build()
        .await
        .expect("load parent config");
    turn.config = std::sync::Arc::new(parent_config);
    let cwd = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(&project_root)
        .expect("absolute project root");

    let refreshed = refreshed_config_for_cwd(turn.config.as_ref(), &cwd)
        .await
        .expect("refresh config")
        .expect("expected refreshed config");

    assert_eq!(
        refreshed
            .permissions
            .shell_environment_policy
            .r#set
            .get("DINOML_CACHE_DIR"),
        Some(&"worktree-cache".to_string())
    );
}

#[tokio::test]
async fn shell_environment_policy_for_command_cwd_uses_trusted_project_config() {
    let (session, mut turn) = make_session_and_context().await;
    let codex_home = tempdir().expect("tempdir");
    let parent_cwd = tempdir().expect("tempdir");
    let project_root = codex_home.path().join("worktree");
    let dot_codex = project_root.join(".codex");
    std::fs::create_dir_all(&dot_codex).expect("create project config dir");
    std::fs::write(project_root.join(".git"), "gitdir: here\n").expect("write .git");
    std::fs::write(
        dot_codex.join("config.toml"),
        r#"
[shell_environment_policy]
inherit = "all"
set = { DINOML_CACHE_DIR = "worktree-cache" }
"#,
    )
    .expect("write project config");
    std::fs::write(
        codex_home.path().join("config.toml"),
        format!(
            r#"[projects.{:?}]
trust_level = "trusted"
"#,
            project_root.display().to_string()
        ),
    )
    .expect("write user config");

    let parent_config = crate::config::ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(parent_cwd.path().to_path_buf()))
        .build()
        .await
        .expect("load parent config");
    turn.config = std::sync::Arc::new(parent_config);
    let cwd = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(&project_root)
        .expect("absolute project root");

    let policy = shell_environment_policy_for_command_cwd(&session, &turn, &cwd)
        .await
        .expect("resolve shell environment policy");

    assert_eq!(
        policy.r#set.get("DINOML_CACHE_DIR"),
        Some(&"worktree-cache".to_string())
    );
}
