//! 配置文件结构体与解析逻辑模块。
//!
//! 包含 Agent 和 Server 的配置定义，以及 UUID 自动生成（`auto_gen`）的处理工具。

use serde::{Deserialize, Deserializer};
use uuid::Uuid;

// 服务器配置模块
pub mod server;

// Agent 配置模块
pub mod agent;

/// 自定义 UUID 反序列化函数。
///
/// `auto_gen` 被禁止直接反序列化；持久化替换由 `get_and_parse_config` 完成。
/// 否则尝试解析输入字符串为标准 UUID 格式。
pub(crate) fn deserialize_uuid_or_auto<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;

    if s.eq_ignore_ascii_case("auto_gen") {
        Err(serde::de::Error::custom(
            "auto_gen is not supported here; use get_and_parse_config for auto-generation",
        ))
    } else {
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

/// 在 TOML 配置文本中查找并替换指定 key 的 `"auto_gen"` 值为新生成的 UUID。
///
/// 仅处理非注释、非空行，且匹配到 `"auto_gen"`（不区分大小写）开头的字符串值。
/// 如果 key 不存在或值不是 auto_gen，则原样返回内容。
///
/// - `content` — 原始配置文件内容
/// - `key` — 要查找的 key（如 `"server_uuid"` 或 `"agent_uuid"`）
/// - `uuid` — 替换后的 UUID 字符串
/// - 返回替换后的完整配置文本
pub(crate) fn replace_auto_gen_uuid(content: &str, key: &str, uuid: &str) -> String {
    let mut new_content = String::with_capacity(content.len() + 32);
    for line in content.lines() {
        let trimmed = line.trim_start();
        // 跳过注释行和空行，原样保留
        if trimmed.starts_with('#') || trimmed.is_empty() {
            new_content.push_str(line);
            new_content.push('\n');
            continue;
        }
        // 定位 key 结束位置（到等号或空白符为止）
        let key_end = trimmed
            .find(|c: char| c == '=' || c.is_ascii_whitespace())
            .unwrap_or(trimmed.len());
        // 使用 key 长度而非硬编码数字，避免 key 改名时代码不同步
        if key_end == key.len()
            && trimmed[..key_end].eq_ignore_ascii_case(key)
            && let Some(eq_pos) = line.find('=')
        {
            let before = &line[..=eq_pos];
            let after = &line[eq_pos + 1..];
            let after_trimmed = after.trim_start();
            if let Some(first_char) = after_trimmed.chars().next()
                && (first_char == '"' || first_char == '\'')
            {
                // first_char 是 ASCII 单字节引号字符，slicing at [1..] 安全
                let rest = &after_trimmed[1..];
                // 使用 get(..8) 替代直接索引 [..8]，防止非 ASCII 边界 panic
                if rest
                    .get(..8)
                    .is_some_and(|s| s.eq_ignore_ascii_case("auto_gen"))
                {
                    let after_value = &rest[8..];
                    new_content.push_str(before);
                    new_content.push(first_char);
                    new_content.push_str(uuid);
                    new_content.push_str(after_value);
                    new_content.push('\n');
                    continue;
                }
            }
        }
        new_content.push_str(line);
        new_content.push('\n');
    }
    new_content
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── deserialize_uuid_or_auto ────────────────────────────────────────

    #[test]
    fn deserialize_valid_uuid_succeeds() {
        let uuid_str = "\"550e8400-e29b-41d4-a716-446655440000\"";
        let result: Result<uuid::Uuid, _> = serde_json::from_str(uuid_str);
        assert!(result.is_ok());
        let parsed: uuid::Uuid = result.unwrap();
        assert_eq!(
            parsed,
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
    }

    #[test]
    fn deserialize_auto_gen_is_rejected() {
        // deserialize_uuid_or_auto rejects "auto_gen" — the deserializer is
        // used via serde, so we wrap it in a struct.
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Wrap {
            #[serde(deserialize_with = "deserialize_uuid_or_auto")]
            uuid: uuid::Uuid,
        }
        let toml_str = r#"uuid = "auto_gen""#;
        let result: Result<Wrap, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_auto_gen_case_insensitive_rejected() {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Wrap {
            #[serde(deserialize_with = "deserialize_uuid_or_auto")]
            uuid: uuid::Uuid,
        }
        let toml_str = r#"uuid = "AUTO_GEN""#;
        let result: Result<Wrap, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_invalid_uuid_fails() {
        let uuid_str = "\"not-a-valid-uuid\"";
        let result: Result<uuid::Uuid, _> = serde_json::from_str(uuid_str);
        assert!(result.is_err());
    }

    // ── replace_auto_gen_uuid ───────────────────────────────────────────

    #[test]
    fn replace_auto_gen_replaces_double_quoted() {
        let content = "server_uuid = \"auto_gen\"\nws_listener = \"0.0.0.0:3000\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!result.contains("auto_gen"));
        assert!(result.contains("ws_listener"));
    }

    #[test]
    fn replace_auto_gen_replaces_single_quoted() {
        let content = "agent_uuid = 'auto_gen'\n";
        let result = replace_auto_gen_uuid(content, "agent_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn replace_auto_gen_preserves_comments() {
        let content = "# this is a comment\nserver_uuid = \"auto_gen\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("# this is a comment"));
    }

    #[test]
    fn replace_auto_gen_preserves_empty_lines() {
        let content = "\nserver_uuid = \"auto_gen\"\n\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn replace_auto_gen_no_match_returns_unchanged() {
        let content = "server_uuid = \"550e8400-e29b-41d4-a716-446655440000\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "new-uuid");
        assert_eq!(result, content);
    }

    #[test]
    fn replace_auto_gen_case_insensitive() {
        let content = "server_uuid = \"AUTO_GEN\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn replace_auto_gen_key_case_insensitive() {
        let content = "SERVER_UUID = \"auto_gen\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn replace_auto_gen_preserves_leading_whitespace() {
        let content = "  server_uuid = \"auto_gen\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert!(result.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn replace_auto_gen_ignores_non_auto_gen_value() {
        let content = "server_uuid = \"some-other-value\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(result, content);
    }

    #[test]
    fn replace_auto_gen_wrong_key_ignored() {
        let content = "agent_uuid = \"auto_gen\"\n";
        let result = replace_auto_gen_uuid(content, "server_uuid", "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(result, content);
    }
}
