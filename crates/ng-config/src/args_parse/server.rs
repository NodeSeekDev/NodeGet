//! Server 命令行参数定义与解析，包含子命令分发。

use palc::{Parser, Subcommand};

/// Server 命令行参数结构体。
#[derive(Parser, Debug, Clone)]
#[command(
    version,
    long_about = "NodeGet is the next-generation server monitoring and management tools. nodeget-server is a part of it",
    after_long_help = "This Server is open-sourced on Github, powered by powerful Rust. Love from NodeGet"
)]
/// Server 命令行参数
pub struct ServerArgs {
    /// 子命令（serve / init / roll-super-token / get-uuid / version）
    #[command(subcommand)]
    pub command: ServerCommand,
}

/// Server 子命令枚举。
#[derive(Subcommand, Debug, Clone, Eq, PartialEq)]
pub enum ServerCommand {
    /// 启动服务器正常运行。
    Serve {
        /// 配置文件路径
        #[arg(long, short)]
        config: String,
    },
    /// 初始化数据库和 Super Token，然后退出。
    Init {
        /// 配置文件路径
        #[arg(long, short)]
        config: String,
    },
    /// 交互式确认后轮换 Super Token（id=1），然后退出。
    RollSuperToken {
        /// 配置文件路径
        #[arg(long, short)]
        config: String,
    },
    /// 打印配置中的服务器 UUID，然后退出。
    GetUuid {
        /// 配置文件路径
        #[arg(long, short)]
        config: String,
    },
    /// 打印版本 JSON 信息后退出。
    Version,
}

impl ServerArgs {
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
                .unwrap_or_else(|| "nodeget-server".to_owned());
            if let Err(e) = Self::try_parse_from(vec![bin_name, "-h".to_owned()]) {
                tracing::info!("{e}");
                std::process::exit(0);
            }
        }

        let args = Self::parse();
        // todo: add check
        args
    }

    /// 获取当前子命令关联的配置文件路径。
    ///
    /// `Version` 子命令无配置文件，返回空字符串。
    #[must_use]
    pub const fn config_path(&self) -> &str {
        match &self.command {
            ServerCommand::Serve { config }
            | ServerCommand::Init { config }
            | ServerCommand::RollSuperToken { config }
            | ServerCommand::GetUuid { config } => config.as_str(),
            ServerCommand::Version => "",
        }
    }
}
