use super::{GaiseUsage, GaiseMessage, OneOrMany};

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GaiseInstructResponse {

    pub output:OneOrMany<GaiseMessage>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id:Option<String>,    

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage:Option<GaiseUsage>
}