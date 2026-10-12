//! Reports — the aggregates behind the dashboard's analytics pages.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::Query;

/// The `reports` namespace. Get one from [`ChatlyClient::reports`].
///
/// The four reports do **not** share a parameter vocabulary, which is the
/// one thing to know before using them:
///
/// | Report | Parameters | `days` default | Ceiling |
/// | --- | --- | --- | --- |
/// | [`Reports::overview`] | `days` only | 7 | 90 |
/// | [`Reports::agents`] | `days` only | 7 | 90 |
/// | [`Reports::conversations_by_day`] | `days` only | 14 | 90 |
/// | [`Reports::sla`] | `days`, `from`, `to`, `group_by` | 30 | 366 |
///
/// The first three clamp silently: an out-of-range or unparseable `days`
/// becomes the default or the ceiling rather than an error, so a typo
/// returns a plausible number for the wrong window. [`Reports::sla`] is
/// stricter and 4xxs instead.
///
/// `from`/`to` and `group_by` are accepted by [`Reports::sla`] alone;
/// passing them to the other three does nothing at all.
#[derive(Debug, Clone, Copy)]
pub struct Reports<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Reports<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/reports/overview` — headline volume and response metrics.
    ///
    /// `days` only: 1–90, default 7. Out-of-range values are clamped, not
    /// rejected.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn overview(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/overview", query)
    }

    /// `GET /v1/reports/agents` — per-agent performance.
    ///
    /// `days` only: 1–90, default 7.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn agents(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/agents", query)
    }

    /// `GET /v1/reports/conversations-by-day` — a daily time series.
    ///
    /// `days` only: 1–90, default **14** — not 7 like its two siblings.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn conversations_by_day(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/conversations-by-day", query)
    }

    /// `GET /v1/reports/sla` — SLA attainment, warnings and breaches.
    ///
    /// The only report with a real range vocabulary:
    ///
    /// - `days` — a preset, default 30, clamped to 366.
    /// - `from` and `to` — ISO date (`2026-07-01`) or instant. They must be
    ///   supplied **together**; one alone is a 400. `from` is inclusive,
    ///   `to` exclusive, and a bare date in `to` means through the end of
    ///   that day. An explicit range overrides `days`, and a range longer
    ///   than 366 days is a 400 rather than being clamped.
    /// - `group_by` — `hour`, `day`, `week` or `month`, default `day`.
    ///   `hour` is limited to 14 days.
    ///
    /// Note `group_by` is snake_case on the wire, unlike every other
    /// parameter in this crate. And unlike `/v1/reports/teams` and
    /// `/v1/reports/labels`, this report does not accept `business_hours`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Api`] carrying a 400 for a
    /// half-specified or oversized range.
    pub fn sla(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/sla", query)
    }

    /// [`Reports::sla`] over a `from`/`to` range with an optional bucket.
    ///
    /// Both bounds are required by the API, so both are required here —
    /// passing one and omitting the other is the shape that 400s.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn sla_range(
        &self,
        from: &str,
        to: &str,
        group_by: Option<&str>,
    ) -> Result<Value, ChatlyError> {
        let mut query = Query::new().set("from", from).set("to", to);
        if let Some(group_by) = group_by {
            query = query.set("group_by", group_by);
        }
        self.sla(&query)
    }
}
