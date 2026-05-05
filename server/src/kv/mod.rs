use anyhow::{Context, Result};
use nodeget_lib::error::NodegetError;
use nodeget_lib::kv::KVStore;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set,
};
use serde_json::Value;
use tracing::{debug, error, warn};

use crate::DB;
use crate::entity::kv;

pub const NAMESPACE_MARKER_KEY: &str = "__nodeget_namespace_marker__";

/// 获取数据库连接
fn get_db() -> Result<&'static DatabaseConnection> {
    DB.get().context("DB not initialized")
}

/// 检查命名空间是否存在
async fn namespace_exists(db: &DatabaseConnection, namespace: &str) -> Result<bool> {
    let count = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(namespace))
        .count(db)
        .await?;
    let exists = count > 0;
    debug!(target: "kv", namespace = %namespace, exists, "namespace_exists check");
    Ok(exists)
}

/// 确保命名空间存在
async fn ensure_namespace_exists(db: &DatabaseConnection, namespace: &str) -> Result<()> {
    if namespace_exists(db, namespace).await? {
        return Ok(());
    }

    warn!(target: "kv", namespace = %namespace, "Namespace not found");
    Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
}

/// 创建一个新的 KV 存储命名空间
///
/// # 参数
/// * `namespace` - 命名空间名称，作为数据库表中的唯一标识
///
/// # 返回值
/// 成功时返回创建的 KVStore，失败返回错误
pub async fn create_kv(namespace: String) -> Result<KVStore> {
    let db = get_db()?;
    let namespace_name = namespace.clone();

    // 检查命名空间是否已存在
    if namespace_exists(db, &namespace).await? {
        debug!(target: "kv", namespace = %namespace, "create_kv: namespace already exists");
        return Err(
            NodegetError::DatabaseError(format!("Namespace '{namespace}' already exists")).into(),
        );
    }

    // 单表模式：写入一条内部 marker 表示 namespace 已创建
    let active_model = kv::ActiveModel {
        namespace: Set(namespace.clone()),
        key: Set(NAMESPACE_MARKER_KEY.to_owned()),
        value: Set(Value::Null),
        ..Default::default()
    };
    if let Err(e) = active_model.insert(db).await {
        error!(target: "kv", namespace = %namespace, error = %e, "create_kv: failed to insert namespace marker");
        return Err(e.into());
    }

    debug!(target: "kv", namespace = %namespace_name, "namespace created");
    Ok(KVStore::new(namespace_name))
}

/// 从 KV 存储中获取指定 key 的值
///
/// # 参数
/// * `namespace` - 命名空间名称
/// * `key` - 要查找的键
///
/// # 返回值
/// 成功时返回对应的值（如果存在），失败返回错误
pub async fn get_v_from_kv(namespace: String, key: String) -> Result<Option<Value>> {
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    let model = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(&namespace))
        .filter(kv::Column::Key.eq(&key))
        .one(db)
        .await?;

    let found = model.is_some();
    debug!(target: "kv", namespace = %namespace, key = %key, found, "get_v_from_kv completed");
    Ok(model.map(|record| record.value))
}

/// 设置 KV 存储中指定 key 的值
///
/// # 参数
/// * `namespace` - 命名空间名称
/// * `key` - 要设置的键
/// * `value` - 要设置的值（任意 JSON 类型）
///
/// # 返回值
/// 成功时返回 ()，失败返回错误
pub async fn set_v_to_kv(namespace: String, key: String, value: Value) -> Result<()> {
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    let model = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(&namespace))
        .filter(kv::Column::Key.eq(&key))
        .one(db)
        .await?;

    if let Some(record) = model {
        let active_model = kv::ActiveModel {
            id: Set(record.id),
            namespace: Set(record.namespace),
            key: Set(record.key),
            value: Set(value),
        };

        active_model.update(db).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "kv key updated");
    } else {
        let active_model = kv::ActiveModel {
            namespace: Set(namespace.clone()),
            key: Set(key.clone()),
            value: Set(value),
            ..Default::default()
        };

        active_model.insert(db).await?;
        debug!(target: "kv", namespace = %namespace, key = %key, "kv key inserted");
    }
    Ok(())
}

