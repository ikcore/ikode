use async_trait::async_trait;
use futures_util::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

use gaise_core::{
    GaiseClient,
    contracts::{
        GaiseConnection, GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructRequest,
        GaiseInstructResponse, GaiseInstructStreamResponse, GaiseListModelsRequest,
        GaiseListModelsResponse, GaiseModel, GaiseProviderError, redact_secrets,
    },
    logging::IGaiseLogger,
    registry::ModelRegistry,
};
#[cfg(feature = "live")]
use gaise_core::{
    GaiseLiveClient,
    contracts::{GaiseLiveConfig, GaiseLiveSession},
};
#[cfg(feature = "elevenlabs")]
use gaise_core::{
    GaiseSpeechClient,
    contracts::{GaiseSpeechRequest, GaiseSpeechResponse, GaiseSpeechStreamResponse},
};
#[cfg(feature = "anthropic")]
use gaise_provider_anthropic::anthropic_client::GaiseClientAnthropic;
#[cfg(feature = "bedrock")]
use gaise_provider_bedrock::bedrock_client::GaiseClientBedrock;
#[cfg(feature = "elevenlabs")]
use gaise_provider_elevenlabs::elevenlabs_client::GaiseClientElevenLabs;
#[cfg(feature = "gemini")]
use gaise_provider_gemini::gemini_client::GaiseClientGemini;
#[cfg(all(feature = "gemini", feature = "live"))]
use gaise_provider_gemini::gemini_live_client::GaiseClientGeminiLive;
#[cfg(feature = "ollama")]
use gaise_provider_ollama::ollama_client::GaiseClientOllama;
#[cfg(feature = "openai")]
use gaise_provider_openai::openai_client::GaiseClientOpenAI;
#[cfg(all(feature = "openai", feature = "live"))]
use gaise_provider_openai::openai_live_client::GaiseClientOpenAILive;
#[cfg(feature = "vertexai")]
pub use gaise_provider_vertexai::contracts::ServiceAccount;
#[cfg(feature = "vertexai")]
use gaise_provider_vertexai::vertexai_client::GaiseClientVertexAI;

/// Configuration for the GAISe client service.
/// This struct holds the necessary URLs and credentials for different AI providers.
#[derive(Debug, Clone, Default)]
pub struct GaiseClientConfig {
    /// URL for the Ollama service (e.g., "http://localhost:11434").
    #[cfg(feature = "ollama")]
    pub ollama_url: Option<String>,
    /// API URL for VertexAI.
    #[cfg(feature = "vertexai")]
    pub vertexai_api_url: Option<String>,
    /// Service Account credentials for VertexAI.
    #[cfg(feature = "vertexai")]
    pub vertexai_sa: Option<ServiceAccount>,
    /// API URL for OpenAI (e.g., "https://api.openai.com/v1").
    #[cfg(feature = "openai")]
    pub openai_api_url: Option<String>,
    /// API key for OpenAI.
    #[cfg(feature = "openai")]
    pub openai_api_key: Option<String>,
    /// AWS Region for Bedrock.
    #[cfg(feature = "bedrock")]
    pub bedrock_region: Option<String>,
    /// API URL for Anthropic (e.g., "https://api.anthropic.com/v1").
    #[cfg(feature = "anthropic")]
    pub anthropic_api_url: Option<String>,
    /// API key for Anthropic.
    #[cfg(feature = "anthropic")]
    pub anthropic_api_key: Option<String>,
    /// API URL for Gemini (e.g., "https://generativelanguage.googleapis.com/v1beta").
    #[cfg(feature = "gemini")]
    pub gemini_api_url: Option<String>,
    /// API key for Gemini.
    #[cfg(feature = "gemini")]
    pub gemini_api_key: Option<String>,
    /// API URL for ElevenLabs (e.g., "https://api.elevenlabs.io"; regional hosts allowed).
    #[cfg(feature = "elevenlabs")]
    pub elevenlabs_api_url: Option<String>,
    /// API key for ElevenLabs.
    #[cfg(feature = "elevenlabs")]
    pub elevenlabs_api_key: Option<String>,
    /// Optional logger for requests and responses.
    pub logger: Option<Arc<dyn IGaiseLogger>>,
}

