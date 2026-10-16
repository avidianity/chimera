mod common;

use std::collections::HashMap;

use chimera::job::client::JobConclusion;
use chimera::job::schema::JobManifest;
use common::*;

// Both lines are syntax errors in bash, so success proves python ran the script.
const PYTHON_ONLY: &str = "import sys\nsys.exit(0 if 1 + 1 == 2 else 1)";

fn with_default_shell(mut manifest: JobManifest, shell: &str) -> JobManifest {
    manifest.defaults.insert(
        "run".into(),
        HashMap::from([("shell".into(), shell.into())]),
    );
    manifest
}

#[tokio::test]
async fn python_shell_runs_the_script_with_python() {
    let env = TestEnv::setup().await;
    let manifest = manifest_with_steps(
        vec![script_step_with_shell("s1", PYTHON_ONLY, "python")],
        &env.mock_server.uri(),
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn explicit_bash_fails_on_a_failing_pipeline_stage() {
    let env = TestEnv::setup().await;
    let manifest = manifest_with_steps(
        vec![script_step_with_shell("s1", "false | true", "bash")],
        &env.mock_server.uri(),
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Failed);
}

#[tokio::test]
async fn default_shell_ignores_a_failing_pipeline_stage() {
    let env = TestEnv::setup().await;
    let manifest = manifest_with_steps(
        vec![script_step("s1", "false | true")],
        &env.mock_server.uri(),
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn defaults_run_shell_applies_to_steps_without_their_own() {
    let env = TestEnv::setup().await;
    let manifest = with_default_shell(
        manifest_with_steps(vec![script_step("s1", PYTHON_ONLY)], &env.mock_server.uri()),
        "python",
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn step_shell_overrides_defaults_run_shell() {
    let env = TestEnv::setup().await;
    let manifest = with_default_shell(
        manifest_with_steps(
            vec![script_step_with_shell(
                "s1",
                "test -n \"$BASH_VERSION\"",
                "bash",
            )],
            &env.mock_server.uri(),
        ),
        "python",
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn custom_shell_runs_the_script_through_its_command() {
    let env = TestEnv::setup().await;
    let manifest = manifest_with_steps(
        vec![script_step_with_shell("s1", PYTHON_ONLY, "python3 -u {0}")],
        &env.mock_server.uri(),
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn invalid_shell_fails_the_job() {
    let env = TestEnv::setup().await;
    let manifest = manifest_with_steps(
        vec![script_step_with_shell("s1", "echo hi", "fish")],
        &env.mock_server.uri(),
    );

    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Failed);
}
