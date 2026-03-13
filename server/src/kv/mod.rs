use anyhow::{Context, Result};
use nodeget_lib::error::NodegetError;
use nodeget_lib::kv::KVStore;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde_json::Value;

use crate::DB;
use crate::entity::kv;

/// 获取数据库连接
fn get_db() -> Result<&'static DatabaseConnection> {
    DB.get().context("DB not initialized")
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

    // 检查命名空间是否已存在
    let existing = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    if existing.is_some() {
        return Err(
            NodegetError::DatabaseError(format!("Namespace '{namespace}' already exists")).into(),
        );
    }

    // 创建新的 KVStore
    let kv_store = KVStore::new(namespace.clone());

    // 插入到数据库
    let active_model = kv::ActiveModel {
        name: Set(namespace),
        kv_value: Set(serde_json::to_value(&kv_store)?),
        ..Default::default()
    };

    active_model.insert(db).await?;

    Ok(kv_store)
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

    // 查找命名空间
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            let kv_store: KVStore = serde_json::from_value(record.kv_value)?;
            Ok(kv_store.get(&key).cloned())
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
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

    // 查找命名空间
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            // 反序列化现有的 KVStore
            let mut kv_store: KVStore = serde_json::from_value(record.kv_value)?;

            // 设置新的值
            kv_store.set(key, value);

            // 更新数据库
            let active_model = kv::ActiveModel {
                id: Set(record.id),
                name: Set(record.name),
                kv_value: Set(serde_json::to_value(&kv_store)?),
            };

            active_model.update(db).await?;
            Ok(())
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
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

    // 尝试查找现有记录
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            let kv_store: KVStore = serde_json::from_value(record.kv_value)?;
            Ok(kv_store)
        }
        None => {
            // 不存在则创建新的
            create_kv(namespace).await
        }
    }
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

    // 查找命名空间
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            // 反序列化现有的 KVStore
            let mut kv_store: KVStore = serde_json::from_value(record.kv_value)?;

            // 删除 key
            kv_store.remove(&key);

            // 更新数据库
            let active_model = kv::ActiveModel {
                id: Set(record.id),
                name: Set(record.name),
                kv_value: Set(serde_json::to_value(&kv_store)?),
            };

            active_model.update(db).await?;
            Ok(())
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
}

/// 删除整个 KV 命名空间
///
/// # 参数
/// * `namespace` - 命名空间名称
///
/// # 返回值
/// 成功时返回 ()，失败返回错误
pub async fn delete_kv(namespace: String) -> Result<()> {
    let db = get_db()?;

    // 查找并删除
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            let active_model: kv::ActiveModel = record.into();
            active_model.delete(db).await?;
            Ok(())
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
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

    // 查找命名空间
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            let kv_store: KVStore = serde_json::from_value(record.kv_value)?;
            Ok(kv_store.keys())
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
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

    // 查找命名空间
    let model = kv::Entity::find()
        .filter(kv::Column::Name.eq(&namespace))
        .one(db)
        .await?;

    match model {
        Some(record) => {
            let kv_store: KVStore = serde_json::from_value(record.kv_value)?;
            Ok(kv_store)
        }
        None => {
            Err(NodegetError::DatabaseError(format!("Namespace '{namespace}' not found")).into())
        }
    }
}

/// 列出所有 KV 命名空间
///
/// # 返回值
/// 成功时返回命名空间列表，失败返回错误
pub async fn list_all_namespaces() -> Result<Vec<String>> {
    let db = get_db()?;

    let models = kv::Entity::find()
        .order_by_asc(kv::Column::Name)
        .all(db)
        .await?;

    Ok(models.into_iter().map(|model| model.name).collect())
}
