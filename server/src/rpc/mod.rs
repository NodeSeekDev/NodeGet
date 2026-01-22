use crate::DB;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::{to_value, Value};

pub mod agent;
pub mod nodeget;
pub mod task;

pub trait RpcHelper {
    fn try_set_json<T: Serialize>(val: T) -> Result<ActiveValue<Value>, String> {
        to_value(val)
            .map(Set)
            .map_err(|e| format!("Serialization error: {e}"))
    }

    fn get_db() -> Result<&'static DatabaseConnection, (i64, String)> {
        DB.get()
            .ok_or_else(|| (102, "DB not initialized".to_string()))
    }
}
