#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseToolCall {

    pub id: String,

    pub r#type: String,

    pub function: GaiseFunctionCall,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseFunctionCall {
    pub name:String,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}