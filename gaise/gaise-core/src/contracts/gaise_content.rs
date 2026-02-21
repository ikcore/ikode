use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")] 
pub enum GaiseContent {

     #[serde(rename = "text")]
    Text {
        text:String
    },
    #[serde(rename = "audio")]
    Audio {
        #[serde(with="serde_bytes")] data:Vec<u8>,
        format:Option<String>
    },
    #[serde(rename = "image")]
    Image {
        #[serde(with="serde_bytes")] data:Vec<u8>,
        format:Option<String>
    },
    #[serde(rename = "file")]
    File {
        #[serde(with="serde_bytes")] data:Vec<u8>,
        name:Option<String>
    },
    #[serde(rename = "parts")]
    Parts {
        parts: Vec<GaiseContent>
    }
}

impl Default for GaiseContent {
    fn default() -> Self {
        GaiseContent::Text { text: String::new() }
    }
}