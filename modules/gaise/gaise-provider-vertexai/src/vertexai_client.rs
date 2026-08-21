use std::sync::Arc;
use std::time::Duration;

use super::contracts::ServiceAccount;
use super::contracts::google_claims::GoogleClaims;
use crate::contracts::catalog::{
    VertexCatalogEndpoint, VertexPublisherModelList, map_vertex_model,
};
use crate::contracts::models::{
    GoogleEmbeddingsRequest, GoogleEmbeddingsResponse, vertex_embed_request,
};

/// Sums provider-named usage counters across the per-text calls that
/// `gemini-embedding-001` requires.
#[derive(Default)]
struct GaiseUsageAccumulator {
    input: std::collections::HashMap<String, usize>,
    output: std::collections::HashMap<String, usize>,
    total: std::collections::HashMap<String, usize>,
}

impl GaiseUsageAccumulator {
    fn add(&mut self, usage: &gaise_core::contracts::GaiseUsage) {
        for (target, source) in [
            (&mut self.input, &usage.input),
            (&mut self.output, &usage.output),
            (&mut self.total, &usage.total),
        ] {
            if let Some(map) = source {
                for (k, v) in map {
                    *target.entry(k.clone()).or_insert(0) += v;
                }
            }
        }
    }

    fn finish(self) -> gaise_core::contracts::GaiseUsage {
        gaise_core::contracts::GaiseUsage {
            input: (!self.input.is_empty()).then_some(self.input),
            output: (!self.output.is_empty()).then_some(self.output),
            total: (!self.total.is_empty()).then_some(self.total),
        }
    }
}
use crate::contracts::{GoogleAccessToken, GoogleChatCompletionResponse, GoogleInstructRequest};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use gaise_core::{
    GaiseClient,
    contracts::{
        GaiseEmbeddingsRequest,
        GaiseEmbeddingsResponse,
        //GaiseRepository,
        GaiseInstructRequest,
        GaiseInstructResponse,
        GaiseInstructStreamResponse,
        GaiseListModelsRequest,
        GaiseListModelsResponse,
        normalize_l2,
    },
};

// const SERVICE_PROVIDER_ID: &str = "google";

#[derive(Clone)]
pub struct GaiseClientVertexAI {
    pub account: ServiceAccount,
    pub api_url: String,
    /// Optional processing tier ("flex", …) read once from the `VERTEXAI_API_TIER`
    /// env var at construction. Unlike OpenAI (a `service_tier` body field), Vertex AI
    /// selects the tier via HTTP headers, so this is applied as request headers on every
    /// generative call when set; left off entirely when unset so Vertex applies its own
    /// default. `flex` trades latency for lower cost.
    service_tier: Option<String>,
    token_state: Arc<tokio::sync::Mutex<TokenState>>,
    http: reqwest::Client,
}

struct TokenState {
    access_token: String,
    token_type: String,
    expires_at: Option<DateTime<Utc>>,
}

impl GaiseClientVertexAI {
    pub async fn new(
        sa: &ServiceAccount,
        api_url: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            account: sa.clone(),
            api_url,
            service_tier: resolve_tier(std::env::var("VERTEXAI_API_TIER").ok()),
            token_state: Arc::new(tokio::sync::Mutex::new(TokenState {
                access_token: String::new(),
                token_type: "Bearer".to_string(),
                expires_at: None,
            })),
            http,
        })
    }

    /// Apply the Vertex AI processing-tier headers when a tier is configured. Vertex
    /// selects the shared/flex tier via HTTP headers (not the request body):
    /// `X-Vertex-AI-LLM-Request-Type: shared` plus
    /// `X-Vertex-AI-LLM-Shared-Request-Type: <tier>` (e.g. `flex`). When no tier is set
    /// the headers are omitted entirely so Vertex applies its own default (dedicated).
    fn apply_tier_headers(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.service_tier {
            Some(tier) => rb
                .header("X-Vertex-AI-LLM-Request-Type", "shared")
                .header("X-Vertex-AI-LLM-Shared-Request-Type", tier.as_str()),
            None => rb,
        }
    }

    pub async fn get_token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let refresh_skew = Duration::from_secs(5 * 60);
        let now = Utc::now();

        let mut state = self.token_state.lock().await;
        let needs_refresh = match state.expires_at {
            None => state.access_token.is_empty(),
            Some(exp) => state.access_token.is_empty() || (now + refresh_skew) >= exp,
        };
        if needs_refresh {
            println!("Google: Refreshing access token (lazy)");

            let response = self.fetch_new_token().await?;
            let expires_at = now + Duration::from_secs(response.expires_in as u64);

            state.access_token = response.access_token;
            state.token_type = response.token_type;
            state.expires_at = Some(expires_at);

            println!(
                "Google: New access token created; expires at {}",
                expires_at
            );
        }
        Ok(state.access_token.clone())
    }

    pub async fn fetch_new_token(
        &self,
    ) -> Result<GoogleAccessToken, Box<dyn std::error::Error + Send + Sync>> {
        println!("Google: Creating new access token");

        let now = Utc::now();
        let claims = GoogleClaims {
            iss: self.account.client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_owned(),
            aud: "https://oauth2.googleapis.com/token".to_owned(),
            iat: now.timestamp(),
            exp: (now + std::time::Duration::from_secs(60 * 60)).timestamp(),
        };
        let jwt = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())?,
        )?;
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ];
        let res = self
            .http
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await?;

        let body = checked_text(res, "Vertex AI token request").await?;
        let response: GoogleAccessToken = serde_json::from_str(&body)?;
        Ok(response)
    }

    pub async fn get_auth_header_value(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let token = self.get_token().await?;
        Ok(format!("Bearer {}", token))
    }
}

