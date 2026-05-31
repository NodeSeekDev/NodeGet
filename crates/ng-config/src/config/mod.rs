use serde::{Deserialize, Deserializer};
use uuid::Uuid;

// 服务器配置模块
pub mod server;

// Agent 配置模块
pub mod agent;

// 自定义 UUID 反序列化函数
//
// auto_gen 被禁止直接反序列化；持久化替换由 get_and_parse_config 完成。
// 否则尝试解析输入字符串为标准 UUID 格式
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
/// # 参数
/// * `content` — 原始配置文件内容
/// * `key` — 要查找的 key（如 `"server_uuid"` 或 `"agent_uuid"`）
/// * `uuid` — 替换后的 UUID 字符串
pub(crate) fn replace_auto_gen_uuid(content: &str, key: &str, uuid: &str) -> String {
    let mut new_content = String::with_capacity(content.len() + 32);
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            new_content.push_str(line);
            new_content.push('\n');
            continue;
        }
        let key_end = trimmed
            .find(|c: char| c == '=' || c.is_ascii_whitespace())
            .unwrap_or(trimmed.len());
        // 使用 key 长度替代魔法数字，避免 key 改名时代码不同步
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
                // first_char 是 ASCII 单字节字符， slicing at [1..] 安全
                let rest = &after_trimmed[1..];
                // 使用 get(..8) 替代直接索引，防止非 ASCII 边界 panic
                if rest
                    .get(..8)
                    .is_some_and(|s| s.eq_ignore_ascii_case("auto_gen"))
                {
                    let after_value = &rest[8..];
                    new_content.push_str(before);
                    new_content.push(' ');
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
