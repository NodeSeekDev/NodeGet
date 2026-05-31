use anyhow::Context;
use base64::Engine as _;
use ng_core::error::NodegetError;
use ng_db::entity::static_file as static_entity;
use ng_db::get_db;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};
use tracing::{debug, error, warn};

use crate::FileInfo;
use crate::cache::StaticCache;

/// 获取配置文件中的 `static_path`，默认 `./static/`
pub fn get_static_path() -> String {
    ng_config::get_server_config()
        .and_then(|lock| lock.read().ok())
        .map_or_else(
            || "./static/".to_owned(),
            |guard| {
                guard
                    .static_path
                    .clone()
                    .unwrap_or_else(|| "./static/".to_owned())
            },
        )
}

/// 校验 static name 的合法性
///
/// name 只作为 RPC / URL 的标识符，也会顺带落到磁盘提示信息里；
/// 但不会直接拼接磁盘路径（磁盘路径由 `path` 字段决定）。
/// 即便如此，仍严格限制字符集以避免跨层混淆。
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        return Err(NodegetError::InvalidInput("name cannot be empty".to_owned()).into());
    }
    if name.len() > 128 {
        return Err(NodegetError::InvalidInput("name too long (max 128 chars)".to_owned()).into());
    }
    // 只允许字母、数字、下划线、短横线、点。禁止 `..`、`/`、`\` 等所有路径分隔符及控制字符
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if !valid {
        return Err(NodegetError::InvalidInput(
            "name contains invalid characters (only [A-Za-z0-9_.-] are allowed)".to_owned(),
        )
        .into());
    }
    // 显式拒绝 `.` 与 `..`，以及任意全点组合
    if name.chars().all(|c| c == '.') {
        return Err(NodegetError::InvalidInput("name cannot be '.' or '..'".to_owned()).into());
    }
    Ok(())
}

/// 校验 `path`（即 static 记录里的子目录字段）的合法性
///
/// 语义：实际磁盘根 = `{static_path(config)}/{path}`。
/// 允许使用 `/` 作为子目录分隔符（例如 `"sites/blog-2026"`），
/// 但每一段必须通过 [`validate_name`] 等价的字符集校验，不允许
/// 绝对路径、`.` / `..` 穿透、Windows 盘符前缀等。
pub fn validate_sub_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty() {
        return Err(NodegetError::InvalidInput("path cannot be empty".to_owned()).into());
    }
    if path.len() > 512 {
        return Err(NodegetError::InvalidInput("path too long (max 512 chars)".to_owned()).into());
    }
    // 整体粗筛：禁止反斜杠（Windows 路径分隔符），避免歧义
    if path.contains('\\') {
        return Err(NodegetError::InvalidInput("path cannot contain backslash".to_owned()).into());
    }

    let p = Path::new(path);
    let mut has_component = false;
    for component in p.components() {
        match component {
            Component::Normal(c) => {
                let segment = c.to_str().ok_or_else(|| {
                    NodegetError::InvalidInput("path contains non-UTF8 component".to_owned())
                })?;
                // 每段走 name 同款字符集校验
                validate_name(segment)?;
                has_component = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(
                    NodegetError::InvalidInput("path cannot contain '..'".to_owned()).into(),
                );
            }
            Component::RootDir => {
                return Err(NodegetError::InvalidInput(
                    "path cannot be absolute (leading '/')".to_owned(),
                )
                .into());
            }
            Component::Prefix(_) => {
                return Err(NodegetError::InvalidInput(
                    "path cannot contain drive prefix".to_owned(),
                )
                .into());
            }
        }
    }
    if !has_component {
        return Err(NodegetError::InvalidInput("path has no valid component".to_owned()).into());
    }
    Ok(())
}

