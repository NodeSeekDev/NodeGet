use uuid::Uuid;

pub struct Metadata {
    agent_uuid: Uuid,
    agent_name: String,
    agent_tags: Vec<String>,
}