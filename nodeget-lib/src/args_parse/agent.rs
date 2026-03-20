use palc::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    version,
    long_about = "NodeGet is the next-generation server monitoring and management tools. nodeget-agent is a part of it",
    after_long_help = "This Agent is open-sourced on Github, powered by powerful Rust. Love from NodeGet"
)]
pub struct AgentArgs {
    #[arg(long, short)]
    pub config: String,
}

impl AgentArgs {
    pub fn par() -> Self {
        if std::env::args_os().len() == 1 {
            let bin_name = std::env::args()
                .next()
                .unwrap_or_else(|| "nodeget-agent".to_owned());
            if let Err(e) = Self::try_parse_from(vec![bin_name, "-h".to_owned()]) {
                println!("{e}");
                std::process::exit(0);
            }
        }

        let args = Self::parse();
        // todo: add check
        args
    }
}
