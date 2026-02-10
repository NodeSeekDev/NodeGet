use crate::AGENT_CONFIG;
use nodeget_lib::error::NodegetError;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

// 命令执行超时时间，设定为 60 秒
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(60);

/// 命令执行结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

// 执行指定的命令
//
// 该函数根据操作系统和配置选择合适的 shell 来执行命令，并处理输出结果
//
// # 参数
// * `command` - 要执行的命令字符串
//
// # 返回值
// 成功时返回命令输出字符串，失败时返回错误信息
pub async fn execute_command(command: String) -> Result<String> {
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

    let mut last_error: NodegetError = NodegetError::Other("No shell was attempted.".to_owned());

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
                log::warn!("Shell '{shell}' not found or usable, trying fallback: {e}");
                last_error = NodegetError::Other(format!("{e}"));
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
                        "[... Output truncated from {original_len} to {max_chars} chars ...]\n{truncated_part}"
                    );
                }

                return Ok(result);
            }
            Ok(Err(e)) => return Err(NodegetError::Other(format!("Failed to wait for process: {e}"))),
            Err(_) => {
                return Err(NodegetError::Other(format!(
                    "Execution timed out (Limit: {}s)",
                    EXECUTE_TIMEOUT.as_secs()
                )));
            }
        }
    }

    // 所有 Shell 均失败
    Err(NodegetError::Other(format!(
        "All available shells failed to execute command. Last error: {last_error}"
    )))
}