/// 解析并校验文件路径，防止目录遍历攻击
///
/// 参数语义：
/// - `static_path`: 配置文件中的 `static_path`（总根）
/// - `sub_path`: 某条 static 记录里的 `path` 字段（相对 `static_path` 的子目录）
/// - `file_path`: 相对 `{static_path}/{sub_path}/` 的文件路径
///
/// 返回以 `{static_path}/{sub_path}/` 为基础、拼接 `file_path` 后的安全路径。
///
/// 调用方必须保证 `sub_path` 已通过 [`validate_sub_path`] 校验。
pub fn resolve_safe_file_path(
    static_path: &str,
    sub_path: &str,
    file_path: &str,
) -> anyhow::Result<PathBuf> {
    // 防御性：再次校验 sub_path，避免调用方忘记
    validate_sub_path(sub_path)?;

    let base = Path::new(static_path).join(sub_path);
    let mut resolved = base.clone();

    let path = Path::new(file_path);
    for component in path.components() {
        match component {
            Component::Normal(c) => resolved.push(c),
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    return Err(NodegetError::InvalidInput(
                        "Invalid path: path traversal detected".to_owned(),
                    )
                    .into());
                }
            }
            Component::Prefix(_) => {
                return Err(NodegetError::InvalidInput(
                    "Invalid path: absolute path not allowed".to_owned(),
                )
                .into());
            }
        }
    }

    // 双重校验：resolved 必须在 base 目录树内
    if !resolved.starts_with(&base) {
        return Err(
            NodegetError::InvalidInput("Invalid path: path traversal detected".to_owned()).into(),
        );
    }

    Ok(resolved)
}

pub async fn create_static(
    name: String,
    path: String,
    is_http_root: bool,
    cors: bool,
) -> anyhow::Result<static_entity::Model> {
    let db = get_db().context("DB not initialized")?;
    let name_trimmed = name.trim().to_owned();
    validate_name(&name_trimmed)?;

    let path_trimmed = path.trim().to_owned();
    validate_sub_path(&path_trimmed)?;

    // 检查是否已存在同名 static
    let existing = static_entity::Entity::find()
        .filter(static_entity::Column::Name.eq(&name_trimmed))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(
            NodegetError::DatabaseError(format!("Static '{name_trimmed}' already exists")).into(),
        );
    }

    // is_http_root 只能同时存在一个
    if is_http_root {
        let has_root = static_entity::Entity::find()
            .filter(static_entity::Column::IsHttpRoot.eq(true))
            .one(db)
            .await?;
        if has_root.is_some() {
            return Err(NodegetError::InvalidInput(
                "Another static already has is_http_root enabled".to_owned(),
            )
            .into());
        }
    }

    let active_model = static_entity::ActiveModel {
        name: Set(name_trimmed.clone()),
        path: Set(path_trimmed.clone()),
        is_http_root: Set(is_http_root),
        cors: Set(cors),
        ..Default::default()
    };

    let model = active_model.insert(db).await.map_err(|e| {
        error!(target: "static", name = %name_trimmed, error = %e, "failed to insert static");
        NodegetError::DatabaseError(format!("Failed to create static: {e}"))
    })?;

    // 创建实际磁盘目录：{static_path}/{path}
    let static_path = get_static_path();
    let dir = Path::new(&static_path).join(&path_trimmed);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        warn!(target: "static", dir = %dir.display(), error = %e, "failed to create static directory");
    }

    StaticCache::reload().await?;
    debug!(target: "static", name = %name_trimmed, path = %path_trimmed, "static created");
    Ok(model)
}

pub async fn read_static(name: &str) -> anyhow::Result<Option<static_entity::Model>> {
    let cache = StaticCache::global();
    let model = cache.get_by_name(name).map(|arc| (*arc).clone());
    debug!(target: "static", name = %name, found = model.is_some(), "read_static from cache");
    Ok(model)
}

pub async fn update_static(
    name: String,
    new_path: String,
    new_is_http_root: bool,
    new_cors: bool,
    new_enable: Option<bool>,
) -> anyhow::Result<static_entity::Model> {
    let db = get_db().context("DB not initialized")?;
    let name_trimmed = name.trim().to_owned();
    validate_name(&name_trimmed)?;

    let new_path_trimmed = new_path.trim().to_owned();
    validate_sub_path(&new_path_trimmed)?;

    let model = static_entity::Entity::find()
        .filter(static_entity::Column::Name.eq(&name_trimmed))
        .one(db)
        .await?
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name_trimmed}' not found")))?;

    // is_http_root 只能同时存在一个
    if new_is_http_root && !model.is_http_root {
        let has_root = static_entity::Entity::find()
            .filter(static_entity::Column::IsHttpRoot.eq(true))
            .filter(static_entity::Column::Id.ne(model.id))
            .one(db)
            .await?;
        if has_root.is_some() {
            return Err(NodegetError::InvalidInput(
                "Another static already has is_http_root enabled".to_owned(),
            )
            .into());
        }
    }

    let mut active_model: static_entity::ActiveModel = model.into();
    active_model.path = Set(new_path_trimmed.clone());
    active_model.is_http_root = Set(new_is_http_root);
    active_model.cors = Set(new_cors);
    active_model.enable = Set(new_enable);

    let updated = active_model.update(db).await.map_err(|e| {
        error!(target: "static", name = %name_trimmed, error = %e, "failed to update static");
        NodegetError::DatabaseError(format!("Failed to update static: {e}"))
    })?;

    // 如新 path 对应目录尚不存在则创建；不迁移旧目录的内容
    let static_path = get_static_path();
    let dir = Path::new(&static_path).join(&new_path_trimmed);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        warn!(target: "static", dir = %dir.display(), error = %e, "failed to create static directory");
    }

    StaticCache::reload().await?;
    debug!(target: "static", name = %name_trimmed, path = %new_path_trimmed, "static updated");
    Ok(updated)
}

