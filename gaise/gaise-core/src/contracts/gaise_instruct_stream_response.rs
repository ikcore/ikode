use super::{GaiseUsage, GaiseMessage, GaiseContent, OneOrMany, GaiseToolCall};
use futures_util::{Stream, StreamExt};

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub enum GaiseStreamChunk {
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "tool_call")]
    ToolCall {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage(GaiseUsage),
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct GaiseInstructStreamResponse {
    pub chunk: GaiseStreamChunk,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GaiseStreamAccumulator {
    pub role: String,
    pub text: String,
    pub tool_calls: std::collections::BTreeMap<usize, GaiseToolCall>,
    pub usage: Option<GaiseUsage>,
    pub external_id: Option<String>,
}

impl GaiseStreamAccumulator {
    pub fn new() -> Self {
        Self {
            role: "assistant".to_string(),
            ..Default::default()
        }
    }

    pub fn push(&mut self, response: &GaiseInstructStreamResponse) {
        if self.external_id.is_none() {
            self.external_id = response.external_id.clone();
        }

        match &response.chunk {
            GaiseStreamChunk::Text(t) => {
                self.text.push_str(t);
            }
            GaiseStreamChunk::ToolCall { index, id, name, arguments } => {
                let entry = self.tool_calls.entry(*index).or_insert_with(|| GaiseToolCall {
                    r#type: "function".to_string(),
                    ..Default::default()
                });

                if let Some(id) = id {
                    entry.id.push_str(id);
                }
                if let Some(name) = name {
                    entry.function.name.push_str(name);
                }
                if let Some(args) = arguments {
                    let current_args = entry.function.arguments.get_or_insert_with(String::new);
                    current_args.push_str(args);
                }
            }
            GaiseStreamChunk::Usage(u) => {
                let current_usage = self.usage.get_or_insert_with(GaiseUsage::default);
                if let Some(input) = &u.input {
                    let cur_input = current_usage.input.get_or_insert_with(std::collections::HashMap::new);
                    for (k, v) in input {
                        *cur_input.entry(k.clone()).or_insert(0) += v;
                    }
                }
                if let Some(output) = &u.output {
                    let cur_output = current_usage.output.get_or_insert_with(std::collections::HashMap::new);
                    for (k, v) in output {
                        *cur_output.entry(k.clone()).or_insert(0) += v;
                    }
                }
            }
        }
    }

    pub fn finish(self) -> GaiseMessage {
        let mut content = None;
        if !self.text.is_empty() {
            content = Some(OneOrMany::One(GaiseContent::Text { text: self.text }));
        }

        let tool_calls = if self.tool_calls.is_empty() {
            None
        } else {
            Some(self.tool_calls.into_values().collect())
        };

        GaiseMessage {
            role: self.role,
            content,
            tool_calls,
            tool_call_id: None,
        }
    }

    pub async fn collect<S, E>(mut stream: S) -> Result<GaiseMessage, E>
    where
        S: Stream<Item = Result<GaiseInstructStreamResponse, E>> + Unpin,
    {
        let mut accumulator = Self::new();
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            accumulator.push(&chunk);
        }
        Ok(accumulator.finish())
    }
}