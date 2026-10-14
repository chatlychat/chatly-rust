//! AI configuration: BYOK provider keys, the spend meter, tone of voice and
//! custom tools.
//!
//! Four unrelated API surfaces under one namespace, because that is how
//! `packages/sdk-spec/surface.json` groups them and because "how is this
//! workspace's AI configured" is one question. The paths are
//! `/v1/ai-providers` (no slash — a sibling of `/v1/ai`, not a child of it),
//! `/v1/ai/quota`, `/v1/ai/tone` and `/v1/ai/tools`.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `provider` values the BYOK routes accept.
///
/// Two, and they are the only two: Chatly's provider abstraction has an
/// OpenAI and an Anthropic implementation. Offer these in a picker; do not
/// validate against them, because the server's list is the authority and may
/// grow before this constant does.
pub const AI_PROVIDERS: [&str; 2] = ["openai", "anthropic"];

/// The `ai` namespace. Get one from [`ChatlyClient::ai`].
#[derive(Debug, Clone, Copy)]
pub struct Ai<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Ai<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/ai-providers` — which providers this workspace has a key
    /// for, as `{ "providers": [ … ] }`.
    ///
    /// An object with one key, not a bare array — the one shape surprise in
    /// this namespace, and the opposite of [`Ai::tools`] next door.
    ///
    /// **No response on any of these routes carries a key.** The stored
    /// secret is write-only across this boundary; each row shows a
    /// four-character hint and a validation status instead, and there is no
    /// route anywhere that reads one back. A `status` of `unverified` means
    /// nobody has proven the key yet, which is not the same as a failure.
    ///
    /// Needs `ai:read`, which is a separate grant from `integrations:read`:
    /// an LLM key spends the customer's money at the vendor.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn providers(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/ai-providers", query)
    }

    /// `POST /v1/ai-providers` — store or replace a provider key.
    ///
    /// Body: `{ "provider": "openai" | "anthropic", "apiKey": "…" }`. See
    /// [`AI_PROVIDERS`].
    ///
    /// **The key is validated against the vendor before it is saved**, so
    /// this call makes a live outbound request and can take a second. Only
    /// an authentication-class *answer* refuses the save: a timeout or a
    /// vendor outage stores the key with `status: "unverified"` rather than
    /// losing it, which is why the response carries a `validation` object
    /// you should read instead of assuming a 201 means "working".
    ///
    /// `reindexQueued` counts the KB articles this install queued for
    /// embedding — zero for every provider but OpenAI, and capped, so the
    /// remainder index on their next publish. It is reported rather than
    /// implied for exactly that reason.
    ///
    /// Answers **201**, and replacing an existing key is also a 201.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown
    /// provider or a key the vendor rejects as unauthenticated.
    pub fn add_provider(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/ai-providers", &Query::new(), Some(body))
    }

    /// `POST /v1/ai-providers/{provider}/test` — re-check the **stored**
    /// key against the vendor.
    ///
    /// Takes no body: the key under test is the one already saved, so this
    /// is not a way to try a candidate key without storing it. Use it to
    /// tell "the key was revoked at the vendor" apart from "our call failed
    /// for some other reason" — the stored row is updated with the outcome.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no key is stored
    /// for that provider.
    pub fn test_provider(&self, provider: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!("/v1/ai-providers/{}/test", encode_path_segment(provider)),
            &Query::new(),
            None,
        )
    }

    /// `GET /v1/ai/quota` — this workspace's AI spend against its ceiling.
    ///
    /// The meter behind the subsidised-spend guard: spend so far, the cap,
    /// and a projection. Worth polling before a batch of autonomous runs,
    /// because the ceiling is enforced at call time and a run that crosses
    /// it stops mid-way rather than being refused up front.
    ///
    /// It meters **subsidised** spend only. Calls made on a key you supplied
    /// through [`Ai::add_provider`] are recorded but not capped, so a
    /// BYOK workspace can show a quota it will never hit.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn quota(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/ai/quota", query)
    }

    /// `GET /v1/ai/tone` — the tone-of-voice rules, as a **bare JSON
    /// array**.
    ///
    /// Ordered by category then position, and bounded at 40 rules (8 in each
    /// of 5 categories), so there is no cursor and no iterator.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn tone(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/ai/tone", query)
    }

    /// `PUT /v1/ai/tone` — replace **the whole rule set**.
    ///
    /// Body: `{ "rules": [ … ] }`. A PUT and not a PATCH, so a rule absent
    /// from the array is deleted; there is no per-rule route to add or
    /// remove one. Read [`Ai::tone`] first, edit the array, send it back.
    ///
    /// Answers the stored rules as a **bare array** — the same shape the GET
    /// returns, re-read after the write, not the `{ rules: … }` envelope you
    /// sent.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if a category is
    /// unknown or over its 8-rule limit.
    pub fn set_tone(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("PUT", "/v1/ai/tone", &Query::new(), Some(body))
    }

    /// `GET /v1/ai/tone/preview` — the rendered block as
    /// `{ "instruction": "…" }`.
    ///
    /// The exact text that is pasted into every customer-facing system
    /// prompt, which is what makes a rule that is being ignored debuggable.
    /// **An empty string is the honest answer for a workspace that has
    /// configured nothing** — no default house style is invented, so do not
    /// treat `""` as a failure.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn tone_preview(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/ai/tone/preview", query)
    }

    /// `POST /v1/ai/tone/generate` — turn a pasted style guide into
    /// proposed rules.
    ///
    /// Body: `{ "styleGuide": "…" }`, 40 to 20,000 characters after
    /// trimming, and the schema is `.strict()` — an extra key is a 400
    /// rather than being ignored.
    ///
    /// **It does not save anything.** The response is a draft for an
    /// operator to approve and then send through [`Ai::set_tone`]; an LLM
    /// reading somebody's brand book should not silently rewrite the thing
    /// that governs every reply. Answers **201** regardless, because Nest
    /// answers a POST with 201 absent an override — a client that insists on
    /// 200 here breaks on the real response.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a style guide
    /// under 40 or over 20,000 characters.
    pub fn generate_tone(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/ai/tone/generate", &Query::new(), Some(body))
    }

    /// `GET /v1/ai/tools` — the workspace's custom tools, as a **bare JSON
    /// array**.
    ///
    /// A bare array, unlike [`Ai::providers`] two routes up. Secret header
    /// *values* are omitted from every row; the header names are shown.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn tools(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/ai/tools", query)
    }

    /// `POST /v1/ai/tools` — define a tool the AI agent may call.
    ///
    /// A tool is an outbound HTTP request the model can make on your
    /// behalf: `name`, `description`, `method`, `urlTemplate`, `headers`,
    /// `secretHeaders`, `bodyTemplate`, `parameters`, `timeoutSeconds`, and
    /// the approval trio `requiresApproval` / `approvalThresholdParam` /
    /// `approvalThresholdValue`.
    ///
    /// Put credentials in `secretHeaders`, never in `headers`: the former is
    /// encrypted at rest and never read back, the latter is returned in
    /// plain text by [`Ai::tools`] to anyone who can list tools.
    ///
    /// Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    pub fn create_tool(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/ai/tools", &Query::new(), Some(body))
    }

    /// `PATCH /v1/ai/tools/{id}` — update a tool.
    ///
    /// **Omitting `secretHeaders` keeps the stored values; sending `{}`
    /// clears them.** That asymmetry is the one thing to get right here,
    /// because a client that helpfully fills in every field of its own model
    /// will wipe the credentials on the first save.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such tool.
    pub fn update_tool(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/ai/tools/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/ai/tools/{id}` — answers `{ "deleted": true }` with a
    /// 200, not a 204.
    ///
    /// A soft delete: the row keeps its `deleted_at` so an in-flight tool
    /// call that names it can still be explained after the fact.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such tool.
    pub fn delete_tool(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/ai/tools/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_vocabulary_is_the_two_implemented_ones() {
        assert_eq!(AI_PROVIDERS.len(), 2);
        assert!(AI_PROVIDERS.contains(&"openai"));
        assert!(AI_PROVIDERS.contains(&"anthropic"));
        // Not a provider abstraction Chatly implements.
        assert!(!AI_PROVIDERS.contains(&"gemini"));
    }
}