pub async fn delete_static(name: &str) -> anyhow::Result<()> {
    let db = get_db().context("DB not initialized")?;
    let name_trimmed = name.trim();
    validate_name(name_trimmed)?;

    let model = static_entity::Entity::find()
        .filter(static_entity::Column::Name.eq(name_trimmed))
        .one(db)
        .await?
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name_trimmed}' not found")))?;

    static_entity::Entity::delete_by_id(model.id)
        .exec(db)
        .await?;

    StaticCache::reload().await?;
    debug!(target: "static", name = %name_trimmed, "static deleted");
    Ok(())
}

pub async fn upload_file(
    name: &str,
    file_path: &str,
    body: Option<Vec<u8>>,
    base64_str: Option<String>,
) -> anyhow::Result<()> {
    if body.is_some() && base64_str.is_some() {
        return Err(
            NodegetError::InvalidInput("Cannot provide both body and base64".to_owned()).into(),
        );
    }
    if body.is_none() && base64_str.is_none() {
        return Err(
            NodegetError::InvalidInput("Must provide either body or base64".to_owned()).into(),
        );
    }

    validate_name(name)?;
    // 必须先存在对应的 static 配置，并拿到它的 path 字段
    let model = StaticCache::global()
        .get_by_name(name)
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

    let data = if let Some(b) = body {
        b
    } else {
        let b64 = base64_str.unwrap();
        base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .map_err(|e| NodegetError::InvalidInput(format!("Invalid base64: {e}")))?
    };

    let static_path = get_static_path();
    let resolved = resolve_safe_file_path(&static_path, &model.path, file_path)?;

    if let Some(parent) = resolved.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        warn!(target: "static", path = %parent.display(), error = %e, "failed to create parent directory");
    }

    tokio::fs::write(&resolved, data).await.map_err(|e| {
        error!(target: "static", path = %resolved.display(), error = %e, "failed to write file");
        NodegetError::IoError(format!("Failed to write file: {e}"))
    })?;

    debug!(target: "static", name = %name, sub_path = %model.path, file = %file_path, "file uploaded");
    Ok(())
}

pub async fn read_file(name: &str, file_path: &str) -> anyhow::Result<String> {
    validate_name(name)?;
    let model = StaticCache::global()
        .get_by_name(name)
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

    let static_path = get_static_path();
    let resolved = resolve_safe_file_path(&static_path, &model.path, file_path)?;

    let data = tokio::fs::read(&resolved).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            NodegetError::NotFound(format!("File not found: {file_path}"))
        } else {
            NodegetError::IoError(format!("Failed to read file: {e}"))
        }
    })?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
    debug!(target: "static", name = %name, sub_path = %model.path, file = %file_path, size = data.len(), "file read");
    Ok(encoded)
}

pub async fn delete_file(name: &str, file_path: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    let model = StaticCache::global()
        .get_by_name(name)
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

    let static_path = get_static_path();
    let resolved = resolve_safe_file_path(&static_path, &model.path, file_path)?;

    match tokio::fs::remove_file(&resolved).await {
        Ok(()) => {
            debug!(target: "static", name = %name, sub_path = %model.path, file = %file_path, "file deleted");
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(target: "static", name = %name, sub_path = %model.path, file = %file_path, "file not found, ignoring");
            Ok(())
        }
        Err(e) => {
            error!(target: "static", name = %name, sub_path = %model.path, file = %file_path, error = %e, "failed to delete file");
            Err(NodegetError::IoError(format!("Failed to delete file: {e}")).into())
        }
    }
}

