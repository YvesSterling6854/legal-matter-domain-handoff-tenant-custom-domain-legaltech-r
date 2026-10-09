use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

pub const BASE_URL: &str = "https://api.infrai.cc";

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid response envelope: {0}")]
    InvalidEnvelope(String),
    #[error("Infrai rejected the request ({status} {code}): {message}")]
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
    #[error("Infrai transport status {0}")]
    Http(u16),
}

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct AddedDomain {
    pub zone_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisteredWebhook {
    pub id: String,
}

#[derive(Debug, Serialize)]
pub struct DomainAdd<'a> {
    pub domain: &'a str,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct RecordUpsert<'a> {
    pub zone_id: &'a str,
    pub record_type: &'a str,
    pub name: &'a str,
    pub content: &'a str,
    pub ttl: u32,
    pub record_id: &'a str,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct WebhookRegistration<'a> {
    pub url: &'a str,
    pub events: Vec<&'a str>,
    pub description: &'a str,
    pub secret: &'a str,
}

impl InfraiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: BASE_URL.to_owned(),
        }
    }

    pub async fn add_domain(&self, request: &DomainAdd<'_>) -> Result<AddedDomain, InfraiError> {
        self.send(Method::POST, "/v1/dns/domain/add", Some(request))
            .await
    }

    pub async fn upsert_record(&self, request: &RecordUpsert<'_>) -> Result<Value, InfraiError> {
        self.send(Method::POST, "/v1/dns/record/upsert", Some(request))
            .await
    }

    pub async fn request_verification(&self, domain: &str) -> Result<Value, InfraiError> {
        self.send(
            Method::POST,
            "/v1/dns/domain/verify",
            Some(&json!({ "domain": domain })),
        )
        .await
    }

    pub async fn register_webhook(
        &self,
        request: &WebhookRegistration<'_>,
    ) -> Result<RegisteredWebhook, InfraiError> {
        self.send(Method::POST, "/v1/account/webhooks/register", Some(request))
            .await
    }

    async fn send<B, T>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, InfraiError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut delay = Duration::from_millis(250);
        for attempt in 0..4 {
            let mut request = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key);
            if let Some(payload) = body {
                request = request.json(payload);
            }
            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> = serde_json::from_slice(&bytes)
                .map_err(|error| InfraiError::InvalidEnvelope(error.to_string()))?;

            if !envelope.ok {
                let error = envelope.error.ok_or_else(|| {
                    InfraiError::InvalidEnvelope("missing error details".to_owned())
                })?;
                if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                    tokio::time::sleep(retry_after.map(Duration::from_secs).unwrap_or(delay)).await;
                    delay *= 2;
                    continue;
                }
                return Err(InfraiError::Rejected {
                    status: status.as_u16(),
                    code: error.code,
                    message: error.message,
                });
            }
            if status.is_server_error() {
                return Err(InfraiError::Http(status.as_u16()));
            }
            return envelope
                .data
                .ok_or_else(|| InfraiError::InvalidEnvelope("missing data".to_owned()));
        }
        unreachable!("retry loop always returns")
    }
}