/// A service that manages and routes requests to multiple Generative AI providers.
///
/// `GaiseClientService` implements the `GaiseClient` trait and uses a provider-prefix
/// routing mechanism (e.g., "openai::gpt-5.6-terra") to delegate calls to the appropriate
/// provider implementation.
pub struct GaiseClientService {
    #[allow(dead_code)]
    config: GaiseClientConfig,
    clients: RwLock<HashMap<String, Arc<dyn GaiseClient>>>,
    #[cfg(feature = "live")]
    live_clients: RwLock<HashMap<String, Arc<dyn GaiseLiveClient>>>,
    #[cfg(feature = "elevenlabs")]
    speech_clients: RwLock<HashMap<String, Arc<dyn GaiseSpeechClient>>>,
    logger: Option<Arc<dyn IGaiseLogger>>,
}

impl GaiseClientService {
    /// Creates a new `GaiseClientService` with the given configuration.
    pub fn new(config: GaiseClientConfig) -> Self {
        let logger = config.logger.clone();
        Self {
            config,
            clients: RwLock::new(HashMap::new()),
            #[cfg(feature = "live")]
            live_clients: RwLock::new(HashMap::new()),
            #[cfg(feature = "elevenlabs")]
            speech_clients: RwLock::new(HashMap::new()),
            logger,
        }
    }

    /// Retrieves an existing client for the specified provider or initializes a new one.
    ///
    /// Supported providers: "ollama", "vertexai", "openai", "bedrock", "anthropic",
    /// "gemini", "elevenlabs".
    pub async fn get_client(
        &self,
        provider: &str,
    ) -> Result<Arc<dyn GaiseClient>, Box<dyn std::error::Error + Send + Sync>> {
        self.get_client_with(provider, None).await
    }

    /// Cache key for a provider plus an optional per-request connection.
    fn client_key(provider: &str, connection: Option<&GaiseConnection>) -> String {
        match connection.filter(|c| !c.is_empty()) {
            Some(c) => format!("{provider}#{}", c.cache_key()),
            None => provider.to_string(),
        }
    }

