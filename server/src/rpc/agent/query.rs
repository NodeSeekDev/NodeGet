use crate::entity::{dynamic_monitoring, static_monitoring};
use crate::rpc::RpcHelper;
use crate::rpc::agent::AgentRpcImpl;
use crate::token::get::check_token_limit;
use futures::StreamExt;
use jsonrpsee::core::RpcResult;
use log::error;
use nodeget_lib::monitoring::query::{
    DynamicDataQuery, DynamicDataQueryField, QueryCondition, StaticDataQuery, StaticDataQueryField,
};
use nodeget_lib::permission::data_structure::{
    DynamicMonitoring, Permission, Scope, StaticMonitoring,
};
use nodeget_lib::utils::error_message::error_to_raw;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, ExprTrait, Order, QueryFilter, QueryOrder,
    QuerySelect, SelectModel, Selector,
};
use serde_json::Value;
use serde_json::value::RawValue;
use nodeget_lib::permission::token_auth::TokenOrAuth;
use nodeget_lib::utils::server_json::rename_and_fix_json;

// 查询静态监控数据
// 
// # 参数
// * `token` - 认证令牌
// * `static_data_query` - 静态数据查询条件
// 
// # 返回值
// 返回查询结果，格式为 Vec<StaticResponseItem> 的原始 JSON 值
pub async fn query_static(
    token: String,
    static_data_query: StaticDataQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 鉴权
        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(toa) => {
                toa
            }
            Err(e) => {
                return Err((101, format!("Failed to parse token: {e}")))
            }
        };

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;

        for cond in &static_data_query.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }

        // 没有指定 UUID，说明是全量查询，需要 Global 权限
        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let permissions: Vec<Permission> = static_data_query
            .fields
            .iter()
            .map(|field| {
                Permission::StaticMonitoring(StaticMonitoring::Read(match field {
                    StaticDataQueryField::Cpu => StaticDataQueryField::Cpu,
                    StaticDataQueryField::System => StaticDataQueryField::System,
                    StaticDataQueryField::Gpu => StaticDataQueryField::Gpu,
                }))
            })
            .collect();

        let is_allowed =
            check_token_limit(&token_or_auth, scopes, permissions).await?;

        if !is_allowed {
            return Err((
                102,
                "Permission Denied: Insufficient StaticMonitoring Read permissions".to_string(),
            ));
        }

        // 查询
        let db = AgentRpcImpl::get_db()?;

        let query = static_monitoring::Entity::find()
            .select_only()
            .column(static_monitoring::Column::Uuid)
            .column(static_monitoring::Column::Timestamp);

        let query = static_data_query
            .fields
            .iter()
            .fold(query, |q, field| match field {
                StaticDataQueryField::Cpu => q.column(static_monitoring::Column::CpuData),
                StaticDataQueryField::System => q.column(static_monitoring::Column::SystemData),
                StaticDataQueryField::Gpu => q.column(static_monitoring::Column::GpuData),
            });

        let mut limit_count = None;
        let mut is_last = false;

        let query = static_data_query
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(uuid) => q.filter(static_monitoring::Column::Uuid.eq(uuid)),
                QueryCondition::TimestampFromTo(start, end) => q.filter(
                    static_monitoring::Column::Timestamp
                        .gte(start)
                        .and(static_monitoring::Column::Timestamp.lte(end)),
                ),
                QueryCondition::TimestampFrom(start) => {
                    q.filter(static_monitoring::Column::Timestamp.gte(start))
                }
                QueryCondition::TimestampTo(end) => {
                    q.filter(static_monitoring::Column::Timestamp.lte(end))
                }
                QueryCondition::Limit(n) => {
                    limit_count = Some(n);
                    q
                }
                QueryCondition::Last => {
                    is_last = true;
                    q
                }
            });

        let query = if is_last {
            query
                .order_by(static_monitoring::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = limit_count {
            query
                .order_by(static_monitoring::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query.order_by(static_monitoring::Column::Timestamp, Order::Asc)
        };

        // 字段映射表
        let field_mappings = [
            ("cpu_data", "cpu"),
            ("system_data", "system"),
            ("gpu_data", "gpu"),
        ];

        execute_query(
            db,
            query.into_json(),
            &field_mappings,
            limit_count.unwrap_or(100),
        )
        .await
    };

    Ok(process_logic
        .await
        .unwrap_or_else(|(code, msg)| error_to_raw(code, &msg)))
}

// 查询动态监控数据
// 
// # 参数
// * `token` - 认证令牌
// * `dynamic_data_query` - 动态数据查询条件
// 
// # 返回值
// 返回查询结果，格式为 Vec<DynamicResponseItem> 的原始 JSON 值
pub async fn query_dynamic(
    token: String,
    dynamic_data_query: DynamicDataQuery,
) -> RpcResult<Box<RawValue>> {
    let process_logic = async {
        // 鉴权
        let token_or_auth = match TokenOrAuth::from_full_token(&token) {
            Ok(toa) => {
                toa
            }
            Err(e) => {
                return Err((101, format!("Failed to parse token: {e}")))
            }
        };

        let mut scopes = Vec::new();
        let mut has_uuid_condition = false;

        for cond in &dynamic_data_query.condition {
            if let QueryCondition::Uuid(uuid) = cond {
                scopes.push(Scope::AgentUuid(*uuid));
                has_uuid_condition = true;
            }
        }

        if !has_uuid_condition {
            scopes.push(Scope::Global);
        }

        let permissions: Vec<Permission> = dynamic_data_query
            .fields
            .iter()
            .map(|field| {
                Permission::DynamicMonitoring(DynamicMonitoring::Read(match field {
                    DynamicDataQueryField::Cpu => DynamicDataQueryField::Cpu,
                    DynamicDataQueryField::Ram => DynamicDataQueryField::Ram,
                    DynamicDataQueryField::Load => DynamicDataQueryField::Load,
                    DynamicDataQueryField::System => DynamicDataQueryField::System,
                    DynamicDataQueryField::Disk => DynamicDataQueryField::Disk,
                    DynamicDataQueryField::Network => DynamicDataQueryField::Network,
                    DynamicDataQueryField::Gpu => DynamicDataQueryField::Gpu,
                }))
            })
            .collect();

        let is_allowed =
            check_token_limit(&token_or_auth, scopes, permissions).await?;

        if !is_allowed {
            return Err((
                102,
                "Permission Denied: Insufficient DynamicMonitoring Read permissions".to_string(),
            ));
        }

        // 查询
        let db = AgentRpcImpl::get_db()?;

        // 构建查询
        let query = dynamic_monitoring::Entity::find()
            .select_only()
            .column(dynamic_monitoring::Column::Uuid)
            .column(dynamic_monitoring::Column::Timestamp);

        let query = dynamic_data_query
            .fields
            .iter()
            .fold(query, |q, field| match field {
                DynamicDataQueryField::Cpu => q.column(dynamic_monitoring::Column::CpuData),
                DynamicDataQueryField::Ram => q.column(dynamic_monitoring::Column::RamData),
                DynamicDataQueryField::Load => q.column(dynamic_monitoring::Column::LoadData),
                DynamicDataQueryField::System => q.column(dynamic_monitoring::Column::SystemData),
                DynamicDataQueryField::Disk => q.column(dynamic_monitoring::Column::DiskData),
                DynamicDataQueryField::Network => q.column(dynamic_monitoring::Column::NetworkData),
                DynamicDataQueryField::Gpu => q.column(dynamic_monitoring::Column::GpuData),
            });

        let mut limit_count = None;
        let mut is_last = false;

        let query = dynamic_data_query
            .condition
            .into_iter()
            .fold(query, |q, cond| match cond {
                QueryCondition::Uuid(uuid) => q.filter(dynamic_monitoring::Column::Uuid.eq(uuid)),
                QueryCondition::TimestampFromTo(start, end) => q.filter(
                    dynamic_monitoring::Column::Timestamp
                        .gte(start)
                        .and(dynamic_monitoring::Column::Timestamp.lte(end)),
                ),
                QueryCondition::TimestampFrom(start) => {
                    q.filter(dynamic_monitoring::Column::Timestamp.gte(start))
                }
                QueryCondition::TimestampTo(end) => {
                    q.filter(dynamic_monitoring::Column::Timestamp.lte(end))
                }
                QueryCondition::Limit(n) => {
                    limit_count = Some(n);
                    q
                }
                QueryCondition::Last => {
                    is_last = true;
                    q
                }
            });

        let query = if is_last {
            query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(1)
        } else if let Some(l) = limit_count {
            query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(l)
        } else {
            query.order_by(dynamic_monitoring::Column::Timestamp, Order::Asc)
        };

        // 字段映射表
        let field_mappings = [
            ("cpu_data", "cpu"),
            ("ram_data", "ram"),
            ("load_data", "load"),
            ("system_data", "system"),
            ("disk_data", "disk"),
            ("network_data", "network"),
            ("gpu_data", "gpu"),
        ];

        execute_query(
            db,
            query.into_json(),
            &field_mappings,
            limit_count.unwrap_or(5000),
        )
        .await
    };

    Ok(process_logic
        .await
        .unwrap_or_else(|(code, msg)| error_to_raw(code, &msg)))
}

// 流式读取、JSON 重命名和序列化
// 
// # 参数
// * `db` - 数据库连接引用
// * `query` - 查询选择器
// * `field_mappings` - 字段映射表，用于将数据库字段名映射为用户友好的字段名
// * `capacity_hint` - 输出缓冲区容量提示
// 
// # 返回值
// 返回序列化后的原始 JSON 值，失败时返回错误代码和消息
async fn execute_query(
    db: &DatabaseConnection,
    query: Selector<SelectModel<Value>>,
    field_mappings: &[(&str, &str)],
    capacity_hint: u64,
) -> Result<Box<RawValue>, (i64, String)> {
    let mut stream = query.stream(db).await.map_err(|e| {
        error!("Database query error: {e}");
        (103, format!("Database query error: {e}"))
    })?;

    let capacity = capacity_hint as usize * 200;
    let mut output_buffer: Vec<u8> = Vec::with_capacity(capacity);

    output_buffer.push(b'[');
    let mut first = true;

    while let Some(item_res) = stream.next().await {
        match item_res {
            Ok(mut v) => {
                if let Some(obj) = v.as_object_mut() {
                    for (old_key, new_key) in field_mappings {
                        rename_and_fix_json(obj, old_key, new_key);
                    }
                }

                if first {
                    first = false;
                } else {
                    output_buffer.push(b',');
                }

                if let Err(e) = serde_json::to_writer(&mut output_buffer, &v) {
                    error!("Serialization failed: {e}");
                    return Err((101, format!("Serialization failed: {e}")));
                }
            }
            Err(e) => {
                error!("Stream read error: {e}");
                return Err((103, format!("Stream read error: {e}")));
            }
        }
    }

    output_buffer.push(b']');

    let json_string = String::from_utf8(output_buffer).map_err(|e| {
        error!("UTF8 conversion error: {e}");
        (101, "UTF8 conversion error (internal)".to_string())
    })?;

    let raw_value = RawValue::from_string(json_string).map_err(|e| {
        error!("RawValue creation error: {e}");
        (101, "RawValue creation error".to_string())
    })?;

    Ok(raw_value)
}
