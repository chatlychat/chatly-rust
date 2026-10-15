//! Bots — Chatly-run AI personas, and customer-run external agents.
//!
//! One resource with two kinds of row. A `chatly` bot is a prompt, a model
//! and a tool list that our autonomous runner executes; an `external` bot is
//! a webhook URL we POST signed deliveries to and wait for a reply from. They
//! share this namespace because they share the table, but almost nothing
//! else: the delivery routes below are meaningless for a Chatly persona, and
//! `enabledTools` / `kbScope` / `guardrails` are read only by the runner,
//! which never runs for an external agent.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `status` values a delivery row can have, for
/// [`Bots::deliveries_with_status`].
///
/// Four, and the enum is closed: a typo is a 400 that names the four, which
/// is why this is a `?status=` parameter rather than a `filter[]` term —
/// the generic filter parser would have handed an unknown literal to the
/// query and answered 200 with an empty page.
pub const DELIVERY_STATUSES: [&str; 4] = ["pending", "delivered", "failed", "dead"];

/// The `bots` namespace. Get one from [`ChatlyClient::bots`].
#[derive(Debug, Clone, Copy)]
pub struct Bots<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Bots<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/bots` — one page of configured bots.
    ///
    /// Rows are `botDtoSchema.partial()`: with `?fields=` the server really
    /// does omit the rest, so treat every field as optional rather than
    /// assuming the shape of a single-bot read.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/bots", query)
    }

    /// `GET /v1/bots` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/bots", query)
    }

    /// `GET /v1/bots/{id}` — one bot, whole.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such bot.
    pub fn get(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/bots/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `POST /v1/bots` — create a persona, or an external agent.
    ///
    /// **The body is a union of two disjoint shapes and neither half is
    /// strict**, which is the trap worth knowing: a field belonging to the
    /// other half is silently *dropped*, not refused. Send `systemPrompt`
    /// alongside `type: "external"` and you get a 201 and an agent with no
    /// prompt — and the published reference used to render only the external
    /// half, so an integrator copying the one visible example hit exactly
    /// that.
    ///
    /// - `type` omitted, or `"chatly"` — a persona: `name`, `systemPrompt`,
    ///   `model`, `enabledTools`, `kbScope`, `maxToolCalls`, `guardrails`,
    ///   `escalationRules`.
    /// - `type: "external"` — your own agent: `name`, `webhookUrl`,
    ///   `timeoutSeconds`, `fallbackMessage`, `canStartConversations`
    ///   (default **false**, so an agent you do not opt in cannot
    ///   cold-message anybody).
    ///
    /// The external arm answers the bot **plus two credentials shown exactly
    /// once** — `signingSecret`, which verifies the `x-chatly-signature` on
    /// what we send you, and a `cb_` `token`, which authenticates the replies
    /// you send back. Neither has a read path: the token is hashed and the
    /// secret has no route that returns it. Persist both from this response
    /// or rotate.
    ///
    /// Answers **201** on both arms.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a `webhookUrl`
    /// the SSRF check refuses.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/bots", &Query::new(), Some(body))
    }

    /// `PATCH /v1/bots/{id}` — update a persona.
    ///
    /// For an external agent's webhook, timeout or fallback copy use
    /// [`Bots::update_external`]; the two patch bodies are different schemas
    /// against the same row.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such bot.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/bots/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `PATCH /v1/bots/{id}/external` — change an external agent's
    /// `webhookUrl`, `timeoutSeconds`, `fallbackMessage` or
    /// `canStartConversations`.
    ///
    /// **An empty patch is a 400**, not a no-op. The server read-modify-writes
    /// the whole `external_config` object, which is deliberate: a partial
    /// jsonb merge that omitted `signingSecret` would delete it, and the
    /// first sign of that would be every delivery arriving with a signature
    /// the customer's secret cannot verify.
    ///
    /// `webhookUrl` is re-validated against the SSRF rules on every patch.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an empty patch
    /// or a refused URL.
    pub fn update_external(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/bots/{}/external", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/bots/{id}/external/rotate-secret` — issue a new signing
    /// secret, effective immediately, and return it once.
    ///
    /// **This form is the hard cut.** It sends no body, and the old secret
    /// stops signing the moment this returns, so deploy the new one to your
    /// verifier before calling this, not after — any delivery in flight
    /// during the gap fails your signature check and retries into the
    /// ladder. For an overlap window use [`Bots::rotate_external_secret_at`].
    ///
    /// Answers **201** with `{ "signingSecret": "…", "activatesAt": null }`.
    /// `activatesAt` is `null` when the new secret is already signing, which
    /// is always the case here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if the bot is not an
    /// external agent.
    pub fn rotate_external_secret(&self, id: &str) -> Result<Value, ChatlyError> {
        self.rotate(id, None)
    }

    /// `POST /v1/bots/{id}/external/rotate-secret` with
    /// `{ "activateAt": "…" }` — issue a new signing secret that starts
    /// signing at `activate_at`, and return it once.
    ///
    /// `activate_at` is an ISO 8601 instant with a `Z` or an offset
    /// (`2026-10-16T09:00:00.000Z`, `2026-10-16T11:00:00+02:00`), passed
    /// through verbatim — the server validates it. **Until that instant the
    /// OLD secret keeps signing every delivery**; from it, the new one does.
    /// There is one signature per delivery, never two, so your verifier
    /// should hold both secrets for the window — call
    /// [`crate::verify_webhook_signature`] with one and, if that fails, the
    /// other — and drop the old one once the switch has passed.
    ///
    /// - More than 168 hours ahead of the **server's** clock is a
    ///   [`ChatlyError::Validation`]. This SDK does not check the bound
    ///   itself, because your clock is not the one that counts.
    /// - An instant already in the past is a hard cut, not an error.
    /// - On an agent with no usable secret today it is also a hard cut.
    ///
    /// Answers **201** with `{ "signingSecret": "…", "activatesAt": … }`:
    /// a string is the instant the switch happens; `null` means the new
    /// secret is already signing (the two hard-cut cases above).
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a malformed or
    /// too-distant instant; [`ChatlyError::NotFound`] if the bot is not an
    /// external agent.
    pub fn rotate_external_secret_at(
        &self,
        id: &str,
        activate_at: &str,
    ) -> Result<Value, ChatlyError> {
        self.rotate(id, Some(activate_at))
    }

    /// The one request both rotate forms build. No `activate_at` means **no
    /// body at all** — not `{}` — which is exactly what the hard cut has
    /// always sent.
    fn rotate(&self, id: &str, activate_at: Option<&str>) -> Result<Value, ChatlyError> {
        let body = activate_at.map(|at| {
            let mut body = serde_json::Map::new();
            body.insert("activateAt".to_owned(), Value::String(at.to_owned()));
            Value::Object(body)
        });
        self.client.write(
            "POST",
            &format!(
                "/v1/bots/{}/external/rotate-secret",
                encode_path_segment(id)
            ),
            &Query::new(),
            body.as_ref(),
        )
    }

    /// `GET /v1/bots/{id}/analytics` — how this bot has been doing.
    ///
    /// An aggregate over the bot's runs and deliveries, not a list, so there
    /// is nothing to paginate.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn analytics(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/bots/{}/analytics", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/bots/{id}/deliveries` — one page of delivery attempts to an
    /// external agent, newest first.
    ///
    /// **The signed payload is not in a list row.** A page of fifty would be
    /// megabytes of transcript, so a row carries status, attempts, latency
    /// and your response code; read [`Bots::delivery`] for the body that was
    /// actually signed.
    ///
    /// Takes `cursor`, `limit`, `sort` and a `status` of
    /// [`DELIVERY_STATUSES`] — see [`Bots::deliveries_with_status`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn deliveries(&self, id: &str, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page(
            &format!("/v1/bots/{}/deliveries", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/bots/{id}/deliveries` — every page, as an iterator.
    pub fn iter_deliveries(&self, id: &str, query: &Query) -> PageIter<Value> {
        self.client.paginate(
            &format!("/v1/bots/{}/deliveries", encode_path_segment(id)),
            query,
        )
    }

    /// [`Bots::deliveries`] narrowed to one status.
    ///
    /// The parameter is a plain `status=…`, not `filter[]=status:eq:…`: it is
    /// a closed zod enum, so a misspelling is a 400 naming the four values
    /// rather than an empty page. [`DELIVERY_STATUSES`] has the spellings.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a status outside
    /// the enum.
    pub fn deliveries_with_status(
        &self,
        id: &str,
        status: &str,
        query: &Query,
    ) -> Result<Page<Value>, ChatlyError> {
        self.deliveries(id, &query.clone().set("status", status))
    }

    /// `GET /v1/bots/{id}/deliveries/{delivery_id}` — one delivery, with the
    /// exact bytes that were signed.
    ///
    /// This is the route to use when a signature verification fails on your
    /// side: it returns the payload as sent, so you can re-run your own HMAC
    /// over it. Verifying a *stored* delivery means the timestamp is long
    /// outside any freshness window — see
    /// [`crate::VerifyOptions::without_tolerance`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such delivery on
    /// that bot.
    pub fn delivery(&self, id: &str, delivery_id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!(
                "/v1/bots/{}/deliveries/{}",
                encode_path_segment(id),
                encode_path_segment(delivery_id)
            ),
            &Query::new(),
        )
    }

    /// `POST /v1/bots/{id}/deliveries/{delivery_id}/redeliver` — queue the
    /// recorded payload again, from attempt 0.
    ///
    /// **The same bytes, including the same `eventId`** — so a receiver that
    /// dedupes on `eventId`, which ours tells you to do, will discard it if
    /// it already processed that id. That is correct on your side: a replay
    /// is for a delivery you never successfully processed, not a way to make
    /// the agent run twice.
    ///
    /// The URL it goes to is the agent's **current** one, not the one the
    /// original attempt used. Takes no body; answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn redeliver(&self, id: &str, delivery_id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!(
                "/v1/bots/{}/deliveries/{}/redeliver",
                encode_path_segment(id),
                encode_path_segment(delivery_id)
            ),
            &Query::new(),
            None,
        )
    }

    /// `POST /v1/bots/{id}/test-delivery` — send a real, really-signed
    /// delivery to the agent's URL and report what came back.
    ///
    /// **Writes nothing**: no ledger row, no conversation, no retry ladder.
    /// The payload carries `"test": true` and all-zero UUIDs, so acknowledge
    /// it with a 2xx and **do not reply** — a reply would target a
    /// conversation that does not exist.
    ///
    /// The response carries your status code, up to 2,000 characters of your
    /// body, the signature header we sent and the exact payload we signed,
    /// which together are enough to debug a verifier without any logging on
    /// your side. Takes no body; answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]. Note that your endpoint answering 500 is **not**
    /// an error here: the call succeeds and reports that status.
    pub fn test_delivery(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!("/v1/bots/{}/test-delivery", encode_path_segment(id)),
            &Query::new(),
            None,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_delivery_statuses_are_the_closed_enum() {
        assert_eq!(DELIVERY_STATUSES.len(), 4);
        assert!(DELIVERY_STATUSES.contains(&"dead"));
        // `succeeded` is the plausible wrong guess; the API says `delivered`.
        assert!(!DELIVERY_STATUSES.contains(&"succeeded"));
    }
}
