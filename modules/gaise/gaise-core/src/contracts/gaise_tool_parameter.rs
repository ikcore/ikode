use std::collections::BTreeMap;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub struct GaiseToolParameter {

    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type:Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description:Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    // BTreeMap (not HashMap) so the tool schema serialises with a deterministic,
    // sorted key order on every request. A HashMap reshuffles its iteration order per
    // instance (random hasher seed), and the tool list is rebuilt each turn — that
    // produces byte-different JSON every request and silently defeats provider prompt
    // caching (the cached prefix can't extend past the tools block).
    pub properties:Option<BTreeMap<String,GaiseToolParameter>>,

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
