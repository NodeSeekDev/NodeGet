//! UUID 生成工具

use crate::error::Result;
use uuid::Uuid;

/// 生成随机 UUID（v4）。
///
/// - 返回新生成的 UUID，当前实现不会失败
pub fn generate_random_uuid() -> Result<Uuid> {
    Ok(Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::generate_random_uuid;
    use uuid::Uuid;

    #[test]
    fn generate_random_uuid_returns_v4() {
        let id = generate_random_uuid().unwrap();
        assert_eq!(id.get_version(), Some(uuid::Version::Random));
    }

    #[test]
    fn generate_random_uuid_returns_unique() {
        let a = generate_random_uuid().unwrap();
        let b = generate_random_uuid().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn generate_random_uuid_valid_format() {
        let id = generate_random_uuid().unwrap();
        // Should be parseable from string representation
        let s = id.to_string();
        let parsed: Uuid = s.parse().unwrap();
        assert_eq!(id, parsed);
    }
}