    /// URL/key resolution: the request's connection wins, then the service config.
    fn resolve_url<'a>(
        connection: Option<&'a GaiseConnection>,
        configured: Option<&'a str>,
        default: Option<&'a str>,
        what: &str,
    ) -> Result<&'a str, Box<dyn std::error::Error + Send + Sync>> {
        connection
            .and_then(|c| c.api_url.as_deref())
            .or(configured)
            .or(default)
            .ok_or_else(|| format!("{what} API URL not configured").into())
    }

    fn resolve_key<'a>(
        connection: Option<&'a GaiseConnection>,
        configured: Option<&'a str>,
        what: &str,
    ) -> Result<&'a str, Box<dyn std::error::Error + Send + Sync>> {
        connection
            .and_then(|c| c.api_key.as_deref())
            .or(configured)
            .ok_or_else(|| format!("{what} API Key not configured").into())
    }

    /// Like [`Self::get_client`], honouring a per-request [`GaiseConnection`]
    /// (endpoint, key, region, service account). Clients built from an
    /// override are cached per distinct connection.
    pub async fn get_client_with(
        &self,
        provider: &str,
        connection: Option<&GaiseConnection>,
    ) -> Result<Arc<dyn GaiseClient>, Box<dyn std::error::Error + Send + Sync>> {
        let key = Self::client_key(provider, connection);
        {
            let clients = self.clients.read().await;
            if let Some(client) = clients.get(&key) {
                return Ok(client.clone());
            }
        }
        #[allow(unused_variables)]
        let connection = connection.filter(|c| !c.is_empty());

        #[allow(unused_variables)]
        let client: Arc<dyn GaiseClient> = match provider {
            #[cfg(feature = "ollama")]
            "ollama" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.ollama_url.as_deref(),
                    Some("http://localhost:11434"),
                    "Ollama",
                )?;
                Arc::new(GaiseClientOllama::new(url.to_string()))
            }
            #[cfg(feature = "vertexai")]
            "vertexai" => Arc::new(self.build_vertexai(connection).await?),
            #[cfg(feature = "openai")]
            "openai" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.openai_api_url.as_deref(),
                    Some("https://api.openai.com/v1"),
                    "OpenAI",
                )?;
                let key =
                    Self::resolve_key(connection, self.config.openai_api_key.as_deref(), "OpenAI")?;
                Arc::new(GaiseClientOpenAI::new(url.to_string(), key.to_string()))
            }
            #[cfg(feature = "bedrock")]
            "bedrock" => {
                let region = connection
                    .and_then(|c| c.region.clone())
                    .or_else(|| self.config.bedrock_region.clone());
                Arc::new(GaiseClientBedrock::new_with_region(region).await)
            }
            #[cfg(feature = "anthropic")]
            "anthropic" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.anthropic_api_url.as_deref(),
                    Some("https://api.anthropic.com/v1"),
                    "Anthropic",
                )?;
                let key = Self::resolve_key(
                    connection,
                    self.config.anthropic_api_key.as_deref(),
                    "Anthropic",
                )?;
                Arc::new(GaiseClientAnthropic::new(url.to_string(), key.to_string()))
            }
            #[cfg(feature = "gemini")]
            "gemini" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.gemini_api_url.as_deref(),
                    Some("https://generativelanguage.googleapis.com/v1beta"),
                    "Gemini",
                )?;
                let key =
                    Self::resolve_key(connection, self.config.gemini_api_key.as_deref(), "Gemini")?;
                Arc::new(GaiseClientGemini::new(url.to_string(), key.to_string()))
            }
            #[cfg(feature = "elevenlabs")]
            "elevenlabs" => Arc::new(self.build_elevenlabs(connection)?),
            _ => return Err(format!("Unknown or disabled provider: {}", provider).into()),
        };

        #[allow(unreachable_code)]
        {
            let mut clients = self.clients.write().await;
            clients.insert(key, client.clone());
            Ok(client)
        }
    }

    #[cfg(feature = "vertexai")]
    async fn build_vertexai(
        &self,
        connection: Option<&GaiseConnection>,
    ) -> Result<GaiseClientVertexAI, Box<dyn std::error::Error + Send + Sync>> {
        let override_sa: Option<ServiceAccount> = connection
            .and_then(|c| c.service_account.clone())
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("invalid VertexAI service account override: {e}"))?;
        let sa = match (&override_sa, &self.config.vertexai_sa) {
            (Some(sa), _) => sa,
            (None, Some(sa)) => sa,
            (None, None) => return Err("VertexAI Service Account not configured".into()),
        };
        let url = Self::resolve_url(
            connection,
            self.config.vertexai_api_url.as_deref(),
            None,
            "VertexAI",
        )?;
        GaiseClientVertexAI::new(sa, url.to_string()).await
    }

    /// Adds a client for a specific provider.
    pub async fn add_client(&self, provider: &str, client: Arc<dyn GaiseClient>) {
        let mut clients = self.clients.write().await;
        clients.insert(provider.to_string(), client);
    }

    /// Provider keys this service can build a client for from its
    /// configuration, plus any clients registered with [`Self::add_client`].
    ///
    /// Ollama and Bedrock have usable defaults (localhost, the AWS provider
    /// chain) but are only listed when explicitly configured so that an
    /// aggregate listing does not wait on services that were never set up.
    pub async fn configured_providers(&self) -> Vec<String> {
        #[allow(unused_mut)]
        let mut providers: Vec<String> = Vec::new();
        #[cfg(feature = "openai")]
        if self.config.openai_api_key.is_some() {
            providers.push("openai".into());
        }
        #[cfg(feature = "anthropic")]
        if self.config.anthropic_api_key.is_some() {
            providers.push("anthropic".into());
        }
        #[cfg(feature = "gemini")]
        if self.config.gemini_api_key.is_some() {
            providers.push("gemini".into());
        }
        #[cfg(feature = "vertexai")]
        if self.config.vertexai_sa.is_some() && self.config.vertexai_api_url.is_some() {
            providers.push("vertexai".into());
        }
        #[cfg(feature = "bedrock")]
        if self.config.bedrock_region.is_some() {
            providers.push("bedrock".into());
        }
        #[cfg(feature = "ollama")]
        if self.config.ollama_url.is_some() {
            providers.push("ollama".into());
        }
        #[cfg(feature = "elevenlabs")]
        if self.config.elevenlabs_api_key.is_some() {
            providers.push("elevenlabs".into());
        }
        let clients = self.clients.read().await;
        for key in clients.keys() {
            if !providers.iter().any(|p| p == key) {
                providers.push(key.clone());
            }
        }
        providers.sort();
        providers
    }

    /// List one provider's models, rewritten to routable `provider::id`
    /// identifiers and enriched from the bundled registry.
    pub async fn list_provider_models(
        &self,
        provider: &str,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let client = self
            .get_client_with(provider, request.connection.as_ref())
            .await?;
        let mut req = request.clone();
        req.provider = Some(provider.to_string());
        req.connection = None;
        // Filter after enrichment: the registry may supply the operations of
        // models whose provider API reports none.
        req.operation = None;
        let mut response = client.list_models(&req).await?;
        let registry = ModelRegistry::bundled();
        for model in &mut response.models {
            finish_model(registry, provider, model);
        }
        response.retain_operation(request.operation);
        Ok(response)
    }

    #[cfg(feature = "elevenlabs")]
    fn build_elevenlabs(
        &self,
        connection: Option<&GaiseConnection>,
    ) -> Result<GaiseClientElevenLabs, Box<dyn std::error::Error + Send + Sync>> {
        let key = Self::resolve_key(
            connection,
            self.config.elevenlabs_api_key.as_deref(),
            "ElevenLabs",
        )?;
        let url = Self::resolve_url(
            connection,
            self.config.elevenlabs_api_url.as_deref(),
            Some(gaise_provider_elevenlabs::elevenlabs_client::DEFAULT_API_URL),
            "ElevenLabs",
        )?;
        Ok(GaiseClientElevenLabs::new(url.to_string(), key.to_string()))
    }

    /// Retrieves or initializes a speech (text-to-speech) client for the provider.
    #[cfg(feature = "elevenlabs")]
    pub async fn get_speech_client(
        &self,
        provider: &str,
    ) -> Result<Arc<dyn GaiseSpeechClient>, Box<dyn std::error::Error + Send + Sync>> {
        self.get_speech_client_with(provider, None).await
    }

    /// Speech client honouring a per-request [`GaiseConnection`].
    #[cfg(feature = "elevenlabs")]
    pub async fn get_speech_client_with(
        &self,
        provider: &str,
        connection: Option<&GaiseConnection>,
    ) -> Result<Arc<dyn GaiseSpeechClient>, Box<dyn std::error::Error + Send + Sync>> {
        let key = Self::client_key(provider, connection);
        {
            let clients = self.speech_clients.read().await;
            if let Some(client) = clients.get(&key) {
                return Ok(client.clone());
            }
        }
        let connection = connection.filter(|c| !c.is_empty());
        let client: Arc<dyn GaiseSpeechClient> = match provider {
            "elevenlabs" => Arc::new(self.build_elevenlabs(connection)?),
            _ => return Err(format!("No speech provider available for: {}", provider).into()),
        };
        let mut clients = self.speech_clients.write().await;
        clients.insert(key, client.clone());
        Ok(client)
    }

    /// Registers a speech client under a provider key.
    #[cfg(feature = "elevenlabs")]
    pub async fn add_speech_client(&self, provider: &str, client: Arc<dyn GaiseSpeechClient>) {
        let mut clients = self.speech_clients.write().await;
        clients.insert(provider.to_string(), client);
    }

    /// Helper to parse a model string into (provider, model_name).
    /// The expected format is "provider::model_name".
    fn parse_model(model: &str) -> Result<(&str, &str), Box<dyn std::error::Error + Send + Sync>> {
        let parts: Vec<&str> = model.splitn(2, "::").collect();
        if parts.len() < 2 {
            return Err("Model name must be in the format 'provider::model'".into());
        }
        Ok((parts[0], parts[1]))
    }
}

