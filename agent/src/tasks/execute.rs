//! 命令执行任务模块。
//!
//! 直接执行 `cmd + args`，不提供字符串拼接 shell 的接口。
//! 超时后先 SIGTERM 整个进程组（回收孙子进程），再 SIGKILL 兜底。
//! 输出过长时做头尾双端截断，保证用户同时看到开头与结尾信息。

use crate::config_access::get_agent_config;
use log::error;
use ng_core::error::NodegetError;
use ng_task::ExecuteTask;
use std::fmt::Write;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

/// 命令执行超时时间，1 分钟。
const EXECUTE_TIMEOUT: Duration = Duration::from_mins(1);
/// 超时后等待进程响应 SIGTERM 的时间；超过则 SIGKILL。
#[cfg(unix)]
const GRACE_AFTER_SIGTERM: Duration = Duration::from_secs(2);

/// 命令执行结果类型
pub type Result<T> = std::result::Result<T, NodegetError>;

/// 执行指定的命令。
///
/// - `task` - 结构化命令参数（cmd + args）
///
/// 1. 校验命令非空
/// 2. 创建子进程并设置独立进程组（Unix）
/// 3. 并发读取 stdout/stderr 并等待进程退出
/// 4. 合并输出，超长时做头尾双端 UTF-8 截断
/// 5. 超时时先 SIGTERM 进程组，等待 GRACE 后 SIGKILL
///
/// 成功时返回命令输出字符串；失败或超时时返回错误。
pub async fn execute_command(task: ExecuteTask) -> Result<String> {
    let config = get_agent_config()?;
    // 注意：字段名叫 "character" 但后续 `result.len() > max_chars` 等比较都以 UTF-8
    // 字节长度为口径。多字节语言（中文 3B/字符）下实际能保留的字符数小于 `max_chars`。
    // 截断时用 `is_char_boundary` 避免切碎 UTF-8，因此不会产生无效字符串，只是尺寸
    // 语义不精确。review_agent.md #70 记录为待在 lib 侧统一字段含义后再调整。
    let max_chars = config.exec_max_character_or_default();

    if task.cmd.trim().is_empty() {
        return Err(NodegetError::InvalidInput(
            "Execute command cannot be empty".to_owned(),
        ));
    }

    let mut cmd = Command::new(&task.cmd);
    cmd.args(&task.args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    // 在 Unix 上把子进程放进独立进程组，超时时可以整组信号回收，
    // 避免 shell 脚本 fork 出的孙子进程变孤儿继续消耗资源。
    // pgid 与 child pid 相同（因为 setpgid(0, 0) 等价于 process_group(0)）。
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().map_err(|e| {
        // 内部记录详细错误，但向外部返回通用错误
        error!("Failed to spawn command '{}': {e}", task.cmd);
        NodegetError::Other("Command execution failed".to_owned())
    })?;

    // 取出 stdout/stderr 的 pipe，自己读取。不用 wait_with_output
    // 因为它会 move 掉 child，超时时就没法主动 kill。
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    // 读取上限：单 pipe 最多把 `max_chars` 字节保留进 buf。
    // 最终结果字符串 head+tail 合计 ≤ max_chars，保留更多对结果无任何用途；
    // 上限化避免 `read_to_end` 把失控命令（cat /dev/zero、yes、超大日志）的
    // 全部输出堆进内存导致 agent OOM。
    //
    // **关键**：到上限后必须继续 drain 剩余字节（读到 sink 丢弃），否则 OS 管道
    // 缓冲（典型 64KiB）填满后 child 的 write() 阻塞，导致 child.wait() 永不返回、
    // 任务卡到 EXECUTE_TIMEOUT 才以"超时"错误结束——这会让 head/tail 截断逻辑
    //（下方 116 行起）在输出充足的常见命令上完全失效。drain 保证 child 能正常
    // 退出，截断逻辑正常生效。max_chars 默认 10000（`DEFAULT_EXEC_MAX_CHARACTER`），drain 的总量受 EXECUTE_TIMEOUT
    //（60s）约束，不会无限。
    let max_capture = u64::try_from(max_chars).unwrap_or(u64::MAX);

    let read_stdout = async {
        read_capped(stdout_pipe.as_mut(), max_capture).await
    };
    let read_stderr = async {
        read_capped(stderr_pipe.as_mut(), max_capture).await
    };

    let wait_and_collect = async {
        let (status, out, err) = tokio::join!(child.wait(), read_stdout, read_stderr);
        (status, out, err)
    };

    match timeout(EXECUTE_TIMEOUT, wait_and_collect).await {
        Ok((Ok(status), stdout_buf, stderr_buf)) => {
            let mut result = String::from_utf8_lossy(&stdout_buf).into_owned();
            let stderr = String::from_utf8_lossy(&stderr_buf);

            if !stderr.is_empty() && !result.is_empty() {
                result.push_str("\n--- STDERR ---\n");
            }
            result.push_str(&stderr);

            if result.is_empty() {
                result.push_str("(No Output)");
            }

            if !status.success() {
                use std::fmt::Write;
                let _ = write!(
                    result,
                    "\n\n[Process exited with code {}]",
                    status.code().unwrap_or(-1)
                );
            }

            if result.len() > max_chars {
                let original_len = result.len();
                // 对 stdout+stderr 合并字符串做"头 + 尾"双端截断，让用户同时看到
                // 命令开始时的输出与最终的错误信息，而不是像之前那样只保留尾部
                // max_chars 字节 —— 若 stdout 巨大、stderr 只有几行，原始做法会把
                // stdout 全丢掉，只剩 stderr 的一点尾巴，极易误导。
                //
                // 分配 head = max_chars / 2, tail = max_chars - head，并按 UTF-8
                // 字符边界向内收缩（避免 ceil_char_boundary 这类 unstable API）。
                let head_budget = max_chars / 2;
                let tail_budget = max_chars - head_budget;

                let mut head_end = head_budget.min(original_len);
                while head_end > 0 && !result.is_char_boundary(head_end) {
                    head_end -= 1;
                }

                let tail_start_raw = original_len.saturating_sub(tail_budget);
                let mut tail_start = tail_start_raw.max(head_end);
                while tail_start < original_len && !result.is_char_boundary(tail_start) {
                    tail_start += 1;
                }

                if tail_start <= head_end {
                    // head 与 tail 衔接，整体就是 head_end 之前的内容
                    result.truncate(head_end);
                } else {
                    let tail_part = result[tail_start..].to_owned();
                    let skipped = tail_start - head_end;
                    result.truncate(head_end);
                    let _ = write!(
                        result,
                        "\n[... Output truncated, {skipped} bytes omitted (original {original_len} bytes) ...]\n"
                    );
                    result.push_str(&tail_part);
                }
            }

            Ok(result)
        }
        Ok((Err(e), _, _)) => Err(NodegetError::Other(format!(
            "Failed to wait for process: {e}"
        ))),
        Err(_) => {
            // 超时：Unix 下先 SIGTERM 整个进程组（回收孙子进程），
            // 给 GRACE_AFTER_SIGTERM 秒自愿退出；过期仍存活则 SIGKILL。
            // 非 Unix 平台退回到 kill_on_drop 语义（child drop 时 SIGKILL）。
            #[cfg(unix)]
            {
                if let Some(pid) = child.id() {
                    // pid 永远 > 0，转换成 i32 由 libc::killpg 使用
                    // 对信号发送失败容忍：进程可能已经自己退出
                    #[allow(clippy::cast_possible_wrap)]
                    let pgid = pid as i32;
                    unsafe {
                        libc::killpg(pgid, libc::SIGTERM);
                    }
                }
                if timeout(GRACE_AFTER_SIGTERM, child.wait()).await.is_err() {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            Err(NodegetError::Other(format!(
                "Execution timed out (Limit: {}s)",
                EXECUTE_TIMEOUT.as_secs()
            )))
        }
    }
}

/// 从可选的 reader 读取最多 `max_capture` 字节存入 buf，然后继续 drain 剩余字节到 sink 丢弃。
///
/// 仅保留前 `max_capture` 字节控制内存；drain 防止 OS 管道缓冲填满导致 child 写阻塞
///（见 `execute_command` 中读取上限的注释）。任何读取错误都被吞掉（`read_to_end`
/// / `copy` 返回的 Result 用 `let _ =` 忽略），与原实现"尽力读取"语义一致。
async fn read_capped<R>(reader: Option<&mut R>, max_capture: u64) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let Some(r) = reader else {
        return buf;
    };
    // 先保留前 max_capture 字节。take 返回的 Take 在 read_to_end 到上限后返回，
    // 底层 reader 不变，之后用同一 r 继续 drain。
    let _ = r.take(max_capture).read_to_end(&mut buf).await;
    let _ = tokio::io::copy(r, &mut tokio::io::sink()).await;
    buf
}
