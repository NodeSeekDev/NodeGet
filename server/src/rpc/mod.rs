use crate::DB;
use nodeget_lib::error::NodegetError;
use sea_orm::{ActiveValue, DatabaseConnection, Set};
use serde::Serialize;
use serde_json::{Value, to_value};

pub mod agent;
pub mod crontab;
pub mod metadata;
pub mod nodeget;
pub mod task;
pub mod token;

pub trait RpcHelper {
    fn try_set_json<T: Serialize>(val: T) -> anyhow::Result<ActiveValue<Value>> {
        to_value(val)
            .map(Set)
            .map_err(|e| NodegetError::SerializationError(format!("Serialization error: {e}")).into())
    }

    fn get_db() -> anyhow::Result<&'static DatabaseConnection> {
        DB.get()
            .ok_or_else(|| NodegetError::DatabaseError("DB not initialized".to_owned()).into())
    }
}