/// Serialize a request for logging with credentials masked.
fn log_value<T: serde::Serialize>(request: &T) -> serde_json::Value {
    let mut value = serde_json::to_value(request).unwrap_or(serde_json::Value::Null);
    redact_secrets(&mut value);
    value
}

/// Apply the registry overlay and the `provider::id` routing form.
fn finish_model(registry: &ModelRegistry, provider: &str, model: &mut GaiseModel) {
    if model.provider.is_empty() {
        model.provider = provider.to_string();
    }
    registry.enrich(model);
    if !model.id.contains("::") {
        model.id = format!("{}::{}", model.provider, model.id);
    }
}

#[async_trait]
impl GaiseClient for GaiseClientService {
    async fn list_models(
        &self,
        request: &GaiseListModelsRequest,
    ) -> Result<GaiseListModelsResponse, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "list_models",
                request.provider.as_deref().unwrap_or("*"),
                log_value(request),
            );
        }

        let response = match &request.provider {
            // One provider: its failure is the caller's failure.
            Some(provider) => self.list_provider_models(provider, request).await?,
            None => {
                let providers = self.configured_providers().await;
                let results =
                    futures_util::future::join_all(providers.iter().map(|p| async move {
                        (p.clone(), self.list_provider_models(p, request).await)
                    }))
                    .await;
                let mut aggregate = GaiseListModelsResponse::default();
                for (provider, result) in results {
                    match result {
                        Ok(mut part) => {
                            aggregate.models.append(&mut part.models);
                            aggregate.errors.append(&mut part.errors);
                        }
                        Err(e) => aggregate.errors.push(GaiseProviderError {
                            provider,
                            message: e.to_string(),
                        }),
                    }
                }
                aggregate
            }
        };

        if let Some(logger) = &self.logger {
            logger.log_response(
                request.correlation_id.as_deref(),
                "list_models",
                request.provider.as_deref().unwrap_or("*"),
                serde_json::json!({
                    "models": response.models.len(),
                    "errors": response.errors,
                }),
                None,
            );
        }
        Ok(response)
    }

    async fn instruct(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {
        let (provider, actual_model) = Self::parse_model(&request.model)?;
        let client = self
            .get_client_with(provider, request.connection.as_ref())
            .await?;

        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "instruct",
                &request.model,
                log_value(request),
            );
        }

        let mut req = request.clone();
        req.model = actual_model.to_string();
        req.connection = None;
        let response = client.instruct(&req).await?;

        if let Some(logger) = &self.logger {
            logger.log_response(
                request.correlation_id.as_deref(),
                "instruct",
                &request.model,
                serde_json::to_value(&response).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(&response.usage).ok(),
            );
        }

        Ok(response)
    }

    async fn instruct_stream(
        &self,
        request: &GaiseInstructRequest,
    ) -> Result<
        Pin<
            Box<
                dyn Stream<
                        Item = Result<
                            GaiseInstructStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let (provider, actual_model) = Self::parse_model(&request.model)?;
        let client = self
            .get_client_with(provider, request.connection.as_ref())
            .await?;

        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "instruct_stream",
                &request.model,
                log_value(request),
            );
        }

        let mut req = request.clone();
        req.model = actual_model.to_string();
        req.connection = None;

        let stream = client.instruct_stream(&req).await?;

        use futures_util::StreamExt;
        use gaise_core::contracts::GaiseStreamChunk;

        let logger = self.logger.clone();
        let correlation_id = request.correlation_id.clone();
        let model = request.model.clone();

        let filtered_stream = stream.filter_map(move |item| {
            let logger = logger.clone();
            let correlation_id = correlation_id.clone();
            let model = model.clone();

            async move {
                match item {
                    Ok(resp) => {
                        if let Some(logger) = logger {
                            logger.log_stream_chunk(
                                correlation_id.as_deref(),
                                "instruct_stream",
                                &model,
                                serde_json::to_value(&resp).unwrap_or(serde_json::Value::Null),
                            );
                        }

                        if let GaiseStreamChunk::Text(ref t) = resp.chunk
                            && t.is_empty()
                        {
                            return None;
                        }
                        Some(Ok(resp))
                    }
                    Err(e) => Some(Err(e)),
                }
            }
        });

        Ok(Box::pin(filtered_stream))
    }

    async fn embeddings(
        &self,
        request: &GaiseEmbeddingsRequest,
    ) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        let (provider, actual_model) = Self::parse_model(&request.model)?;
        let client = self
            .get_client_with(provider, request.connection.as_ref())
            .await?;

        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "embeddings",
                &request.model,
                log_value(request),
            );
        }

        let mut req = request.clone();
        req.model = actual_model.to_string();
        req.connection = None;
        let response = client.embeddings(&req).await?;

        if let Some(logger) = &self.logger {
            logger.log_response(
                request.correlation_id.as_deref(),
                "embeddings",
                &request.model,
                serde_json::to_value(&response).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(&response.usage).ok(),
            );
        }

        Ok(response)
    }
}

