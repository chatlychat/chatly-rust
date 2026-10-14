//! Customer-card endpoints — one signing secret per URL, with a two-phase
//! rotation.
//!
//! A sibling of [`crate::CustomerCards`], not a sub-resource of it: a card is
//! identified by its key, an endpoint by its URL, and several cards can point
//! at one URL. Give that URL an endpoint secret and every request to it is
//! signed with *that* secret instead of the leading card's — and stops
//! carrying `x-chatly-card-id`, which is a change at your verifier.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `customer_card_endpoints` namespace. Get one from
/// [`ChatlyClient::customer_card_endpoints`].
#[derive(Debug, Clone, Copy)]
pub struct CustomerCardEndpoints<'a> {
    client: &'a ChatlyClient,
}

impl<'a> CustomerCardEndpoints<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/customer-card-endpoints` — every endpoint secret, as a
    /// **bare JSON array**.
    ///
    /// Never the secret itself, nor a prefix or a hash of one. The field
    /// worth reading is `cardKeys`: the cards currently pointing at that URL.
    /// **An empty `cardKeys` means the secret signs nothing** — a leftover
    /// from a card that was renamed or removed, and the one state this list
    /// exists to make visible.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/customer-card-endpoints", query)
    }

    /// `POST /v1/customer-card-endpoints` — give one card URL its own
    /// signing secret, returned once.
    ///
    /// Body: `{ "url": "https://…" }`, `.strict()` — send `endpointUrl` and
    /// you get a 400 rather than a silently ignored key. The URL goes
    /// through the same SSRF guard as a card's.
    ///
    /// **From this call on, every request to that URL is signed with this
    /// secret and carries no `x-chatly-card-id`.** Deploy the secret before
    /// calling, not after: there is no grace period, and a verifier still
    /// keyed on the per-card secret rejects everything in the gap.
    ///
    /// Two refusals: a URL **no configured card uses** is a 400 (the secret
    /// would sign nothing), and a URL that **already has** an endpoint is a
    /// 409 (rotate it instead of creating a second).
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] and
    /// [`ChatlyError::Conflict`] as above.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            "/v1/customer-card-endpoints",
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/customer-card-endpoints/{id}/rotate-secret` — mint the
    /// **next** secret. It signs nothing yet.
    ///
    /// This is the half that makes rotation survivable, and the difference
    /// from [`crate::CustomerCards::rotate_secret`]: the new secret comes
    /// back with `active: false`, the current one keeps signing, and you
    /// promote the new one with [`CustomerCardEndpoints::activate_secret`]
    /// once your verifier accepts both. So the safe sequence is rotate →
    /// deploy → activate.
    ///
    /// Calling this again before activating **replaces** the pending secret;
    /// the one you were handed first is gone. It is therefore not idempotent
    /// in result — pin an `Idempotency-Key` if you may retry a timeout.
    ///
    /// Takes no body. Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such endpoint.
    pub fn rotate_secret(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/customer-card-endpoints/{}/rotate-secret",
                encode_path_segment(id)
            ),
            &Query::new(),
            None,
        )
    }

    /// `POST /v1/customer-card-endpoints/{id}/activate-secret` — promote the
    /// pending secret. It starts signing now and the previous one is gone.
    ///
    /// **A second call is a 409**, deliberately: "there is no pending
    /// secret" is a truthful answer, and a caller asking twice has lost track
    /// of a rotation, which is not a thing to paper over with a 200.
    ///
    /// Takes no body. Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Conflict`] if nothing is pending.
    pub fn activate_secret(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/customer-card-endpoints/{}/activate-secret",
                encode_path_segment(id)
            ),
            &Query::new(),
            None,
        )
    }

    /// `DELETE /v1/customer-card-endpoints/{id}` — answers `{ "ok": true }`
    /// with a 200.
    ///
    /// **This is a change at your verifier, not a cleanup.** Requests to that
    /// URL go back to being signed by the leading card of the group and to
    /// carrying `x-chatly-card-id`, so a verifier that only knows the
    /// endpoint secret starts rejecting everything.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such endpoint.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/customer-card-endpoints/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}
