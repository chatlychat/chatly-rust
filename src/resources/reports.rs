//! Reports — the aggregates behind the dashboard's analytics pages.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `dimension` values [`Reports::attribution`] accepts.
///
/// Ten, and they are the page-context keys the widget captures when a
/// conversation opens — not an analytics vocabulary of our own. An unknown
/// value is a 400 rather than a silent fall back to `utm_source`.
pub const ATTRIBUTION_DIMENSIONS: [&str; 10] = [
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
    "referrer_url",
    "page_url_at_open",
    "device_type",
    "browser",
    "os",
];

/// The `reports` namespace. Get one from [`ChatlyClient::reports`].
///
/// These reports do **not** share a parameter vocabulary, which is the one
/// thing to know before using them:
///
/// | Report | Parameters | `days` default | Ceiling |
/// | --- | --- | --- | --- |
/// | [`Reports::overview`] | `days` only | 7 | 90 |
/// | [`Reports::agents`] | `days` only | 7 | 90 |
/// | [`Reports::sources`] | `days` only | 7 | 90 |
/// | [`Reports::conversations_by_day`] | `days` only | 14 | 90 |
/// | [`Reports::attribution`] | `days`, `dimension` | 30 | 90 |
/// | [`Reports::response_times`] | `days` only | 30 | 90 |
/// | [`Reports::voice`] | `days` only | 30 | **365** |
/// | [`Reports::cohorts`] | `weeks` (2–26) | 8 weeks | 26 weeks |
/// | [`Reports::sla`] | `days`, `from`, `to`, `group_by` | 30 | 366 |
/// | [`Reports::teams`] | `from`, `to`, `business_hours` | 30 | — |
/// | [`Reports::labels`] | `from`, `to`, `business_hours`, `group_by` | 30 | — |
/// | [`Reports::themes`] | `date` (`YYYY-MM-DD`) | latest batch | — |
///
/// **Most of them clamp silently.** An out-of-range or unparseable `days`
/// becomes the default or the ceiling rather than an error, so a typo
/// returns a plausible number for the wrong window. The exceptions refuse
/// instead: [`Reports::sla`] 4xxs on a half-specified range,
/// [`Reports::attribution`] on an unknown `dimension`, and
/// [`Reports::themes`] on a `date` that is not a calendar date.
///
/// Parameters do not carry across. `days` does nothing on the
/// `from`/`to` reports, `business_hours` does nothing on [`Reports::sla`]
/// (which is the one route in this family that does not accept it), and
/// `group_by` is read only by [`Reports::sla`] and [`Reports::labels`].
/// Nothing complains about a parameter a route ignores.
///
/// **Four of these answer bare arrays** — [`Reports::agents`],
/// [`Reports::conversations_by_day`], [`Reports::sources`] and
/// [`Reports::attribution`] — and the rest answer objects. There is no way
/// to tell from the name, which is why every method says so.
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

    /// `GET /v1/reports/agents` — per-agent performance, as a **JSON
    /// array** of one row per agent.
    ///
    /// `days` only: 1–90, default 7.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn agents(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/agents", query)
    }

    /// `GET /v1/reports/conversations-by-day` — a daily time series, as a
    /// **JSON array** of `{ day, count }`.
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

    /// `GET /v1/reports/sources` — volume by channel of origin, as a **bare
    /// JSON array**.
    ///
    /// `days` only: 1–90, default 7, clamped.
    ///
    /// **`channelId` is null for a conversation whose channel row is
    /// gone** — deleted or never created — and in that case `channelName` is
    /// a human label for the *type* rather than null. So do not key this
    /// report on `channelId`; one row legitimately has none.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn sources(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/sources", query)
    }

    /// `GET /v1/reports/attribution` — volume and resolution grouped by
    /// captured campaign data, as a **bare JSON array**.
    ///
    /// `dimension` is one of [`ATTRIBUTION_DIMENSIONS`], default
    /// `utm_source`, and an unknown value is **refused with a 400 rather
    /// than defaulted** — a typo silently answered as `utm_source` would be
    /// a confident report of the wrong thing with no way to notice. `days`
    /// is 1–90, default **30** (not 7, unlike its neighbours).
    ///
    /// Two things about the rows:
    ///
    /// - **The row with `value: null` is the untagged bucket and is real.**
    ///   Dropping it inflates every other `percentage`.
    /// - The tail beyond the row cap is summed into a single `(other)` row,
    ///   so the array is bounded however many distinct values exist and
    ///   there is nothing to page to.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown
    /// dimension.
    pub fn attribution(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/attribution", query)
    }

    /// `GET /v1/reports/cohorts` — the weekly contact-retention grid.
    ///
    /// **One object, not a bare array of cells.** `weeks` is 2–26, default
    /// 8, clamped — and the response **echoes the window it actually
    /// used**, which can differ from what you asked for. Label your axes
    /// from the echoed value, not from your input.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn cohorts(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/cohorts", query)
    }

    /// `GET /v1/reports/response-times` — first-response and resolution
    /// percentiles with the sample size.
    ///
    /// `days` only: 1–90, default 30. One object.
    ///
    /// **Percentiles are seconds and are `null` when `sampleSize` is 0** —
    /// not zero, which would read as "instant". It also carries a `byDay`
    /// series of the same shape [`Reports::conversations_by_day`] returns
    /// *bare*, so the two routes are not interchangeable even where the
    /// numbers overlap.
    ///
    /// Distinct from `reports/analytics/volume`, which is a bucketed series
    /// off a materialised view and is not key-accessible.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn response_times(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/response-times", query)
    }

    /// `GET /v1/reports/teams` — per-team volume, percentiles, CSAT and SLA
    /// breaches.
    ///
    /// One object whose `rows` hold one entry per team **plus a `null`
    /// `teamId` row for the unassigned bucket** — a real row, not a
    /// placeholder.
    ///
    /// Takes `from`/`to` (ISO date or instant; inclusive and exclusive) and
    /// `business_hours` — snake_case, like `group_by` on [`Reports::sla`]
    /// and unlike everything else in this crate. Default window is 30 days.
    ///
    /// **Read `businessHours.applied` before captioning the durations.**
    /// `false` means they are calendar time and `businessHours.reason` says
    /// why — the figures are never zeroed and never relabelled, so a caption
    /// that says "business hours" can be wrong while the numbers are right.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn teams(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/teams", query)
    }

    /// `GET /v1/reports/labels` — volume, resolution time and CSAT by label.
    ///
    /// One object. Takes `from`/`to`, `business_hours` and `group_by`
    /// (`hour` | `day` | `week` | `month`).
    ///
    /// **Capped and uncursored in two different ways**, which is the thing
    /// to know: `rows` holds the 50 busiest labels with the remainder
    /// counted in `otherLabels`, and `trend` is emitted for the **top 8
    /// only**. There is no cursor, so the tail is not reachable from here at
    /// all. `trend[].counts` is keyed by label *name*.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn labels(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/labels", query)
    }

    /// `GET /v1/reports/voice` — call volume, answer rate, talk time,
    /// heatmap and per-agent rollup.
    ///
    /// `days` only, and the ceiling is **365** here rather than the 90 every
    /// other report uses; default 30.
    ///
    /// Two contract details worth more than the field names:
    /// **rates are 0–1, not percentages**, and
    /// `recordingStorage.exact: false` means `totalBytesUpperBound` is part
    /// measured and part bitrate extrapolation over `unmeasuredCalls`. That
    /// flag is as much part of the answer as the byte count — billing a
    /// customer from the number without it would be billing an estimate.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn voice(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/voice", query)
    }

    /// `GET /v1/reports/themes` — the latest theme batch: what customers
    /// wrote about, clustered.
    ///
    /// **One object, not a bare array.** `batchDate` names the snapshot
    /// every row was measured over, and is `null` with `themes` empty when
    /// no batch has ever been generated — which is the ordinary answer for a
    /// young workspace, not an error.
    ///
    /// `date` is **`YYYY-MM-DD` and nothing else**. It reaches a `date`
    /// column, and until recently it was passed through untouched: `?date=
    /// yesterday` came back as a 500 from Postgres. It now 400s, and so does
    /// `2026-02-30` — a shape-valid non-date. Omit it for the newest batch.
    ///
    /// Theme rows **omit `conversationIds`** (a theme can hold hundreds);
    /// [`Reports::theme_conversations`] serves those on demand.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Api`] carrying a 400 for a `date`
    /// that is not a calendar date.
    pub fn themes(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/reports/themes", query)
    }

    /// `GET /v1/reports/themes/{id}/conversations` — the conversation ids in
    /// one theme, as the snapshot recorded them.
    ///
    /// One object wrapping the id list, bounded by the batch cap of 5,000.
    ///
    /// **An unknown or purged theme id answers 200 with an empty list, not a
    /// 404.** So this route cannot tell you apart "no such theme" from "a
    /// theme whose conversations were purged" — if you need that
    /// distinction, check the id against [`Reports::themes`] first.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn theme_conversations(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!(
                "/v1/reports/themes/{}/conversations",
                encode_path_segment(id)
            ),
            query,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_attribution_dimensions_are_the_page_context_keys() {
        assert_eq!(ATTRIBUTION_DIMENSIONS.len(), 10);
        assert_eq!(ATTRIBUTION_DIMENSIONS[0], "utm_source", "the default");
        assert!(ATTRIBUTION_DIMENSIONS.contains(&"referrer_url"));
        // snake_case keys as captured, not camelCase like the response fields.
        assert!(ATTRIBUTION_DIMENSIONS
            .iter()
            .all(|d| d.chars().all(|c| c.is_ascii_lowercase() || c == '_')));
    }
}
