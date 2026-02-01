use sha2::{Digest, Sha256};

pub mod generate_token;
pub mod get;
pub mod super_token;

pub fn hash_string(need_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("NODEGET{}", need_hash).as_bytes());
    hex::encode(hasher.finalize())
}

pub fn split_token(full_token: &str) -> Result<(&str, &str), String> {
    full_token
        .split_once(':')
        .ok_or_else(|| "Invalid token format: missing ':' separator".to_string())
}

pub fn split_username_password(full_auth: &str) -> Result<(&str, &str), String> {
    full_auth
        .split_once('|')
        .ok_or_else(|| "Invalid auth format: missing '|' separator".to_string())
}

pub fn parse_token_and_auth(token: &str) -> (Option<String>, Option<String>, Option<String>) {
        if let Ok((u, p)) = split_username_password(token) {
            (None, Some(u.to_string()), Some(p.to_string()))
        } else {
            (Some(token.to_string()), None, None)
        }
}