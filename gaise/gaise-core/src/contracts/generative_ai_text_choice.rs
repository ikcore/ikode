use super::GenerativeAITextMessage;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GenerativeAITextChoice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index:Option<usize>,
    pub message:GenerativeAITextMessage,
    pub finish_reason:Option<String>
}