#[cfg(feature = "live")]
impl GaiseClientService {
    /// Retrieves or initializes a live client for the specified provider.
    pub async fn get_live_client(
        &self,
        provider: &str,
    ) -> Result<Arc<dyn GaiseLiveClient>, Box<dyn std::error::Error + Send + Sync>> {
        self.get_live_client_with(provider, None).await
    }

    /// Live client honouring a per-request [`GaiseConnection`].
    pub async fn get_live_client_with(
        &self,
        provider: &str,
        connection: Option<&GaiseConnection>,
    ) -> Result<Arc<dyn GaiseLiveClient>, Box<dyn std::error::Error + Send + Sync>> {
        let key = Self::client_key(provider, connection);
        {
            let clients = self.live_clients.read().await;
            if let Some(client) = clients.get(&key) {
                return Ok(client.clone());
            }
        }
        #[allow(unused_variables)]
        let connection = connection.filter(|c| !c.is_empty());

        #[allow(unused_variables)]
        let client: Arc<dyn GaiseLiveClient> = match provider {
            #[cfg(feature = "gemini")]
            "gemini" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.gemini_api_url.as_deref(),
                    Some("https://generativelanguage.googleapis.com/v1beta"),
                    "Gemini",
                )?;
                let key =
                    Self::resolve_key(connection, self.config.gemini_api_key.as_deref(), "Gemini")?;
                Arc::new(GaiseClientGeminiLive::new(url.to_string(), key.to_string()))
            }
            #[cfg(feature = "openai")]
            "openai" => {
                let url = Self::resolve_url(
                    connection,
                    self.config.openai_api_url.as_deref(),
                    Some("https://api.openai.com"),
                    "OpenAI",
                )?;
                let key =
                    Self::resolve_key(connection, self.config.openai_api_key.as_deref(), "OpenAI")?;
                Arc::new(GaiseClientOpenAILive::new(url.to_string(), key.to_string()))
            }
            #[cfg(feature = "elevenlabs")]
            "elevenlabs" => Arc::new(self.build_elevenlabs(connection)?),
            _ => return Err(format!("No live provider available for: {}", provider).into()),
        };

        #[allow(unreachable_code)]
        {
            let mut clients = self.live_clients.write().await;
            clients.insert(key, client.clone());
            Ok(client)
        }
    }
}

