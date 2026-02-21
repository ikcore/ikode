use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use gaise_core::{
    GaiseClient,
    contracts::{
        GaiseEmbeddingsRequest,
        GaiseEmbeddingsResponse,
        //GaiseRepository,
        GaiseInstructRequest,
        GaiseInstructResponse
    }
};
use super::contracts::google_claims::GoogleClaims;
use crate::contracts::{GoogleAccessToken, GoogleChatCompletionResponse, GoogleInstructRequest};
use crate::contracts::models::{GoogleEmbeddingsResponse, GoogleEmbeddingsRequest};
use super::contracts::ServiceAccount;

// const SERVICE_PROVIDER_ID: &str = "google";


#[derive(Clone)]
pub struct GaiseClientVertexAI {
    pub account:ServiceAccount,
    pub token:Arc<Mutex<String>>,
    //pub repository:Option<Arc<dyn GaiseRepository>>,
    pub api_url:String,
    pub updater_thread: Option<Arc<tokio::task::JoinHandle<()>>>,
    pub updater_thread_should_stop: Arc<AtomicBool>,
}

impl Drop for GaiseClientVertexAI {
    fn drop(&mut self) {

        self.updater_thread_should_stop.store(true, Ordering::SeqCst);

        if let Some(_thread) = self.updater_thread.take() {
            
            //thread.join().expect("The updater thread has panicked");
        }
        println!("Google token updater thread stopped");
    }
}

impl GaiseClientVertexAI {
    pub async fn new(sa:&ServiceAccount, api_url:String
        //,repository:Option<Arc<dyn GaiseRepository>>
        ) -> GaiseClientVertexAI {

        let mut client = GaiseClientVertexAI {
            account: sa.clone(),
            token: Arc::new(Mutex::new("".to_owned())),
            //repository,
            updater_thread: None,
            api_url,
            updater_thread_should_stop: Arc::new(AtomicBool::new(false))
        };
        client.update_token().await;
        client.start_token_updater();
        client
    }

    pub async fn fetch_new_token(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {

        println!("Google: Creating new access token");

        // Define JWT claims
        let now = Utc::now();
        let claims = GoogleClaims {
            iss: self.account.client_email.to_string(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_owned(),
            aud: "https://oauth2.googleapis.com/token".to_owned(),
            iat: now.timestamp(),
            exp: (now + std::time::Duration::from_secs(60*60)).timestamp(),
        };
    
        // Encode the JWT
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &jsonwebtoken::EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())?)?;
    
        // Data for the POST request
        let params = [("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &token)];
    
        // Make the POST request
        //let client = reqwest::Client::new();
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;

        let res = client.post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await?;
    
        // Parse the response to get the access token
        let response_body = res.text().await?;
        let response:GoogleAccessToken = serde_json::from_str(&response_body)?;

        Ok(response.access_token) // Example token
    }

    pub async fn update_token(&self) {

        match self.fetch_new_token().await {
            Ok(new_token) => {
                let mut token = self.token.lock().unwrap();
                *token = new_token;
                println!("Google: New access token created");
            },
            Err(e) => {
                eprintln!("Google: Failed to update token: {:?}",e);
            }
        }
    }

    pub fn start_token_updater(&mut self) {
        let cloned_self = self.clone();
        let should_stop = self.updater_thread_should_stop.clone();
        
        self.updater_thread = Some(Arc::new(tokio::spawn(async move {
            let mut start = Instant::now();
            let time_window = Duration::from_secs(15  * 60 );        
            while !should_stop.load(Ordering::SeqCst) {
                if start.elapsed() >= time_window {
                    println!("Google: Updating account token");
                    cloned_self.update_token().await;
                    start = Instant::now();  
                } else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        })));
    }
}

#[async_trait]
impl GaiseClient for GaiseClientVertexAI {
    async fn instruct(&self, request:&GaiseInstructRequest) -> Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>> {

        let url = self.api_url.replace("{{MODEL}}", &request.model);
        let json = serde_json::to_string(&GoogleInstructRequest::from(request))?;     

        let token = self.token.clone().lock().unwrap().clone();    
        if token == "" {
            return Err("no google access token!".to_owned().into());
        }

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build().unwrap();
        let res = client.post(&url)
            .header("Authorization", "Bearer ".to_owned() + &token)
            .header("Content-type", "application/json")
            .body(json)
            .send()
            .await
            .expect("failed to get response");

        let res_json = res.text().await.expect("failed to get payload");
        let response:GoogleChatCompletionResponse = serde_json::from_str(&res_json)?;
        let response_view = response.to_view();

        /*
        if let Some(repository) = &self.repository {
            _ = repository.log_chat(SERVICE_PROVIDER_ID, request, &response_view);
        }
        */
        Ok(response_view)
    }

    async fn embeddings(&self, request:&GaiseEmbeddingsRequest) -> Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>> {
        
        let url = self.api_url.replace("{{MODEL}}", &request.model);
        let json = serde_json::to_string(&GoogleEmbeddingsRequest::from(request))?;

        let token = self.token.clone().lock().unwrap().clone();    
        if token == "" {
            return Err("no google access token!".to_owned().into());
        }
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build().unwrap();
        let res = client.post(&url)
            .header("Authorization", "Bearer ".to_owned() + &token)
            .header("Content-type", "application/json")
            .body(json)
            .send()
            .await
            .expect("failed to get response");

        let res_wrapper = res.text();
        let res_json = res_wrapper.await.expect("failed to get payload");
        let response:GoogleEmbeddingsResponse = serde_json::from_str(&res_json)?;
        let response_view = response.to_view();

        Ok(response_view)
    }
}

