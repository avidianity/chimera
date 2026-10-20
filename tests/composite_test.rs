mod common;

use chimera::job::client::JobConclusion;
use common::*;

fn create_greet_action(workspace_dir: &std::path::Path) {
    let action_dir = workspace_dir.join(".github/actions/greet");
    std::fs::create_dir_all(&action_dir).unwrap();
    std::fs::write(
        action_dir.join("action.yml"),
        r#"
name: 'Greet'
description: 'A test composite action'
inputs:
  name:
    description: 'Who to greet'
    required: true
  loud:
    description: 'Uppercase the greeting'
    default: 'false'
runs:
  using: 'composite'
  steps:
    - run: |
        if [ "$INPUT_LOUD" = "true" ]; then
          echo "HELLO $INPUT_NAME!!!" | tr '[:lower:]' '[:upper:]'
        else
          echo "Hello, $INPUT_NAME!"
        fi
      shell: bash
    - run: echo "Greeting delivered"
      shell: bash
"#,
    )
    .unwrap();
}

fn composite_step(id: &str, action_path: &str, inputs: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "displayName": format!("Run: {id}"),
        "reference": {
            "name": action_path,
            "type": "repository",
            "repositoryType": "self",
            "path": action_path
        },
        "inputs": inputs,
        "condition": null,
        "timeoutInMinutes": null,
        "continueOnError": false,
        "order": 1,
        "environment": null,
        "contextName": id
    })
}

