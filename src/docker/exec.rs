use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use bollard::Docker;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::ExecInspectResponse;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use super::output::process_docker_output;
use crate::job::commands::CommandPolicy;
use crate::job::execute::{JobState, OUTPUT_DRAIN_GRACE, StepConclusion, StepResult};
use crate::job::logs::LogSender;
use crate::job::output::OutputProcessor;

const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Run a command inside a running container via `docker exec`.
///
/// This is the container equivalent of `run_process()` — it handles stdout/stderr
/// streaming, workflow command parsing, timeout, and cancellation.
#[allow(clippy::too_many_arguments)]
pub async fn docker_exec(
    docker: &Docker,
    container_id: &str,
    cmd: Vec<String>,
    env: &HashMap<String, String>,
    working_dir: &str,
    job_state: &mut JobState,
    log_sender: &LogSender,
    timeout: Duration,
    cancel_token: &CancellationToken,
    debug_enabled: bool,
) -> Result<StepResult> {
    let env_list: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();

    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd),
                env: Some(env_list),
                working_dir: Some(working_dir.to_string()),
                ..Default::default()
            },
        )
        .await
        .context("creating docker exec")?;

    let exec_output = docker
        .start_exec(&exec.id, None)
        .await
        .context("starting docker exec")?;

    let StartExecResults::Attached { output, .. } = exec_output else {
        anyhow::bail!("docker exec did not return attached output");
    };

    let processor = OutputProcessor::new(
        log_sender.clone(),
        job_state.masks.clone(),
        debug_enabled,
        CommandPolicy::from_env(env),
    );

    let stream_processor = processor.clone();
    let stream_task = tokio::spawn(async move {
        process_docker_output(output, &stream_processor).await;
    });
    let stream_abort = stream_task.abort_handle();

    // The stream stays open while any process holds the exec's output, including a
    // background process the step left behind, so the exec's exit also ends the step.
    let finished = async {
        let mut stream_task = stream_task;
        tokio::select! {
            result = &mut stream_task => result,
            () = wait_for_exit(docker, &exec.id) => {
                match tokio::time::timeout(OUTPUT_DRAIN_GRACE, &mut stream_task).await {
                    Ok(result) => result,
                    Err(_) => {
                        warn!("step exited but a process it started still holds its output, detaching");
                        stream_task.abort();
                        Ok(())
                    }
                }
            }
        }
    };

    let timed_stream = tokio::time::timeout(timeout, finished);

    let result = tokio::select! {
        timeout_result = timed_stream => {
            match timeout_result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => {
                    warn!(error = %e, "docker exec stream task panicked");
                    Ok(())
                }
                Err(_) => {
                    warn!("docker exec timed out");
                    stream_abort.abort();
                    Err(StepConclusion::Failed)
                }
            }
        }
        _ = cancel_token.cancelled() => {
            warn!("job cancelled, docker exec will be stopped");
            stream_abort.abort();
            Err(StepConclusion::Cancelled)
        }
    };

    if let Err(conclusion) = result {
        return Ok(StepResult { conclusion });
    }

    processor.apply_to_job_state(job_state).await;

    // Check exit code
    let inspect = docker
        .inspect_exec(&exec.id)
        .await
        .context("inspecting docker exec result")?;

    let exit_code = inspect.exit_code.unwrap_or(-1);
    let conclusion = if exit_code == 0 && !processor.command_failed() {
        StepConclusion::Succeeded
    } else {
        StepConclusion::Failed
    };

    Ok(StepResult { conclusion })
}

/// Resolves once the exec's main process has exited.
async fn wait_for_exit(docker: &Docker, exec_id: &str) {
    loop {
        match docker.inspect_exec(exec_id).await {
            Ok(inspect) if has_exited(&inspect) => return,
            Ok(_) => {}
            Err(e) => debug!(error = %e, "inspecting docker exec failed"),
        }
        tokio::time::sleep(EXIT_POLL_INTERVAL).await;
    }
}

/// True once the exec's process has run and exited.
///
/// Docker reports `running: false` with no exit code in the window between
/// `start_exec` returning and the process actually starting.
fn has_exited(inspect: &ExecInspectResponse) -> bool {
    inspect.running == Some(false) && inspect.exit_code.is_some()
}

#[cfg(test)]
#[path = "exec_test.rs"]
mod exec_test;