#[async_trait]
impl GaiseClient for GaiseClientVertexAI {
    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        std::pin::Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<
                            GaiseInstructStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url =
            self.api_url.replace("{{MODEL}}", &request.model) + ":streamGenerateContent?alt=sse";
        let json = serde_json::to_string(&GoogleInstructRequest::from(request))?;

        let token = self
            .get_token()
            .await
            .map_err(|e| format!("no google access token: {e}"))?;

        let req = self
            .http
            .post(&url)
            .header("Authorization", "Bearer ".to_owned() + &token)
            .header("Content-type", "application/json")
            .body(json);
        let res = self.apply_tier_headers(req).send().await?;

        if !res.status().is_success() {
            return Err(response_error(res, "Vertex AI streaming request").await);
        }

        let stream = res.bytes_stream();

        // Buffer SSE data across TCP chunks — a single `data: {...}` line
        // can be split across multiple chunks.
        let buffered_stream = futures_util::stream::unfold(
            (stream, Vec::<u8>::new()),
            |(mut stream, mut buf)| async move {
                use futures_util::StreamExt as _;
                loop {
                    // Try to extract complete lines from the buffer
                    let mut results = Vec::new();
                    while let Some(newline_pos) = buf.iter().position(|byte| *byte == b'\n') {
                        let line_bytes: Vec<u8> = buf.drain(..=newline_pos).collect();
                        let line = String::from_utf8_lossy(&line_bytes);
                        let line = line.trim();

                        if let Some(json_str) = line.strip_prefix("data:").map(str::trim_start) {
                            match serde_json::from_str::<GoogleChatCompletionResponse>(json_str) {
                                Ok(response) => {
                                    for r in response.to_stream_view() {
                                        results.push(Ok(r));
                                    }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[vertexai-stream] parse failed: {e}; json preview: {}...",
                                        &json_str[..json_str.len().min(200)]
                                    );
                                    results.push(Err(
                                        Box::new(e) as Box<dyn std::error::Error + Send + Sync>
                                    ));
                                }
                            }
                        }
                    }

                    if !results.is_empty() {
                        return Some((futures_util::stream::iter(results), (stream, buf)));
                    }

                    // Need more data
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            buf.extend_from_slice(&chunk);
                        }
                        Some(Err(e)) => {
                            let err: Box<dyn std::error::Error + Send + Sync> = Box::new(e);
                            return Some((
                                futures_util::stream::iter(vec![Err(err)]),
                                (stream, buf),
                            ));
                        }
                        None => {
                            // Stream ended — flush any remaining buffered data
                            let remaining = String::from_utf8_lossy(&buf);
                            let trimmed = remaining.trim();
                            if let Some(json_str) =
                                trimmed.strip_prefix("data:").map(str::trim_start)
                                && let Ok(response) =
                                    serde_json::from_str::<GoogleChatCompletionResponse>(json_str)
                            {
                                let results: Vec<
                                    Result<
                                        GaiseInstructStreamResponse,
                                        Box<dyn std::error::Error + Send + Sync>,
                                    >,
                                > = response.to_stream_view().into_iter().map(Ok).collect();
                                if !results.is_empty() {
                                    return Some((
                                        futures_util::stream::iter(results),
                                        (stream, Vec::new()),
                                    ));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        )
        .flatten();

        Ok(Box::pin(buffered_stream))
    }

    async fn instruct(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url.replace("{{MODEL}}", &request.model) + ":generateContent";
        let json = serde_json::to_string(&GoogleInstructRequest::from(request))?;

        let token = self
            .get_token()
            .await
            .map_err(|e| format!("no google access token: {e}"))?;

        let req = self
            .http
            .post(&url)
            .header("Authorization", "Bearer ".to_owned() + &token)
            .header("Content-type", "application/json")
            .body(json);
        let res = self
            .apply_tier_headers(req)
            .send()
            .await
            .map_err(|e| format!("Vertex AI request failed: {e}"))?;

        let res_json = checked_text(res, "Vertex AI request").await?;

        let response: GoogleChatCompletionResponse = serde_json::from_str(&res_json)?;
        let response_view = response.to_view();

        Ok(response_view)
    }

    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let endpoint = VertexCatalogEndpoint::from_template(&self.api_url)?;
        let mut models = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let page = self
                .list_publisher_models_page(&endpoint, token.as_deref())
                .await?;
            models.extend(
                page.publisher_models
                    .iter()
                    .map(|m| map_vertex_model(m, request.include_raw)),
            );
            match page.next_page_token {
                Some(next) if !next.is_empty() && token.as_deref() != Some(next.as_str()) => {
                    token = Some(next)
                }
                _ => break,
            }
        }
        let mut response = GaiseListModelsResponse::from_models(models);
        response.retain_operation(request.operation);
        Ok(response)
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        if request
            .model
            .to_ascii_lowercase()
            .starts_with("gemini-embedding-2")
        {
            return Err("Vertex AI serves gemini-embedding-2 through the `:embedContent` method on the `aiplatform.{location}.rep.googleapis.com` host, which the GAISe Vertex adapter does not call yet; use `gemini::gemini-embedding-2` or a Vertex text-embedding model".into());
        }
        let url = self.api_url.replace("{{MODEL}}", &request.model) + ":predict";

        // gemini-embedding-001 takes one text per call; the others batch up to 250.
        let (wire, resolved) = vertex_embed_request(request);
        let batches: Vec<GoogleEmbeddingsRequest> = if resolved.single_input {
            wire.instances
                .into_iter()
                .map(|instance| GoogleEmbeddingsRequest {
                    instances: vec![instance],
                    parameters: wire.parameters.clone(),
                })
                .collect()
        } else {
            vec![wire]
        };

        let token = self
            .get_token()
            .await
            .map_err(|e| format!("no google access token: {e}"))?;

        let mut response_view = GaiseEmbeddingsResponse::default();
        let mut usage_total: Option<GaiseUsageAccumulator> = None;
        for batch in &batches {
            let json = serde_json::to_string(batch)?;
            let res = self
                .http
                .post(&url)
                .header("Authorization", "Bearer ".to_owned() + &token)
                .header("Content-type", "application/json")
                .body(json)
                .send()
                .await
                .map_err(|e| format!("embeddings request failed: {e}"))?;

            let res_json = checked_text(res, "Vertex AI embeddings request").await?;
            let response: GoogleEmbeddingsResponse =
                serde_json::from_str(&res_json).map_err(|e| {
                    format!(
                        "embeddings response parse failed: {e} — body: {}",
                        &res_json[..res_json.len().min(500)]
                    )
                })?;
            let part = response.to_view();
            response_view.output.extend(part.output);
            response_view.external_id = response_view.external_id.or(part.external_id);
            if let Some(usage) = part.usage {
                usage_total.get_or_insert_with(Default::default).add(&usage);
            }
        }
        response_view.usage = usage_total.map(|u| u.finish());
        if resolved.normalize_locally {
            response_view
                .output
                .iter_mut()
                .for_each(|v| normalize_l2(v));
        }

        Ok(response_view)
    }
}

