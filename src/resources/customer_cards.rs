//! Customer cards — panels in the agent sidebar, rendered from *your*
//! backend.
//!
//! A card is a URL Chatly signs and POSTs to while an agent is looking at a
//! contact; you answer with blocks and they appear beside the conversation.
//! This namespace is the configuration side. The signing secret for a given
//! URL may instead live on an *endpoint* — see
//! [`crate::CustomerCardEndpoints`], which is a different resource with a
//! different lifecycle.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// How many cards one workspace may configure.
///
/// The cap the service enforces, and the reason every route here is
/// pagination-exempt.
pub const CUSTOMER_CARD_CAP: usize = 25;

/// The `customer_cards` namespace. Get one from
/// [`ChatlyClient::customer_cards`].
#[derive(Debug, Clone, Copy)]
pub struct CustomerCards<'a> {
    client: &'a ChatlyClient,
}

impl<'a> CustomerCards<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/customer-cards` — every configured card, as a **bare JSON
    /// array**.
    ///
    /// **Header NAMES only.** No header values, no secret, not a prefix or a
    /// hash of one. There is no route anywhere that reads a card secret
    /// back; if you lost it, rotate.
    ///
    /// Pagination-exempt — see [`CUSTOMER_CARD_CAP`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/customer-cards", query)
    }

    /// `POST /v1/customer-cards` — configure a card. **Returns the signing
    /// secret, once.**
    ///
    /// The body is the shared card config: a `key`, a `title`, a `url`, a
    /// `headers` bag and a `position`. The `url` goes through an SSRF guard —
    /// https only, no embedded credentials, no private or link-local
    /// address — and that refusal is a 400 rather than a later delivery
    /// failure.
    ///
    /// The response is the only one that carries `secret`. It verifies the
    /// `x-chatly-signature` on what we send you; store it before you
    /// acknowledge this call.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a URL the SSRF
    /// guard refuses or a duplicate `key`.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/customer-cards", &Query::new(), Some(body))
    }

    /// `PATCH /v1/customer-cards/{id}` — change a card.
    ///
    /// Every field optional, and the schema is **`.strict()`**: a misspelled
    /// key is a 400 rather than a silent no-op, which is the failure mode
    /// this route is shaped to avoid.
    ///
    /// **An absent field is unchanged; `headers: {}` clears them.** Those are
    /// different requests, so a client that serialises its whole model on
    /// every save will wipe the header bag.
    ///
    /// Answers the card as stored — **no secret**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such card.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/customer-cards/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/customer-cards/{id}/rotate-secret` — mint a new signing
    /// secret, shown once.
    ///
    /// **No overlap window on this resource**: the previous secret stops
    /// verifying the moment this returns, so update your endpoint first.
    /// (The *endpoint* resource does have a two-phase rotation — see
    /// [`crate::CustomerCardEndpoints::rotate_secret`] — and that is the
    /// difference between the two.)
    ///
    /// Takes no body. Answers **201**. Not idempotent in result: each call
    /// mints another secret, so pin an `Idempotency-Key` with
    /// [`ChatlyClient::with_idempotency_key`] if you may retry a timeout.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such card.
    pub fn rotate_secret(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/customer-cards/{}/rotate-secret",
                encode_path_segment(id)
            ),
            &Query::new(),
            None,
        )
    }

    /// `DELETE /v1/customer-cards/{id}` — answers `{ "ok": true }` with a
    /// 200, not a 204.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such card.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/customer-cards/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}
