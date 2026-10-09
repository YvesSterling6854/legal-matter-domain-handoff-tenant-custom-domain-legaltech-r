use axum::{routing::post, Router};
use legal_domain_handoff::domain_onboarding::{onboard_matter_domain, MatterDomain};
use legal_domain_handoff::infrai_client::InfraiClient;
use legal_domain_handoff::verification_webhook::{
    receive_verification, IntakeState, MatterStatus, WebhookState,
};
use std::{env, sync::Arc};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
enum ServiceError {
    #[error("missing environment variable {0}")]
    MissingEnv(&'static str),
    #[error(transparent)]
    Onboarding(#[from] legal_domain_handoff::domain_onboarding::OnboardingError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn required(name: &'static str) -> Result<String, ServiceError> {
    env::var(name).map_err(|_| ServiceError::MissingEnv(name))
}

#[tokio::main]
async fn main() -> Result<(), ServiceError> {
    let api_key = required("INFRAI_API_KEY")?;
    let matter_id = required("MATTER_ID")?;
    let domain = required("MATTER_DOMAIN")?;
    let delivery_target = required("SIGNED_DELIVERY_TARGET")?;
    let webhook_url = required("PUBLIC_WEBHOOK_URL")?;
    let webhook_secret = required("INFRAI_WEBHOOK_SECRET")?;

    let client = InfraiClient::new(api_key);
    let pending = onboard_matter_domain(
        &client,
        MatterDomain {
            matter_id: &matter_id,
            domain: &domain,
            delivery_target: &delivery_target,
            webhook_url: &webhook_url,
            webhook_secret: &webhook_secret,
        },
    )
    .await?;
    println!(
        "matter={} domain={} zone_id={} webhook_id={} state=awaiting_domain_verification",
        pending.matter_id, pending.domain, pending.zone_id, pending.webhook_id
    );

    let state = WebhookState {
        secret: webhook_secret,
        matters: Arc::new(RwLock::new(vec![MatterStatus {
            matter_id,
            domain,
            state: IntakeState::AwaitingDomain,
        }])),
    };
    let app = Router::new()
        .route("/webhooks/infrai", post(receive_verification))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("listening on http://0.0.0.0:3000/webhooks/infrai");
    axum::serve(listener, app).await?;
    Ok(())
}