#[cfg(feature = "live")]
#[async_trait]
impl GaiseLiveClient for GaiseClientService {
    async fn live_connect(
        &self,
        config: &GaiseLiveConfig,
    ) -> Result<GaiseLiveSession, Box<dyn std::error::Error + Send + Sync>> {
        let (provider, actual_model) = Self::parse_model(&config.model)?;
        let client = self
            .get_live_client_with(provider, config.connection.as_ref())
            .await?;

        let mut cfg = config.clone();
        cfg.model = actual_model.to_string();
        cfg.connection = None;
        client.live_connect(&cfg).await
    }
}

#[cfg(feature = "elevenlabs")]
#[async_trait]
impl GaiseSpeechClient for GaiseClientService {
    async fn speech(
        &self,
        request: &GaiseSpeechRequest,
    ) -> Result<GaiseSpeechResponse, Box<dyn std::error::Error + Send + Sync>> {
        let (provider, actual_model) = Self::parse_model(&request.model)?;
        let client = self
            .get_speech_client_with(provider, request.connection.as_ref())
            .await?;
        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "speech",
                &request.model,
                log_value(request),
            );
        }
        let mut req = request.clone();
        req.model = actual_model.to_string();
        req.connection = None;
        let response = client.speech(&req).await?;
        if let Some(logger) = &self.logger {
            logger.log_response(
                request.correlation_id.as_deref(),
                "speech",
                &request.model,
                serde_json::json!({
                    "format": response.format,
                    "bytes": response.audio.len(),
                    "external_id": response.external_id,
                }),
                serde_json::to_value(&response.usage).ok(),
            );
        }
        Ok(response)
    }

    async fn speech_stream(
        &self,
        request: &GaiseSpeechRequest,
    ) -> Result<
        Pin<
            Box<
                dyn Stream<
                        Item = Result<
                            GaiseSpeechStreamResponse,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send,
            >,
        >,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let (provider, actual_model) = Self::parse_model(&request.model)?;
        let client = self
            .get_speech_client_with(provider, request.connection.as_ref())
            .await?;
        if let Some(logger) = &self.logger {
            logger.log_request(
                request.correlation_id.as_deref(),
                "speech_stream",
                &request.model,
                log_value(request),
            );
        }
        let mut req = request.clone();
        req.model = actual_model.to_string();
        req.connection = None;
        client.speech_stream(&req).await
    }
}