impl GaiseClientVertexAI {
    /// One page of Model Garden `publishers.models.list` (v1beta1) for the
    /// publisher named in the configured URL template.
    pub async fn list_publisher_models_page(
        &self,
        endpoint: &VertexCatalogEndpoint,
        page_token: Option<&str>,
    ) -> Result<VertexPublisherModelList, Box<dyn std::error::Error + Send + Sync>> {
        let token = self
            .get_token()
            .await
            .map_err(|e| format!("no google access token: {e}"))?;
        let res = self
            .http
            .get(endpoint.list_url(page_token))
            .header("Authorization", "Bearer ".to_owned() + &token)
            .send()
            .await
            .map_err(|e| format!("model list request failed: {e}"))?;
        let body = checked_text(res, "Vertex AI model list request").await?;
        serde_json::from_str(&body).map_err(|e| {
            format!(
                "model list response parse failed: {e} — body: {}",
                &body[..body.len().min(500)]
            )
            .into()
        })
    }
}

async fn checked_text(
    response: reqwest::Response,
    operation: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if !response.status().is_success() {
        return Err(response_error(response, operation).await);
    }
    response
        .text()
        .await
        .map_err(|e| format!("{operation} response body read failed: {e}").into())
}

async fn response_error(
    response: reqwest::Response,
    operation: &str,
) -> Box<dyn std::error::Error + Send + Sync> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let preview: String = body.chars().take(1_000).collect();
    format!("{operation} returned HTTP {status}: {preview}").into()
}

/// Normalise a raw `VERTEXAI_API_TIER` value: trim surrounding whitespace and treat an
/// empty value as unset, so a blank `VERTEXAI_API_TIER=` never produces empty tier
/// headers. Mirrors the OpenAI provider's tier resolution.
fn resolve_tier(raw: Option<String>) -> Option<String> {
    raw.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::resolve_tier;

    #[test]
    fn tier_resolution_trims_and_treats_blank_as_unset() {
        assert_eq!(
            resolve_tier(Some("flex".to_string())).as_deref(),
            Some("flex")
        );
        // Surrounding whitespace is trimmed.
        assert_eq!(
            resolve_tier(Some("  flex  ".to_string())).as_deref(),
            Some("flex")
        );
        // A blank / whitespace-only value is unset (so no empty headers are sent).
        assert_eq!(resolve_tier(Some("".to_string())), None);
        assert_eq!(resolve_tier(Some("   ".to_string())), None);
        // An absent env var is unset.
        assert_eq!(resolve_tier(None), None);
    }
}
