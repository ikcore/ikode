#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseToolConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode:Option<String>, // auto
}