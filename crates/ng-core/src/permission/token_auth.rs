use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenOrAuth {
    Token(String, String),
    Auth(String, String),
}

impl TokenOrAuth {
    pub fn from_full_token(full_token: &str) -> Result<Self, String> {
        if let Some((key, secret)) = full_token.split_once(':') {
            Ok(Self::Token(key.to_string(), secret.to_string()))
        } else if let Some((username, password)) = full_token.split_once('|') {
            Ok(Self::Auth(username.to_string(), password.to_string()))
        } else {
            Err("Invalid token format: must be 'key:secret' or 'username|password'".to_string())
        }
    }

    #[must_use]
    pub fn token_key(&self) -> Option<&str> {
        match self {
            Self::Token(key, _) => Some(key),
            Self::Auth(_, _) => None,
        }
    }

    #[must_use]
    pub fn token_secret(&self) -> Option<&str> {
        match self {
            Self::Token(_, secret) => Some(secret),
            Self::Auth(_, _) => None,
        }
    }

    #[must_use]
    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(username, _) => Some(username),
        }
    }

    #[must_use]
    pub fn password(&self) -> Option<&str> {
        match self {
            Self::Token(_, _) => None,
            Self::Auth(_, password) => Some(password),
        }
    }

    #[must_use]
    pub const fn is_token(&self) -> bool {
        matches!(self, Self::Token(_, _))
    }

    #[must_use]
    pub const fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_, _))
    }
}
