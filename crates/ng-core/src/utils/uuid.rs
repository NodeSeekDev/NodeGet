use crate::error::Result;
use uuid::Uuid;

pub fn generate_random_uuid() -> Result<Uuid> {
    Ok(Uuid::new_v4())
}
