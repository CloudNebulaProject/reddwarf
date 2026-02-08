use crate::error::{Result, RuntimeError};
use tracing::debug;

/// Output from a command execution
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Execute a command and fail on non-zero exit code
pub async fn exec(program: &str, args: &[&str]) -> Result<CommandOutput> {
    let output = exec_unchecked(program, args).await?;

    if output.exit_code != 0 {
        return Err(RuntimeError::command_failed(
            format!("{} {}", program, args.join(" ")),
            output.exit_code,
            &output.stderr,
        ));
    }

    Ok(output)
}

/// Execute a command and return output regardless of exit code
pub async fn exec_unchecked(program: &str, args: &[&str]) -> Result<CommandOutput> {
    debug!("Executing: {} {}", program, args.join(" "));

    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            RuntimeError::command_failed(
                format!("{} {}", program, args.join(" ")),
                -1,
                e.to_string(),
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    debug!(
        "Command exited with code {}: {} {}",
        exit_code,
        program,
        args.join(" ")
    );

    Ok(CommandOutput {
        stdout,
        stderr,
        exit_code,
    })
}
