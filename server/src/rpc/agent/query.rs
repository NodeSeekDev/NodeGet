use crate::entity::{dynamic_monitoring, static_monitoring};
use crate::rpc::agent::AgentRpcImpl;
use log::error;
use nodeget_lib::monitoring::query::{
    DynamicDataQuery, DynamicDataQueryField, QueryCondition, StaticDataQuery, StaticDataQueryField,
};
use nodeget_lib::utils::error_message::generate_error_message;
use sea_orm::{
    ColumnTrait, EntityTrait, ExprTrait, Order, QueryOrder,
    QuerySelect,
};
use sea_orm::{QueryFilter};
use serde_json::{Map, Value, from_value};

pub async fn query_static(_token: String, data: Value) -> Value {
    let process_logic = async {
        let db = AgentRpcImpl::get_db()?;

        let query_req: StaticDataQuery = from_value(data).map_err(|e| {
            error!("Unable to parse query data: {e}");
            (101, format!("Unable to parse query data: {e}"))
        })?;

        // 查询构建器
        let mut query = static_monitoring::Entity::find();

        // 最新数据 (仅一个)
        let mut is_last = false;

        // 应用过滤条件 (QueryCondition)
        for cond in query_req.condition {
            match cond {
                QueryCondition::Uuid(uuid) => {
                    query = query.filter(static_monitoring::Column::Uuid.eq(uuid));
                }
                QueryCondition::TimestampFromTo(start, end) => {
                    query = query.filter(
                        static_monitoring::Column::Timestamp
                            .gte(start)
                            .and(static_monitoring::Column::Timestamp.lte(end)),
                    );
                }
                QueryCondition::TimestampFrom(start) => {
                    query = query.filter(static_monitoring::Column::Timestamp.gte(start));
                }
                QueryCondition::TimestampTo(end) => {
                    query = query.filter(static_monitoring::Column::Timestamp.lte(end));
                }
                QueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        // 时间倒序第一条
        if is_last {
            query = query
                .order_by(static_monitoring::Column::Timestamp, Order::Desc)
                .limit(1);
        } else {
            query = query.order_by(static_monitoring::Column::Timestamp, Order::Asc);
        }

        // 查询
        let models = query.all(db).await.map_err(|e| {
            error!("Database query error: {e}");
            (103, format!("Database query error: {e}"))
        })?;

        let result_list: Vec<Value> = models
            .into_iter()
            .map(|model| {
                let mut map = Map::new();

                map.insert("uuid".to_string(), Value::String(model.uuid));
                map.insert(
                    "timestamp".to_string(),
                    Value::Number(model.timestamp.into()),
                );

                for field in &query_req.fields {
                    match field {
                        StaticDataQueryField::Cpu => {
                            map.insert("cpu".to_string(), model.cpu_data.clone());
                        }
                        StaticDataQueryField::System => {
                            map.insert("system".to_string(), model.system_data.clone());
                        }
                        StaticDataQueryField::Gpu => {
                            map.insert("gpu".to_string(), model.gpu_data.clone());
                        }
                    }
                }

                Value::Object(map)
            })
            .collect();

        Ok(result_list)
    };

    match process_logic.await {
        Ok(results) => Value::Array(results),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}

pub async fn query_dynamic(_token: String, data: Value) -> Value {
    let process_logic = async {
        // 1. 获取数据库连接
        let db = AgentRpcImpl::get_db()?;

        // 2. 解析请求参数
        let query_req: DynamicDataQuery = from_value(data).map_err(|e| {
            error!("Unable to parse dynamic query data: {e}");
            (101, format!("Unable to parse dynamic query data: {e}"))
        })?;

        // 3. 开始构建查询
        let mut query = dynamic_monitoring::Entity::find();

        // 用于标记是否包含 "Last" 条件
        let mut is_last = false;

        // 4. 应用过滤条件
        for cond in query_req.condition {
            match cond {
                QueryCondition::Uuid(uuid) => {
                    query = query.filter(dynamic_monitoring::Column::Uuid.eq(uuid));
                }
                QueryCondition::TimestampFromTo(start, end) => {
                    query = query.filter(
                        dynamic_monitoring::Column::Timestamp
                            .gte(start)
                            .and(dynamic_monitoring::Column::Timestamp.lte(end)),
                    );
                }
                QueryCondition::TimestampFrom(start) => {
                    query = query.filter(dynamic_monitoring::Column::Timestamp.gte(start));
                }
                QueryCondition::TimestampTo(end) => {
                    query = query.filter(dynamic_monitoring::Column::Timestamp.lte(end));
                }
                QueryCondition::Last => {
                    is_last = true;
                }
            }
        }

        // 5. 处理排序和 Limit
        if is_last {
            // 取最新的一条
            query = query
                .order_by(dynamic_monitoring::Column::Timestamp, Order::Desc)
                .limit(1);
        } else {
            // 默认按时间正序
            query = query.order_by(dynamic_monitoring::Column::Timestamp, Order::Asc);
        }

        // 6. 执行查询
        let models = query.all(db).await.map_err(|e| {
            error!("Database query error: {e}");
            (103, format!("Database query error: {e}"))
        })?;

        // 7. 映射结果字段
        let result_list: Vec<Value> = models
            .into_iter()
            .map(|model| {
                let mut map = Map::new();

                map.insert("uuid".to_string(), Value::String(model.uuid));
                map.insert(
                    "timestamp".to_string(),
                    Value::Number(model.timestamp.into()),
                );

                // 根据请求的 fields 填充数据
                for field in &query_req.fields {
                    match field {
                        DynamicDataQueryField::Cpu => {
                            map.insert("cpu".to_string(), model.cpu_data.clone());
                        }
                        DynamicDataQueryField::Ram => {
                            map.insert("ram".to_string(), model.ram_data.clone());
                        }
                        DynamicDataQueryField::Load => {
                            map.insert("load".to_string(), model.load_data.clone());
                        }
                        DynamicDataQueryField::System => {
                            map.insert("system".to_string(), model.system_data.clone());
                        }
                        DynamicDataQueryField::Disk => {
                            map.insert("disk".to_string(), model.disk_data.clone());
                        }
                        DynamicDataQueryField::Network => {
                            map.insert("network".to_string(), model.network_data.clone());
                        }
                        DynamicDataQueryField::Gpu => {
                            map.insert("gpu".to_string(), model.gpu_data.clone());
                        }
                    }
                }

                Value::Object(map)
            })
            .collect();

        Ok(result_list)
    };

    match process_logic.await {
        Ok(results) => Value::Array(results),
        Err((code, msg)) => generate_error_message(code, &msg),
    }
}
