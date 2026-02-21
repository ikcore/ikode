use std::collections::HashMap;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseToolParameter {

    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type:Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description:Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties:Option<HashMap<String,GaiseToolParameter>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<GaiseToolParameter>>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required:Option<Vec<String>>
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseTool {
    pub name: String,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<GaiseToolParameter>,
}