/// 递归列出某个 static 记录目录下所有文件的相对路径、体积和修改时间
///
/// 返回的 [`FileInfo::path`] 以 `/` 作为分隔符（跨平台一致），相对于 `{static_path}/{sub_path}/`。
/// 例如 `[{path:"index.html",size:123,mtime:1715000000000}, {path:"docs/1.md",...}]`（`mtime` 为毫秒）。
///
/// 行为：
/// - 如果磁盘目录不存在，视为空目录返回 `vec![]`，而非报错（static 记录刚建但没上传文件是正常态）。
/// - 不跟随符号链接（防止 symlink 逃逸 static 目录）。
/// - 只列出普通文件，跳过目录、符号链接、socket 等。
/// - 结果按 `path` 字典序排序，保证稳定输出。
pub async fn list_file(name: &str) -> anyhow::Result<Vec<FileInfo>> {
    validate_name(name)?;
    let model = StaticCache::global()
        .get_by_name(name)
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

    let static_path = get_static_path();
    let base = Path::new(&static_path).join(&model.path);

    let files = tokio::task::spawn_blocking(move || collect_files(&base))
        .await
        .map_err(|e| NodegetError::Other(format!("Failed to join file listing task: {e}")))??;

    debug!(target: "static", name = %name, sub_path = %model.path, count = files.len(), "file list produced");
    Ok(files)
}

/// 将一个文件从 `from` 路径移动/重命名为 `to`，两者均相对当前 static 的磁盘子目录。
///
/// 行为：
/// - 源文件不存在 → 返回 [`NodegetError::NotFound`]。
/// - 自动为目标创建缺失的父目录。
/// - 源与目标指向同一路径 → 视作 no-op，返回 Ok。
/// - 跨 static 移动不支持：`from` 与 `to` 都在同一 static 的磁盘根下。
pub async fn rename_file(name: &str, from: &str, to: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    let model = StaticCache::global()
        .get_by_name(name)
        .ok_or_else(|| NodegetError::NotFound(format!("Static '{name}' not found")))?;

    let static_path = get_static_path();
    let from_resolved = resolve_safe_file_path(&static_path, &model.path, from)?;
    let to_resolved = resolve_safe_file_path(&static_path, &model.path, to)?;

    // 源与目标相同 → no-op
    if from_resolved == to_resolved {
        debug!(target: "static", name = %name, sub_path = %model.path, from = %from, to = %to, "rename: source == destination, no-op");
        return Ok(());
    }

    // 确保目标父目录存在
    if let Some(parent) = to_resolved.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        warn!(target: "static", path = %parent.display(), error = %e, "failed to create parent directory for rename");
    }

    tokio::fs::rename(&from_resolved, &to_resolved)
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NodegetError::NotFound(format!("Source file not found: {from}"))
            } else {
                NodegetError::IoError(format!("Failed to rename file: {e}"))
            }
        })?;

    debug!(target: "static", name = %name, sub_path = %model.path, from = %from, to = %to, "file renamed");
    Ok(())
}

/// 列出缓存中所有静态服务配置的 `name` 字段，结果按字典序排序。
///
/// 数据源是 [`StaticCache`]，不访问数据库、不涉及磁盘 I/O。
pub async fn list_all_names() -> Vec<String> {
    let mut names: Vec<String> = StaticCache::global()
        .get_all()
        .iter()
        .map(|m| m.name.clone())
        .collect();
    names.sort();
    debug!(target: "static", count = names.len(), "static name list produced");
    names
}

