//! Webhook *deliveries* — the outbound side.
//!
//! This namespace inspects what Chatly sent you. Verifying the signature on
//! something it sent is [`crate::verify_webhook_signature`], which is a
//! free function and needs no client.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `webhooks` namespace. Get one from [`ChatlyClient::webhooks`].
///
/// Managing endpoints and their secrets is not key-accessible — only
/// inspecting and replaying deliveries is.
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
}
