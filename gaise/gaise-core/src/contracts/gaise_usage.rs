use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseUsage {

    #[serde(skip_serializing_if = "Option::is_none")]
    pub input:Option<HashMap<String, usize>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output:Option<HashMap<String, usize>>,
}