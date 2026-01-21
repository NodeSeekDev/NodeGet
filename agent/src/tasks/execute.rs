use crate::AGENT_CONFIG;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const EXECUTE_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn execute_command(command: String) -> Result<String, String> {
    let config = AGENT_CONFIG.get().expect("Agent config not initialized");
    let max_chars = config.exec_max_character.unwrap_or(10000);

    let exec_shell_config = config.exec_shell.clone().unwrap_or_else(|| {
        #[cfg(target_os = "windows")]
        return "cmd".to_string();
        #[cfg(not(target_os = "windows"))]
        return "bash".to_string();
    });

    let mut shells_to_try = vec![exec_shell_config.as_str()];

    #[cfg(target_os = "windows")]
    {
        if exec_shell_config.as_str() != "cmd" && !shells_to_try.contains(&"cmd") {
            shells_to_try.push("cmd");
        }
        if exec_shell_config.as_str() != "powershell" && !shells_to_try.contains(&"powershell") {
            shells_to_try.push("powershell");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if exec_shell_config.as_str() != "bash" && !shells_to_try.contains(&"bash") {
            shells_to_try.push("bash");
        }
        if exec_shell_config.as_str() != "sh" && !shells_to_try.contains(&"sh") {
            shells_to_try.push("sh");
        }
    }


    let mut last_error: String = "No shell was attempted.".to_string();

    for shell in &shells_to_try {
        let (shell_path, shell_arg) = {
            #[cfg(target_os = "windows")]
            if shell.eq_ignore_ascii_case("powershell") {
                (shell, vec!["-Command"])
            } else {
                (shell, vec!["/C"])
            }
            #[cfg(not(target_os = "windows"))]
            (shell, vec!["-c"])
        };

        let mut cmd = Command::new(*shell_path);
        cmd.args(shell_arg).arg(&command);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        // 尝试启动子进程
        let child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                log::warn!("Shell '{}' not found or usable, trying fallback: {}", shell, e);
                last_error = e.to_string();
                continue;
            }
        };

        // 等待结果
        match timeout(EXECUTE_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => {
                let mut result = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr);

                if !stderr.is_empty() && !result.is_empty() {
                    result.push_str("\n--- STDERR ---\n");
                }
                result.push_str(&stderr);

                if result.is_empty() {
                    result.push_str("(No Output)");
                }

                if !output.status.success() {
                    use std::fmt::Write;
                    let _ = write!(
                        result,
                        "\n\n[Process exited with code {}]",
                        output.status.code().unwrap_or(-1)
                    );
                }

                // 截断并返回
                if result.len() > max_chars {
                    let original_len = result.len();
                    let truncated_part = result.split_off(original_len - max_chars);
                    result = format!(
                        "[... Output truncated from {} to {} chars ...]\n{}",
                        original_len,
                        max_chars,
                        truncated_part
                    );
                }

                return Ok(result);
            }
            Ok(Err(e)) => return Err(format!("Failed to wait for process: {e}")),
            Err(_) => return Err(format!(
                "Execution timed out (Limit: {}s)",
                EXECUTE_TIMEOUT.as_secs()
            )),
        }
    }

    // 所有 Shell 均失败
    Err(format!(
        "All available shells failed to execute command. Last error: {}",
        last_error
    ))
}
