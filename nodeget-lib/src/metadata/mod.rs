use uuid::Uuid;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Metadata {
    pub agent_uuid: Uuid,
    pub agent_name: String,
    pub agent_tags: Vec<String>,
}
