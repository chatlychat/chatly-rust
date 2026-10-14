//! Webhooks — the subscriptions, and the deliveries made against them.
//!
//! Two halves. The endpoint routes manage what Chatly delivers *to*; the
//! delivery routes inspect what it already sent. Verifying the signature on
//! one of those deliveries is [`crate::verify_webhook_signature`], a free
//! function that needs no client.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `webhooks` namespace. Get one from [`ChatlyClient::webhooks`].
#[derive(Debug, Clone, Copy)]
pub struct Webhooks<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Webhooks<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/webhooks/deliveries` — one page of delivery attempts.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn deliveries(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/webhooks/deliveries", query)
    }

    /// `GET /v1/webhooks/deliveries` — every page, as an iterator.
    pub fn iter_deliveries(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/webhooks/deliveries", query)
    }

    /// `GET /v1/webhooks/deliveries/{id}` — one delivery, including its
    /// body.
    ///
    /// Useful for re-verifying an archived delivery — which is the one case
    /// [`crate::VerifyOptions::without_tolerance`] is for, since a stored
    /// delivery's `t` is by definition outside any live freshness window.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such delivery.
    pub fn delivery(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/webhooks/deliveries/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `POST /v1/webhooks/deliveries/{id}/redeliver` — send it again.
    ///
    /// A write, so it carries an `Idempotency-Key`: an SDK-level retry of a
    /// redelivery is not a second redelivery.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn redeliver(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/webhooks/deliveries/{}/redeliver",
                encode_path_segment(id)
            ),
            &Query::new(),
            None,
        )
    }

    /// `GET /v1/webhooks` — the subscriptions, as a **bare JSON array**.
    ///
    /// Rows carry `id`, `url`, `description`, `eventTypes`, `enabled` and
    /// `schemaVersion` — **never the secret**. There is no reveal route and
    /// no rotation route, so losing a secret means deleting the endpoint and
    /// creating another.
    ///
    /// Pagination-exempt; typically under 50 rows.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/webhooks", query)
    }

    /// `GET /v1/webhooks/events` — the subscribable event types, as
    /// `{ "eventTypes": [ … ] }`.
    ///
    /// An object, not a bare array. Static and workspace-independent.
    ///
    /// **Build your picker from this, not from a list of your own.** The
    /// dashboard's picker and this controller's validation were separately
    /// hard-coded once, and the dashboard offered three events the platform
    /// does not emit — a subscription that validates and then never fires.
    /// This route is the same constant the create route validates against.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn events(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/webhooks/events", query)
    }

    /// `POST /v1/webhooks` — subscribe an endpoint. **Returns the secret,
    /// once.**
    ///
    /// `url` and `eventTypes` (at least one, each from
    /// [`Webhooks::events`]) are required; `description` (≤280) and
    /// `schemaVersion` are optional, the latter defaulting to the latest.
    ///
    /// **An unrecognised `schemaVersion` is refused here** even though the
    /// dispatch path resolves an unknown one to the latest — loud on the
    /// write, generous on the read. A typo costs you one 400 now instead of
    /// an integration pinned to nothing that goes quiet with no error
    /// anywhere.
    ///
    /// The `secret` in this response is emitted here and nowhere else. It is
    /// what [`crate::verify_webhook_signature`] takes; store it before you
    /// acknowledge the call.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an empty
    /// `eventTypes`, an event type outside the enum, or an unknown schema
    /// version.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/webhooks", &Query::new(), Some(body))
    }

    /// `GET /v1/webhooks/schema-versions` — the pinnable versions, oldest
    /// first.
    ///
    /// `{ latest, versions }`, where each version carries its own
    /// `eventTypes` — which is the point: a consumer choosing a pin needs to
    /// see what each one contains *before* writing the subscription.
    ///
    /// **`isDeprecated` is advisory.** Nothing refuses a deprecated pin
    /// today, so treat it as a warning and not an error; a client that
    /// errors on it refuses a version the server is still happy to deliver.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn schema_versions(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/webhooks/schema-versions", query)
    }

    /// `PATCH /v1/webhooks/{id}/schema-version` — move an endpoint to
    /// another version.
    ///
    /// Body: `{ "schemaVersion": "…" }` and nothing else — `.strict()`, so
    /// sending the rest of an endpoint here is a 400 rather than a partial
    /// update.
    ///
    /// **Read `missingEventTypes` in the response.** It names, by event type
    /// and not as a count, what this endpoint is subscribed to that the
    /// version you just pinned does not describe — so a successful 200 can
    /// still mean some of your events will not be delivered in the shape you
    /// expect. That field is the only warning you get.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown
    /// version, [`ChatlyError::NotFound`] if no such endpoint.
    pub fn set_schema_version(
        &self,
        id: &str,
        body: &impl Serialize,
    ) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/webhooks/{}/schema-version", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/webhooks/{id}` — remove a subscription. Answers
    /// `{ "ok": true }` with a 200, not a 204.
    ///
    /// This route was closed to API keys while `POST /v1/webhooks` and every
    /// GET here were open, which is a worse asymmetry than closing the whole
    /// surface would have been: a deploy pipeline that registers an endpoint
    /// per environment leaked one into the customer's workspace on every run
    /// with nothing able to clean up. It was found by someone trying to write
    /// a `chatly listen` command that registers a temporary endpoint and
    /// deregisters it on Ctrl-C.
    ///
    /// Safe by the create route's own argument: both need
    /// `integrations:write`, and a key that can add a delivery target can
    /// already do the thing with consequences.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such endpoint.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/webhooks/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}