/// 同步递归收集 `base` 下所有普通文件，返回 [`FileInfo`] 列表。
///
/// 使用显式栈而非递归调用，避免极深目录栈溢出。
fn collect_files(base: &Path) -> anyhow::Result<Vec<FileInfo>> {
    // 目录不存在或不是目录 → 返回空列表（对应 static 记录创建后还没上传文件的情况）
    match std::fs::metadata(base) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Ok(Vec::new()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(NodegetError::IoError(format!("Failed to stat static dir: {e}")).into());
        }
    }

    let mut out: Vec<FileInfo> = Vec::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(base.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(NodegetError::IoError(format!(
                    "Failed to read dir {}: {e}",
                    dir.display()
                ))
                .into());
            }
        };

        for entry in read {
            let entry = entry.map_err(|e| {
                NodegetError::IoError(format!(
                    "Failed to read dir entry in {}: {e}",
                    dir.display()
                ))
            })?;

            // 使用 symlink_metadata 以识别符号链接本身，不跟随
            let meta = match entry.path().symlink_metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!(target: "static", path = %entry.path().display(), error = %e, "skip entry: cannot stat");
                    continue;
                }
            };
            let ft = meta.file_type();

            if ft.is_symlink() {
                // 不跟随符号链接，避免逃逸根目录
                continue;
            }

            let path = entry.path();
            if ft.is_dir() {
                queue.push_back(path);
            } else if ft.is_file() {
                // 构造相对路径，使用 '/' 分隔符；遇到非 UTF-8 段则跳过整个文件
                if let Ok(rel) = path.strip_prefix(base) {
                    let mut parts: Vec<&str> = Vec::new();
                    let mut ok = true;
                    for c in rel.components() {
                        if let Component::Normal(s) = c {
                            if let Some(s) = s.to_str() {
                                parts.push(s);
                            } else {
                                ok = false;
                                break;
                            }
                        } else {
                            // 不预期出现非 Normal 组件（来自 walk 结果），保险起见跳过
                            ok = false;
                            break;
                        }
                    }
                    if ok && !parts.is_empty() {
                        // mtime 不可用（某些文件系统不支持）时置 0，不算致命错误
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| {
                                t.duration_since(std::time::UNIX_EPOCH)
                                    .ok()
                                    .and_then(|d| i64::try_from(d.as_millis()).ok())
                            })
                            .unwrap_or(0);
                        out.push(FileInfo {
                            path: parts.join("/"),
                            size: meta.len(),
                            mtime,
                        });
                    } else if !ok {
                        warn!(target: "static", path = %path.display(), "skip file: non-UTF-8 path component");
                    }
                }
            }
            // 其他类型（socket、fifo 等）跳过
        }
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 生成一个进程内唯一的临时目录路径（不依赖外部 crate）
    fn unique_tempdir() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!(
            "nodeget-static-test-{}-{n}-{ts}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    fn write_file(base: &Path, rel: &str, content: &[u8]) {
        let p = base.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn collect_files_missing_dir_returns_empty() {
        let base = std::env::temp_dir().join("nodeget-static-test-does-not-exist-xyz");
        let files = collect_files(&base).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn collect_files_empty_dir_returns_empty() {
        let base = unique_tempdir();
        let files = collect_files(&base).unwrap();
        assert!(files.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn collect_files_flat_and_nested() {
        let base = unique_tempdir();
        write_file(&base, "index.html", b"<html/>");
        write_file(&base, "docs/1.md", b"# 1");
        write_file(&base, "docs/sub/2.md", b"# 2");
        write_file(&base, "assets/logo.png", b"\x89PNG");

        let files = collect_files(&base).unwrap();
        // 字典序 + 体积
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "assets/logo.png",
                "docs/1.md",
                "docs/sub/2.md",
                "index.html",
            ]
        );
        let sizes: Vec<u64> = files.iter().map(|f| f.size).collect();
        assert_eq!(sizes, vec![4, 3, 3, 7]);
        // mtime：任何合理的文件系统都应返回真实时间戳。
        // 若所有 mtime 都是 0，说明元数据读取或毫秒转换路径全部走了 fallback，
        // 属于实现回归，这里强校验。
        assert!(
            files.iter().any(|f| f.mtime > 0),
            "expected at least one file to have a real mtime, got: {:?}",
            files.iter().map(|f| f.mtime).collect::<Vec<_>>()
        );
        // 非负（i64 永远如此，但作为防御性校验保留）
        assert!(files.iter().all(|f| f.mtime >= 0));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn collect_files_skips_directories_without_files() {
        let base = unique_tempdir();
        std::fs::create_dir_all(base.join("empty_dir/nested")).unwrap();
        write_file(&base, "a.txt", b"a");

        let files = collect_files(&base).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.txt");
        assert_eq!(files[0].size, 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn collect_files_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let base = unique_tempdir();
        let outside = unique_tempdir();
        write_file(&outside, "secret.txt", b"secret");
        write_file(&base, "real.txt", b"real");

        // 在 base 下创建指向 outside 的符号链接
        let link = base.join("link-to-outside");
        symlink(&outside, &link).unwrap();

        let files = collect_files(&base).unwrap();
        // 不应跟随 symlink 进入 outside，也不应把 link 本身列为文件
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "real.txt");
        assert_eq!(files[0].size, 4);

        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
