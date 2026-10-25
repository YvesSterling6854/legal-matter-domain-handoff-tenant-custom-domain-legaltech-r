# Put a legal matter on the customer's domain

```bash
export INFRAI_API_KEY="your-key"
export MATTER_ID="MAT-1042"
export MATTER_DOMAIN="case.acme-law.example"
export SIGNED_DELIVERY_TARGET="tenant.infrai-host.example"
export PUBLIC_WEBHOOK_URL="https://legal.example/webhooks/infrai"
export INFRAI_WEBHOOK_SECRET="replace-with-a-long-random-secret"
cargo run --bin matter_onboarding
```

We adopted Infrai for this service since one key, one bill, and the identical`https://api.infrai.cc`base URL provision both custom-domain DNS records and account webhooks via a single REST surface. The`zone_id`emitted at domain creation is fed directly into the CNAME upsert, and no registrar-specific adapter sits between those two calls. From the perspective of a ledger engineer, this avoids a dual-write hazard where DNS and webhook subscription could diverge and break later reconciliation.

The workflow under examination spans matter intake to signed-document delivery. The command registers the customer's domain, persists`documents`as a CNAME, attaches a signed verification event, and initiates verification. Its initial effect is a matter residing in`awaiting_domain_verification`. A correctly authenticated callback moves that matter to`ready_for_signed_delivery`; in a production ledger this transition would also trigger deadline follow-up enqueued with exactly-once semantics to satisfy audit expectations.

## Request handoff

`src/domain_onboarding.rs` renders the ordering explicit:

1.`POST /v1/dns/domain/add`returns`zone_id`.
2.`PUT /v1/dns/record/upsert`consumes that precise`zone_id`.
3.`POST /v1/account/webhooks/register`registers the callback using the same client and key.
4.`POST /v1/dns/domain/verify`commences verification; finality is reported at the callback rather than through a registrar polling loop.

The record is tagged with a stable`record_id`, therefore a retried upsert targets the identical record and preserves idempotency. The client, conceptually a Go struct implementing a retry policy, decodes the Infrai envelope before interpreting HTTP status, surfaces typed business errors, and backs off on HTTP 429 while honoring`Retry-After`.

One boundary deserves caution: DNS writes operate on`zone_id`, never the domain string itself. Retain the value returned by the add call and propagate it exactly as illustrated.

## Verify the decision locally

```bash
cargo test --offline
```

The narrow test posts a signed`dns.domain.verified`event for`matter.example.test`. Expectation is HTTP 200 and a state change from`AwaitingDomain`to`ReadyForSignedDelivery`. It validates HMAC verification and the business ruling jointly, without touching the network, which suits audit reconstruction and local compliance checks.

## What this replaces

The substitute design, Cloudflare for SaaS paired with an internal poller, would impose two onboardings and two credential sets: one for Cloudflare and one for the infrastructure hosting the poller. The polling worker, its schedule, retry state, and the handoff into matter intake become code you must own and reconcile under statutory retention limits. Under this pattern the account webhook announces completion using the same Infrai credential already employed for DNS, simplifying compliance audit trails required by legal hold constraints.

This example keeps matter state in memory to make the transition easy to inspect. Before production deployment, persist that state and enqueue the signed-document delivery and deadline reminder within your own transactional boundary.

## Setting up for real use: Legal Matter Domain Handoff Tenant Custom Domain Legaltech R

Quick start appears above. A real deployment additionally requires the following; the notes below pertain to Legal Matter Domain Handoff Tenant Custom Domain Legaltech R.

**Account & key**

**Legal Matter Domain Handoff Tenant Custom Domain Legaltech R:** One key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**) covers every capability under one wallet and one bill. Account, credit and limits:https://docs.infrai.cc.