//! Agent 命令行参数定义与解析。

use crate::config::agent::DEFAULT_AGENT_CONFIG_PATH;
use palc::Parser;

/// Agent 命令行参数结构体。
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    long_about = "NodeGet is the next-generation server monitoring and management tools. nodeget-agent is a part of it",
    after_long_help = "This Agent is open-sourced on Github, powered by powerful Rust. Love from NodeGet"
)]
/// Agent 命令行参数
pub struct AgentArgs {
    /// 配置文件路径，默认 `config.toml`
    #[arg(long, short, default_value_t = DEFAULT_AGENT_CONFIG_PATH.to_owned())]
    pub config: String,

    /// 打印版本信息后退出
    #[arg(long, short, default_value_t = false)]
    pub version: bool,

    /// 干跑模式：仅解析配置并打印，不实际启动 Agent
    #[arg(long, short, default_value_t = false)]
    pub dry_run: bool,
}

impl AgentArgs {
    /// 解析命令行参数。
    ///
    /// 若未传入任何参数，自动显示帮助信息后退出，内部步骤：
    /// 1. 检查参数数量是否为 1（仅程序名）
    /// 2. 若无参数，构造 `-h` 命令并解析以显示帮助
    /// 3. 解析实际参数并返回
    #[must_use]
    pub fn par() -> Self {
        if std::env::args_os().len() == 1 {
            // 无参数时自动显示帮助
            let bin_name = std::env::args()
                .next()
                .unwrap_or_else(|| "nodeget-agent".to_owned());
            if let Err(e) = Self::try_parse_from(vec![bin_name, "-h".to_owned()]) {
                tracing::info!("{e}");
                std::process::exit(0);
            }
        }

        let args = Self::parse();
        // todo: add check
        args
    }
}