#[tokio::test]
async fn composite_action_runs() {
    let env = TestEnv::setup().await;
    create_greet_action(env.workspace.workspace_dir());

    let manifest = manifest_with_steps(
        vec![composite_step(
            "greet",
            ".github/actions/greet",
            serde_json::json!({"name": "chimera"}),
        )],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();
    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn composite_action_with_input_variation() {
    let env = TestEnv::setup().await;
    create_greet_action(env.workspace.workspace_dir());

    let manifest = manifest_with_steps(
        vec![
            composite_step(
                "quiet",
                ".github/actions/greet",
                serde_json::json!({"name": "chimera"}),
            ),
            composite_step(
                "loud",
                ".github/actions/greet",
                serde_json::json!({"name": "chimera", "loud": "true"}),
            ),
        ],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();
    assert_eq!(conclusion, JobConclusion::Succeeded);
}

/// A composite action that forwards a hyphenated input of its own to a sub-step,
/// the way `pnpm/action-setup`'s `dest:` is wired up in real workflows.
fn create_forwarding_action(workspace_dir: &std::path::Path) {
    let action_dir = workspace_dir.join(".github/actions/forward");
    std::fs::create_dir_all(&action_dir).unwrap();
    std::fs::write(
        action_dir.join("action.yml"),
        r#"
name: 'Forward'
description: 'Forwards a hyphenated input to a sub-step'
inputs:
  pnpm-dest:
    description: 'Where pnpm goes'
    required: false
    default: '~/setup-pnpm'
runs:
  using: 'composite'
  steps:
    - shell: bash
      run: |
        echo "dest=${{ inputs.pnpm-dest }}"
        test "${{ inputs.pnpm-dest }}" = "/tmp/pnpm-here" || exit 1
"#,
    )
    .unwrap();
}

#[tokio::test]
async fn composite_action_forwards_hyphenated_input() {
    let env = TestEnv::setup().await;
    create_forwarding_action(env.workspace.workspace_dir());

    let manifest = manifest_with_steps(
        vec![composite_step(
            "forward",
            ".github/actions/forward",
            serde_json::json!({"pnpm-dest": "/tmp/pnpm-here"}),
        )],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();
    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn composite_action_runs_a_script_from_its_own_path() {
    use std::os::unix::fs::PermissionsExt;

    let env = TestEnv::setup().await;
    let action_dir = env.workspace.workspace_dir().join(".github/actions/entry");
    std::fs::create_dir_all(&action_dir).unwrap();
    std::fs::write(
        action_dir.join("action.yml"),
        r#"
name: 'Entry'
description: 'Adds its own path to PATH, then calls a script from it'
runs:
  using: 'composite'
  steps:
    - run: echo "$ACTION_PATH" >> $GITHUB_PATH
      shell: bash
      env:
        ACTION_PATH: ${{ github.action_path }}
    - run: entrypoint.sh
      shell: bash
"#,
    )
    .unwrap();
    let script = action_dir.join("entrypoint.sh");
    std::fs::write(&script, "#!/usr/bin/env bash\necho entrypoint ran\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let manifest = manifest_with_steps(
        vec![composite_step(
            "entry",
            ".github/actions/entry",
            serde_json::json!({}),
        )],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();
    assert_eq!(conclusion, JobConclusion::Succeeded);
}

/// GitHub reads every `with:` and `env:` value of a composite sub-step as a string, so
/// an unquoted `true` or `3` reaches the nested action like its quoted form.
#[tokio::test]
async fn composite_action_passes_non_string_scalars_as_strings() {
    let env = TestEnv::setup().await;
    let actions_dir = env.workspace.workspace_dir().join(".github/actions");
    std::fs::create_dir_all(actions_dir.join("inner")).unwrap();
    std::fs::write(
        actions_dir.join("inner/action.yml"),
        r#"
name: 'Inner'
description: 'Checks the inputs it receives'
inputs:
  cache:
    default: 'false'
  retries:
    default: '1'
runs:
  using: 'composite'
  steps:
    - run: |
        test "${{ inputs.cache }}" = "true" || exit 1
        test "${{ inputs.retries }}" = "3" || exit 1
      shell: bash
"#,
    )
    .unwrap();
    std::fs::create_dir_all(actions_dir.join("outer")).unwrap();
    std::fs::write(
        actions_dir.join("outer/action.yml"),
        r#"
name: 'Outer'
description: 'Passes unquoted scalars down'
runs:
  using: 'composite'
  steps:
    - uses: ./.github/actions/inner
      with:
        cache: true
        retries: 3
    - run: |
        test "$VERBOSE" = "false" || exit 1
        test "$PORT" = "8080" || exit 1
      shell: bash
      env:
        VERBOSE: false
        PORT: 8080
"#,
    )
    .unwrap();

    let manifest = manifest_with_steps(
        vec![composite_step(
            "outer",
            ".github/actions/outer",
            serde_json::json!({}),
        )],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

fn write_action(workspace_dir: &std::path::Path, name: &str, action_yml: &str) {
    let action_dir = workspace_dir.join(".github/actions").join(name);
    std::fs::create_dir_all(&action_dir).unwrap();
    std::fs::write(action_dir.join("action.yml"), action_yml).unwrap();
}

#[tokio::test]
async fn composite_sub_step_reads_output_of_earlier_sub_step() {
    let env = TestEnv::setup().await;
    write_action(
        env.workspace.workspace_dir(),
        "repro",
        r#"
name: repro
runs:
  using: composite
  steps:
    - id: mark
      shell: bash
      run: echo "name=hello" >> "$GITHUB_OUTPUT"
    - shell: bash
      run: |
        echo "name is '${{ steps.mark.outputs.name }}'"
        test "${{ steps.mark.outputs.name }}" = "hello" || exit 1
"#,
    );

    let manifest = manifest_with_steps(
        vec![composite_step(
            "repro",
            ".github/actions/repro",
            serde_json::json!({}),
        )],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

/// The workflow reads a composite action's outputs through its `outputs:` mapping,
/// not through whatever its sub-steps happened to write.
#[tokio::test]
async fn composite_action_outputs_mapping_reaches_the_workflow() {
    let env = TestEnv::setup().await;
    write_action(
        env.workspace.workspace_dir(),
        "mapped",
        r#"
name: mapped
inputs:
  suffix:
    default: 'world'
outputs:
  greeting:
    description: 'The greeting'
    value: ${{ steps.make.outputs.raw }}-${{ inputs.suffix }}
runs:
  using: composite
  steps:
    - id: make
      shell: bash
      run: echo "raw=hello" >> "$GITHUB_OUTPUT"
"#,
    );

    let manifest = manifest_with_steps(
        vec![
            composite_step("mapped", ".github/actions/mapped", serde_json::json!({})),
            script_step(
                "check",
                r#"
                test "${{ steps.mapped.outputs.greeting }}" = "hello-world" || exit 1
                test -z "${{ steps.mapped.outputs.raw }}" || exit 1
                test -z "${{ steps.make.outputs.raw }}" || exit 1
                "#,
            ),
        ],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

#[tokio::test]
async fn composite_sub_step_sees_env_and_path_from_earlier_sub_step() {
    let env = TestEnv::setup().await;
    write_action(
        env.workspace.workspace_dir(),
        "env-chain",
        r#"
name: env-chain
runs:
  using: composite
  steps:
    - shell: bash
      run: |
        echo "GREETING=hi" >> "$GITHUB_ENV"
        echo "/opt/composite-bin" >> "$GITHUB_PATH"
    - shell: bash
      run: |
        test "$GREETING" = "hi" || exit 1
        case ":$PATH:" in *:/opt/composite-bin:*) ;; *) exit 1 ;; esac
"#,
    );

    let manifest = manifest_with_steps(
        vec![
            composite_step(
                "env-chain",
                ".github/actions/env-chain",
                serde_json::json!({}),
            ),
            script_step("after", r#"test "$GREETING" = "hi" || exit 1"#),
        ],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

/// The shape of the reported failure: a sub-step's output is handed to a nested
/// action's `with:`, and the nested action's mapped output is read afterwards.
#[tokio::test]
async fn composite_passes_sub_step_output_to_nested_action() {
    let env = TestEnv::setup().await;
    let workspace_dir = env.workspace.workspace_dir();
    write_action(
        workspace_dir,
        "echo-name",
        r#"
name: echo-name
inputs:
  name:
    required: true
outputs:
  echoed:
    value: ${{ steps.echo.outputs.echoed }}
runs:
  using: composite
  steps:
    - id: echo
      shell: bash
      run: |
        test -n "${{ inputs.name }}" || exit 1
        echo "echoed=${{ inputs.name }}" >> "$GITHUB_OUTPUT"
"#,
    );
    write_action(
        workspace_dir,
        "outer",
        r#"
name: outer
outputs:
  final:
    value: ${{ steps.nested.outputs.echoed }}
runs:
  using: composite
  steps:
    - id: mark
      shell: bash
      run: echo "name=artifact-1" >> "$GITHUB_OUTPUT"
    - id: nested
      uses: ./.github/actions/echo-name
      with:
        name: ${{ steps.mark.outputs.name }}
    - shell: bash
      run: test "${{ steps.nested.outputs.echoed }}" = "artifact-1" || exit 1
"#,
    );

    let manifest = manifest_with_steps(
        vec![
            composite_step("outer", ".github/actions/outer", serde_json::json!({})),
            script_step(
                "check",
                r#"test "${{ steps.outer.outputs.final }}" = "artifact-1" || exit 1"#,
            ),
        ],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}

/// A composite action has its own `steps` context, and the workflow's comes back
/// once the action is done.
#[tokio::test]
async fn composite_action_scopes_the_steps_context() {
    let env = TestEnv::setup().await;
    write_action(
        env.workspace.workspace_dir(),
        "scoped",
        r#"
name: scoped
runs:
  using: composite
  steps:
    - shell: bash
      run: test -z "${{ steps.first.outputs.value }}" || exit 1
    - id: flaky
      shell: bash
      continue-on-error: true
      run: exit 1
    - shell: bash
      run: |
        test "${{ steps.flaky.outcome }}" = "failure" || exit 1
        test "${{ steps.flaky.conclusion }}" = "success" || exit 1
"#,
    );

    let manifest = manifest_with_steps(
        vec![
            script_step("first", r#"echo "value=kept" >> "$GITHUB_OUTPUT""#),
            composite_step("scoped", ".github/actions/scoped", serde_json::json!({})),
            script_step(
                "check",
                r#"
                test "${{ steps.first.outputs.value }}" = "kept" || exit 1
                test -z "${{ steps.flaky.outcome }}" || exit 1
                "#,
            ),
        ],
        &env.mock_server.uri(),
    );
    let (conclusion, _) = env.run(&manifest).await.unwrap();

    assert_eq!(conclusion, JobConclusion::Succeeded);
}
