use log::info;

use crate::token::super_token::generate_super_token;

pub mod init;
pub mod roll_super_token;
pub mod serve;

async fn init_or_skip_super_token() {
    let token = match generate_super_token().await {
        Ok(token) => token,
        Err(e) => {
            panic!("Failed to generate super token: {e}");
        }
    };

    match token {
        Some(token) => {
            info!("Super Token: {}", token.0);
            info!("Root Password: {}", token.1);
        }
        None => {
            info!("Super Token already exists, skipped.");
        }
    }
}
