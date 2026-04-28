use palc::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    version,
    long_about = "NodeGet is the next-generation server monitoring and management tools. nodeget-server is a part of it",
    after_long_help = "This Server is open-sourced on Github, powered by powerful Rust. Love from NodeGet"
)]
pub struct ServerArgs {
    #[command(subcommand)]
    pub command: ServerCommand,
}

#[derive(Subcommand, Debug, Clone, Eq, PartialEq)]
pub enum ServerCommand {
    /// Start server normally.
    Serve {
        #[arg(long, short)]
        config: String,
    },
    /// Initialize database and super token, then exit.
    Init {
        #[arg(long, short)]
        config: String,
    },
    /// Rotate the super token (id = 1) after interactive confirmation, then exit.
    RollSuperToken {
        #[arg(long, short)]
        config: String,
    },
    /// Print server UUID from config and exit.
    GetUuid {
        #[arg(long, short)]
        config: String,
    },
    /// Get Version JSON
    Version,
}

impl ServerArgs {
    #[must_use]
    pub fn par() -> Self {
        if std::env::args_os().len() == 1 {
            let bin_name = std::env::args()
                .next()
                .unwrap_or_else(|| "nodeget-server".to_owned());
            if let Err(e) = Self::try_parse_from(vec![bin_name, "-h".to_owned()]) {
                println!("{e}");
                std::process::exit(0);
            }
        }

        let args = Self::parse();
        // todo: add check
        args
    }

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
