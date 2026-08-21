use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;
const MAX_OUTPUT_BYTES_PER_STREAM: usize = 512 * 1024;

#[derive(Deserialize)]
pub struct ExecuteCommandArgs {
    pub command: String,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "execute_command".to_string(),
        description: Some("Executes a shell command.".to_string()),
        parameters: Some(obj(
            vec![
                ("command", p("string", "The command to execute")),
                (
                    "timeout_seconds",
                    p(
                        "integer",
                        "Maximum runtime in seconds (default 120, maximum 600)",
                    ),
                ),
            ],
            vec!["command"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: ExecuteCommandArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!(
        "{} Executing: {}",
        "🚀 ".bright_magenta(),
        args.command.bright_magenta()
    );

    if !host.ask_permission(
        "execute_command",
        &args.command,
        &format!("Execute command: {}", args.command),
    )? {
        return Ok("Command not run (denied or cancelled). If in planning mode, switch with /mode agentic.".to_string());
    }

    let timeout_secs = args
        .timeout_seconds
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);
    let output = run_shell_command(&args.command, Duration::from_secs(timeout_secs)).await?;
    // A launched shell command can mutate files even when it exits non-zero or
    // times out, so conservatively flag the workspace after execution completes.
    host.mark_workspace_changed();
    Ok(output)
}

async fn run_shell_command(command: &str, timeout: Duration) -> Result<String> {
    // Run through the platform shell; the system prompt tells the model which one.
    let mut process = if cfg!(target_os = "windows") {
        let mut command_process = Command::new("cmd");
        command_process.args(["/C", command]);
        command_process
    } else {
        let mut command_process = Command::new("sh");
        command_process.args(["-c", command]);
        command_process
    };
    process
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = process.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("command stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("command stderr was not captured"))?;

    let execution = async {
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_capped(stdout, MAX_OUTPUT_BYTES_PER_STREAM),
            read_capped(stderr, MAX_OUTPUT_BYTES_PER_STREAM),
        )?;
        Ok::<_, std::io::Error>((status, stdout, stderr))
    };

    let (status, (stdout, stdout_truncated), (stderr, stderr_truncated)) =
        match tokio::time::timeout(timeout, execution).await {
            Ok(result) => result?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Ok(format!(
                    "Command timed out after {} seconds and was terminated.",
                    timeout.as_secs()
                ));
            }
        };

    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    let exit = status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    let stdout_note = if stdout_truncated {
        "\n[stdout truncated at 512 KiB]"
    } else {
        ""
    };
    let stderr_note = if stderr_truncated {
        "\n[stderr truncated at 512 KiB]"
    } else {
        ""
    };
    Ok(format!(
        "EXIT STATUS: {exit}\nSTDOUT:\n{stdout}{stdout_note}\nSTDERR:\n{stderr}{stderr_note}"
    ))
}

/// Drain a pipe fully so the child cannot block on a full buffer, while retaining
/// only the first `limit` bytes for the model-facing result.
async fn read_capped<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(kept.len());
        let take = read.min(remaining);
        kept.extend_from_slice(&buffer[..take]);
        truncated |= take < read;
    }
    Ok((kept, truncated))
}

#[cfg(test)]
mod tests {
    use super::read_capped;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn output_reader_drains_but_caps_retained_bytes() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let write = tokio::spawn(async move {
            writer.write_all(b"abcdefghij").await.unwrap();
        });
        let (bytes, truncated) = read_capped(reader, 4).await.unwrap();
        write.await.unwrap();
        assert_eq!(bytes, b"abcd");
        assert!(truncated);
    }
}
