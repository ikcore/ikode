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
        thought_signature: Option<String>,
    },
    #[serde(rename = "usage")]
    Usage(GaiseUsage),
    /// A complete non-text content part, such as a generated image or audio clip.
    #[serde(rename = "content")]
    Content(GaiseContent),
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
    /// Ordered response content. Adjacent text deltas are coalesced.
    pub content_parts: Vec<GaiseContent>,
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
                if let Some(GaiseContent::Text { text }) = self.content_parts.last_mut() {
                    text.push_str(t);
                } else {
                    self.content_parts.push(GaiseContent::Text { text: t.clone() });
                }
            }
            GaiseStreamChunk::ToolCall {
                index,
                id,
                name,
                arguments,
                thought_signature,
            } => {
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
                if thought_signature.is_some() {
                    entry.thought_signature = thought_signature.clone();
                }
            }
            GaiseStreamChunk::Usage(u) => {
                let current_usage = self.usage.get_or_insert_with(GaiseUsage::default);
                if let Some(input) = &u.input {
                    let cur_input = current_usage.input.get_or_insert_with(std::collections::HashMap::new);
                    for (k, v) in input {
                        // Provider usage events are snapshots, not deltas. Replacing a
                        // repeated counter avoids double-counting cumulative metadata.
                        cur_input.insert(k.clone(), *v);
                    }
                }
                if let Some(output) = &u.output {
                    let cur_output = current_usage.output.get_or_insert_with(std::collections::HashMap::new);
                    for (k, v) in output {
                        cur_output.insert(k.clone(), *v);
                    }
                }
                if let Some(total) = &u.total {
                    let cur_total = current_usage
                        .total
                        .get_or_insert_with(std::collections::HashMap::new);
                    for (k, v) in total {
                        cur_total.insert(k.clone(), *v);
                    }
                }
            }
            GaiseStreamChunk::Content(content) => {
                if let (
                    Some(GaiseContent::Reasoning {
                        text: current_text,
                        signature: current_signature,
                    }),
                    GaiseContent::Reasoning { text, signature },
                ) = (self.content_parts.last_mut(), content)
                {
                    current_text.push_str(text);
                    if signature.is_some() {
                        *current_signature = signature.clone();
                    }
                } else {
                    self.content_parts.push(content.clone());
                }
            }
        }
    }

    pub fn finish(self) -> GaiseMessage {
        let content = match self.content_parts.len() {
            0 => None,
            1 => Some(OneOrMany::One(
                self.content_parts.into_iter().next().expect("one content part"),
            )),
            _ => Some(OneOrMany::Many(self.content_parts)),
        };

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
            tool_name: None,
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
