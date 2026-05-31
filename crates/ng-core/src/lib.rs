pub mod error;
pub mod js_result;
pub mod monitoring;
pub mod permission;
pub mod self_update;
pub mod utils;

/// NameValidator trait — unified input validation
pub trait NameValidator: Sized {
    fn validate(name: &str) -> Result<Self, error::NodegetError>;
}