/// 获取或创建 KV 存储（如果不存在则创建）
///
/// # 参数
/// * `namespace` - 命名空间名称
///
/// # 返回值
/// 成功时返回 KVStore，失败返回错误
pub async fn get_or_create_kv(namespace: String) -> Result<KVStore> {
    let db = get_db()?;

    if namespace_exists(db, &namespace).await? {
        debug!(target: "kv", namespace = %namespace, "get_or_create_kv: namespace exists, fetching store");
        return get_kv_store(namespace).await;
    }

    debug!(target: "kv", namespace = %namespace, "get_or_create_kv: namespace not found, creating");
    create_kv(namespace).await
}

/// 删除 KV 存储中的指定 key
///
/// # 参数
/// * `namespace` - 命名空间名称
/// * `key` - 要删除的键
///
/// # 返回值
/// 成功时返回 ()，失败返回错误
pub async fn delete_key_from_kv(namespace: String, key: String) -> Result<()> {
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    kv::Entity::delete_many()
        .filter(kv::Column::Namespace.eq(&namespace))
        .filter(kv::Column::Key.eq(&key))
        .exec(db)
        .await?;

    debug!(target: "kv", namespace = %namespace, key = %key, "kv key deleted");
    Ok(())
}

/// 删除整个 KV 命名空间
///
/// # 参数
/// * `namespace` - 命名空间名称
///
/// # 返回值
/// 成功时返回 ()，失败返回错误
pub async fn delete_kv(namespace: String) -> Result<()> {
    debug!(target: "kv", namespace = %namespace, "Deleting namespace");
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    kv::Entity::delete_many()
        .filter(kv::Column::Namespace.eq(&namespace))
        .exec(db)
        .await?;

    debug!(target: "kv", namespace = %namespace, "namespace deleted");
    Ok(())
}

/// 获取 KV 存储中的所有 keys
///
/// # 参数
/// * `namespace` - 命名空间名称
///
/// # 返回值
/// 成功时返回 key 列表，失败返回错误
pub async fn get_keys_from_kv(namespace: String) -> Result<Vec<String>> {
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    let models = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(&namespace))
        .order_by_asc(kv::Column::Key)
        .all(db)
        .await?;

    let keys_count = models.len();
    debug!(target: "kv", namespace = %namespace, keys_count, "get_keys_from_kv completed");
    Ok(models.into_iter().map(|model| model.key).collect())
}

/// 获取完整的 `KVStore`
///
/// # 参数
/// * `namespace` - 命名空间名称
///
/// # 返回值
/// 成功时返回 KVStore，失败返回错误
pub async fn get_kv_store(namespace: String) -> Result<KVStore> {
    let db = get_db()?;
    ensure_namespace_exists(db, &namespace).await?;

    let models = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(&namespace))
        .order_by_asc(kv::Column::Key)
        .all(db)
        .await?;

    let entries_count = models.len();
    let mut kv_store = KVStore::new(namespace.clone());
    for model in models {
        kv_store.set(model.key, model.value);
    }

    debug!(target: "kv", namespace = %namespace, entries_count, "get_kv_store completed");
    Ok(kv_store)
}

/// 获取完整的 `KVStore`，namespace 不存在时返回 None 而非报错
pub async fn get_kv_store_optional(namespace: String) -> Result<Option<KVStore>> {
    let db = get_db()?;
    if !namespace_exists(db, &namespace).await? {
        return Ok(None);
    }

    let models = kv::Entity::find()
        .filter(kv::Column::Namespace.eq(&namespace))
        .order_by_asc(kv::Column::Key)
        .all(db)
        .await?;

    let entries_count = models.len();
    let mut kv_store = KVStore::new(namespace.clone());
    for model in models {
        kv_store.set(model.key, model.value);
    }

    debug!(target: "kv", namespace = %namespace, entries_count, "get_kv_store_optional completed");
    Ok(Some(kv_store))
}

/// 列出所有 KV 命名空间
///
/// # 返回值
/// 成功时返回命名空间列表，失败返回错误
pub async fn list_all_namespaces() -> Result<Vec<String>> {
    let db = get_db()?;

    let namespaces: Vec<String> = kv::Entity::find()
        .select_only()
        .column(kv::Column::Namespace)
        .distinct()
        .order_by_asc(kv::Column::Namespace)
        .into_tuple()
        .all(db)
        .await?;

    debug!(target: "kv", count = namespaces.len(), "list_all_namespaces completed");
    Ok(namespaces)
}
