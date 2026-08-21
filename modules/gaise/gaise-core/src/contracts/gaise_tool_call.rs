#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseToolCall {

    pub id: String,

    pub r#type: String,

    pub function: GaiseFunctionCall,

    /// Opaque provider signature that must be echoed in some multi-turn tool
    /// conversations (notably Gemini 3 thought signatures).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseFunctionCall {
    pub name:String,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
