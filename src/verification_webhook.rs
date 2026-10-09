use axum::{
    body::Bytes, extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct WebhookState {
    pub secret: String,
    pub matters: Arc<RwLock<Vec<MatterStatus>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatterStatus {
    pub matter_id: String,
    pub domain: String,
    pub state: IntakeState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntakeState {
    AwaitingDomain,
    ReadyForSignedDelivery,
}

#[derive(Debug, Deserialize)]
struct VerificationEvent {
    event: String,
    data: VerificationData,
}

#[derive(Debug, Deserialize)]
struct VerificationData {
    domain: String,
}

#[derive(Debug, Error)]
enum WebhookError {
    #[error("signature rejected")]
    Signature,
    #[error("invalid event payload")]
    Payload,
    #[error("matter domain not found")]
    MatterNotFound,
}

pub async fn receive_verification(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    match process_event(&state, &headers, &body).await {
        Ok(status) => (StatusCode::OK, axum::Json(status)).into_response(),
        Err(WebhookError::Signature) => StatusCode::UNAUTHORIZED.into_response(),
        Err(WebhookError::Payload) => StatusCode::BAD_REQUEST.into_response(),
        Err(WebhookError::MatterNotFound) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn process_event(
    state: &WebhookState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<serde_json::Value, WebhookError> {
    let supplied = headers
        .get("x-infrai-signature")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| hex::decode(value).ok())
        .ok_or(WebhookError::Signature)?;
    let mut mac =
        HmacSha256::new_from_slice(state.secret.as_bytes()).map_err(|_| WebhookError::Signature)?;
    mac.update(body);
    mac.verify_slice(&supplied)
        .map_err(|_| WebhookError::Signature)?;

    let event: VerificationEvent =
        serde_json::from_slice(body).map_err(|_| WebhookError::Payload)?;
    if event.event != "dns.domain.verified" {
        return Err(WebhookError::Payload);
    }
    let mut matters = state.matters.write().await;
    let matter = matters
        .iter_mut()
        .find(|matter| matter.domain == event.data.domain)
        .ok_or(WebhookError::MatterNotFound)?;
    matter.state = IntakeState::ReadyForSignedDelivery;
    Ok(serde_json::json!({ "matter_id": matter.matter_id, "state": "ready_for_signed_delivery" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use axum::{body::Body, routing::post, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn verified_domain_releases_signed_delivery() {
        let secret = "local-test-secret";
        let body = br#"{"event":"dns.domain.verified","data":{"domain":"matter.example.test"}}"#;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = hex::encode(mac.finalize().into_bytes());
        let matters = Arc::new(RwLock::new(vec![MatterStatus {
            matter_id: "MAT-1042".to_owned(),
            domain: "matter.example.test".to_owned(),
            state: IntakeState::AwaitingDomain,
        }]));
        let app = Router::new()
            .route("/webhooks/infrai", post(receive_verification))
            .with_state(WebhookState {
                secret: secret.to_owned(),
                matters: matters.clone(),
            });
        let response = app
            .oneshot(
                Request::post("/webhooks/infrai")
                    .header("x-infrai-signature", signature)
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            matters.read().await[0].state,
            IntakeState::ReadyForSignedDelivery
        );
    }
}
