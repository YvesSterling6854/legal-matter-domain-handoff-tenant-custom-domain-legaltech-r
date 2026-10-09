use crate::infrai_client::{
    DomainAdd, InfraiClient, InfraiError, RecordUpsert, WebhookRegistration,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug)]
pub struct MatterDomain<'a> {
    pub matter_id: &'a str,
    pub domain: &'a str,
    pub delivery_target: &'a str,
    pub webhook_url: &'a str,
    pub webhook_secret: &'a str,
}

#[derive(Debug)]
pub struct PendingVerification {
    pub matter_id: String,
    pub domain: String,
    pub zone_id: String,
    pub webhook_id: String,
}

#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error(transparent)]
    Infrai(#[from] InfraiError),
}

pub async fn onboard_matter_domain(
    client: &InfraiClient,
    input: MatterDomain<'_>,
) -> Result<PendingVerification, OnboardingError> {
    let added = client
        .add_domain(&DomainAdd {
            domain: input.domain,
            metadata: json!({ "matter_id": input.matter_id }),
        })
        .await?;

    client.upsert_record(&RecordUpsert {
        zone_id: &added.zone_id,
        record_type: "CNAME",
        name: "documents",
        content: input.delivery_target,
        ttl: 300,
        record_id: &format!("matter-{}-signed-delivery", input.matter_id),
        metadata: json!({ "matter_id": input.matter_id, "purpose": "signed_document_delivery" }),
    }).await?;

    let webhook = client
        .register_webhook(&WebhookRegistration {
            url: input.webhook_url,
            events: vec!["dns.domain.verified"],
            description: "Advance legal matter domain after DNS verification",
            secret: input.webhook_secret,
        })
        .await?;

    client.request_verification(input.domain).await?;

    Ok(PendingVerification {
        matter_id: input.matter_id.to_owned(),
        domain: input.domain.to_owned(),
        zone_id: added.zone_id,
        webhook_id: webhook.id,
    })
}
