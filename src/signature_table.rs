//! Which outbound surfaces Chatly signs, and what each one signs.
//!
//! A port of `packages/sdk-node/src/signature-table.ts`, which is the
//! reference. Split from [`crate::verify_chatly_signature`]'s module for the
//! same reason the TypeScript is: the data and the verifier each read on
//! their own, and each stays under the 650-line ceiling.
//!
//! ```text
//! | surface                 | header(s)                                             | base string                     | t unit  |
//! | ----------------------- | ----------------------------------------------------- | ------------------------------- | ------- |
//! | unified_v2              | x-chatly-signature-v2: t=…,v1=…                       | <t>.<METHOD>.<path>.<rawBody>   | seconds |
//! | agent_delivery          | x-chatly-signature: t=…,v1=…                          | <t>.<rawBody>                   | seconds |
//! | event_webhook           | x-livechat-signature: t=…,v1=…                        | <t>.<rawBody>                   | seconds |
//! | ai_tool_call            | x-chatly-signature: t=…,v1=…                          | <t>.<METHOD>.<path>.<rawBody>   | seconds |
//! | canvas_step             | x-chatly-canvas-signature: sha256=… + -ts             | <ts>.<rawBody>                  | MS      |
//! | customer_card           | x-chatly-signature: sha256=… + x-chatly-timestamp     | <ts>.<rawBody>                  | MS      |
//! | workflow_webhook        | x-chatly-signature-v2: t=…,v1=…                       | <t>.<rawBody>                   | seconds |
//! | workflow_webhook_legacy | x-chatly-signature: sha256=…                          | <rawBody> — NO timestamp        | n/a     |
//! ```
//!
//! **Three surfaces send `x-chatly-signature` and they sign three different
//! strings.** A verifier that keys on the header name can be pointed at the
//! wrong construction while looking completely correct, which is why the
//! surface is an input and is never inferred from the request.
//!
//! **Two surfaces send MILLISECONDS.** A millisecond timestamp read as
//! seconds lands roughly 54,000 years out, so a tolerance check rejects every
//! genuine request and the failure looks like a wrong secret. The unit is
//! therefore a property of the SURFACE here, never of the value.
//!
//! `workflow_webhook` already sends `x-chatly-signature-v2`, but over
//! `<t>.<rawBody>` — the v2 header predates the unified base. It accepts the
//! unified base as well whenever the caller supplies a method and a path, so
//! a receiver that passes both keeps verifying across the sender's switch.
//! `unified_v2` is strict and accepts only the target base.

use std::fmt;
use std::str::FromStr;

/// Five minutes, matching `SIGNATURE_TOLERANCE_SECONDS` on the producing side.
///
/// Applied in BOTH directions: a far-future timestamp is refused as firmly as
/// a stale one, because "the future" is what a wrong unit looks like.
pub const CHATLY_SIGNATURE_TOLERANCE_SECONDS: u64 = 300;

/// The header the unified v2 construction travels in, on every surface.
pub const CHATLY_V2_SIGNATURE_HEADER: &str = "x-chatly-signature-v2";

/// Which outbound surface produced the request being verified.
///
/// [`ChatlySignatureSurface::as_str`] and `Display` render the stable
/// snake_case names every Chatly SDK uses, and `FromStr` parses them back, so
/// a surface can live in configuration as a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatlySignatureSurface {
    /// `unified_v2` — the construction every surface is moving to.
    UnifiedV2,
    /// `agent_delivery` — external-agent push delivery.
    AgentDelivery,
    /// `event_webhook` — an event webhook subscription (`whsec_…`).
    EventWebhook,
    /// `ai_tool_call` — a custom AI tool call.
    AiToolCall,
    /// `canvas_step` — a canvas / interactive-step callback (milliseconds).
    CanvasStep,
    /// `customer_card` — a customer-card fetch (milliseconds).
    CustomerCard,
    /// `workflow_webhook` — the workflow "call a webhook" action's v2 header.
    WorkflowWebhook,
    /// `workflow_webhook_legacy` — the workflow action's legacy header, which
    /// carries NO timestamp and so cannot bound a replay.
    WorkflowWebhookLegacy,
}

impl ChatlySignatureSurface {
    /// Every surface, in the table's order.
    ///
    /// Exists so a test can assert every variant has a table entry and a
    /// round-tripping name: a surface added here and forgotten in
    /// [`ChatlySignatureSurface::spec`] would otherwise be a compile error at
    /// best and an `unknown_surface` refusal with a green suite at worst.
    pub const ALL: [ChatlySignatureSurface; 8] = [
        Self::UnifiedV2,
        Self::AgentDelivery,
        Self::EventWebhook,
        Self::AiToolCall,
        Self::CanvasStep,
        Self::CustomerCard,
        Self::WorkflowWebhook,
        Self::WorkflowWebhookLegacy,
    ];

    /// The stable snake_case name, identical across every Chatly SDK.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnifiedV2 => "unified_v2",
            Self::AgentDelivery => "agent_delivery",
            Self::EventWebhook => "event_webhook",
            Self::AiToolCall => "ai_tool_call",
            Self::CanvasStep => "canvas_step",
            Self::CustomerCard => "customer_card",
            Self::WorkflowWebhook => "workflow_webhook",
            Self::WorkflowWebhookLegacy => "workflow_webhook_legacy",
        }
    }

    /// Parse a stable name. Exact match only — `"Event_Webhook"` is `None`,
    /// because the TypeScript reference treats it as an unknown surface and
    /// the eight SDKs must agree on what a surface name is.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// This surface's row in the table.
    pub const fn spec(self) -> SurfaceSpec {
        // Kept as one `match` over data rather than branches in the verifier:
        // the verifier reads fields, so adding a surface is a table edit.
        match self {
            Self::UnifiedV2 => SurfaceSpec {
                signature_header: CHATLY_V2_SIGNATURE_HEADER,
                timestamp_from: TimestampFrom::InSignature,
                format: SignatureFormat::Tv1,
                unit: Some(TimestampUnit::Seconds),
                base: ChatlySignatureBase::Target,
                also_accepts_target_base: false,
                describe: "the unified v2 signature, sent alongside every surface's legacy \
                           header, over t.METHOD.path.body",
            },
            Self::AgentDelivery => SurfaceSpec {
                signature_header: "x-chatly-signature",
                timestamp_from: TimestampFrom::InSignature,
                format: SignatureFormat::Tv1,
                unit: Some(TimestampUnit::Seconds),
                base: ChatlySignatureBase::Body,
                also_accepts_target_base: false,
                describe: "external-agent push delivery, signed with the bot's signingSecret",
            },
            Self::EventWebhook => SurfaceSpec {
                signature_header: "x-livechat-signature",
                timestamp_from: TimestampFrom::InSignature,
                format: SignatureFormat::Tv1,
                unit: Some(TimestampUnit::Seconds),
                base: ChatlySignatureBase::Body,
                also_accepts_target_base: false,
                describe: "event webhook, signed with that subscription's whsec_ secret",
            },
            Self::AiToolCall => SurfaceSpec {
                signature_header: "x-chatly-signature",
                timestamp_from: TimestampFrom::InSignature,
                format: SignatureFormat::Tv1,
                unit: Some(TimestampUnit::Seconds),
                base: ChatlySignatureBase::Target,
                also_accepts_target_base: false,
                describe: "custom AI tool call, signed with that tool's secret over \
                           t.METHOD.path.body",
            },
            Self::CanvasStep => SurfaceSpec {
                signature_header: "x-chatly-canvas-signature",
                timestamp_from: TimestampFrom::Header("x-chatly-canvas-ts"),
                format: SignatureFormat::Sha256,
                unit: Some(TimestampUnit::Milliseconds),
                base: ChatlySignatureBase::Body,
                also_accepts_target_base: false,
                describe: "canvas / interactive-step callback, timestamp in MILLISECONDS",
            },
            Self::CustomerCard => SurfaceSpec {
                signature_header: "x-chatly-signature",
                timestamp_from: TimestampFrom::Header("x-chatly-timestamp"),
                format: SignatureFormat::Sha256,
                unit: Some(TimestampUnit::Milliseconds),
                base: ChatlySignatureBase::Body,
                also_accepts_target_base: false,
                describe: "customer-card fetch, timestamp in MILLISECONDS",
            },
            Self::WorkflowWebhook => SurfaceSpec {
                signature_header: CHATLY_V2_SIGNATURE_HEADER,
                timestamp_from: TimestampFrom::InSignature,
                format: SignatureFormat::Tv1,
                unit: Some(TimestampUnit::Seconds),
                base: ChatlySignatureBase::Body,
                // The one surface that sent the v2 header before the base
                // gained the method and path. See this module's header.
                also_accepts_target_base: true,
                describe: "workflow \"call a webhook\" action, replay-bounded v2 header",
            },
            Self::WorkflowWebhookLegacy => SurfaceSpec {
                signature_header: "x-chatly-signature",
                timestamp_from: TimestampFrom::None,
                format: SignatureFormat::Sha256,
                unit: None,
                base: ChatlySignatureBase::BodyOnly,
                also_accepts_target_base: false,
                describe: "workflow action, LEGACY header over the body alone — no replay \
                           protection",
            },
        }
    }
}

impl fmt::Display for ChatlySignatureSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error from parsing an unrecognised surface name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSurface(pub String);

impl fmt::Display for UnknownSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a Chatly signature surface", self.0)
    }
}

impl std::error::Error for UnknownSurface {}

impl FromStr for ChatlySignatureSurface {
    type Err = UnknownSurface;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s).ok_or_else(|| UnknownSurface(s.to_owned()))
    }
}

/// Which string the MAC is taken over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatlySignatureBase {
    /// `body` → `<t>.<rawBody>`
    Body,
    /// `target` → `<t>.<METHOD>.<path>.<rawBody>` (the unified base)
    Target,
    /// `body_only` → `<rawBody>`, no timestamp anywhere — unbounded replay
    BodyOnly,
}

impl ChatlySignatureBase {
    /// The stable snake_case name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Target => "target",
            Self::BodyOnly => "body_only",
        }
    }
}

impl fmt::Display for ChatlySignatureBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a verification failed. Stable strings, safe to branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatlySignatureFailure {
    /// `unknown_surface`
    UnknownSurface,
    /// `missing_secret`
    MissingSecret,
    /// `missing_signature_header`
    MissingSignatureHeader,
    /// `duplicate_signature_header`
    DuplicateSignatureHeader,
    /// `malformed_signature_header`
    MalformedSignatureHeader,
    /// `missing_timestamp`
    MissingTimestamp,
    /// `malformed_timestamp`
    MalformedTimestamp,
    /// `timestamp_outside_tolerance`
    TimestampOutsideTolerance,
    /// `missing_request_target`
    MissingRequestTarget,
    /// `replay_unprotected`
    ReplayUnprotected,
    /// `signature_mismatch`
    SignatureMismatch,
}

impl ChatlySignatureFailure {
    /// Every reason, so a test can assert each has a distinct name.
    pub const ALL: [ChatlySignatureFailure; 11] = [
        Self::UnknownSurface,
        Self::MissingSecret,
        Self::MissingSignatureHeader,
        Self::DuplicateSignatureHeader,
        Self::MalformedSignatureHeader,
        Self::MissingTimestamp,
        Self::MalformedTimestamp,
        Self::TimestampOutsideTolerance,
        Self::MissingRequestTarget,
        Self::ReplayUnprotected,
        Self::SignatureMismatch,
    ];

    /// The stable snake_case name, identical across every Chatly SDK.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownSurface => "unknown_surface",
            Self::MissingSecret => "missing_secret",
            Self::MissingSignatureHeader => "missing_signature_header",
            Self::DuplicateSignatureHeader => "duplicate_signature_header",
            Self::MalformedSignatureHeader => "malformed_signature_header",
            Self::MissingTimestamp => "missing_timestamp",
            Self::MalformedTimestamp => "malformed_timestamp",
            Self::TimestampOutsideTolerance => "timestamp_outside_tolerance",
            Self::MissingRequestTarget => "missing_request_target",
            Self::ReplayUnprotected => "replay_unprotected",
            Self::SignatureMismatch => "signature_mismatch",
        }
    }
}

impl fmt::Display for ChatlySignatureFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How hard to insist on the unified v2 construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChatlyV2Policy {
    /// The default. If `x-chatly-signature-v2` is present AND a method and
    /// path were supplied, verify THAT and nothing else; otherwise verify the
    /// surface's own legacy construction. A receiver that passes its method
    /// and path gets the stronger check the day the sender starts emitting
    /// v2, with no code change.
    #[default]
    Prefer,
    /// Refuse unless the v2 header is there.
    Require,
    /// The surface's own construction only.
    Ignore,
}

/// Where a surface's timestamp comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampFrom {
    /// The digest header's own `t=` field.
    InSignature,
    /// A header of its own, by name.
    Header(&'static str),
    /// The surface sends none.
    None,
}

/// The shape of a signature header's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureFormat {
    /// `t=<timestamp>,v1=<hex>[,v1=<hex>…][,b=<marker>]`
    Tv1,
    /// `sha256=<hex>`
    Sha256,
}

/// The unit a surface's timestamp is sent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampUnit {
    /// Unix seconds.
    Seconds,
    /// Unix milliseconds.
    Milliseconds,
}

/// One row of the table. See [`ChatlySignatureSurface::spec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSpec {
    /// The header carrying the digest.
    pub signature_header: &'static str,
    /// Where `t` comes from.
    pub timestamp_from: TimestampFrom,
    /// `t=…,v1=<hex>` or `sha256=<hex>`.
    pub format: SignatureFormat,
    /// The unit of the timestamp, `None` when there is none.
    pub unit: Option<TimestampUnit>,
    /// The string the MAC is taken over.
    pub base: ChatlySignatureBase,
    /// True when the UNIFIED base is also accepted on this surface's own
    /// header, for a sender mid-migration. Only `workflow_webhook`.
    pub also_accepts_target_base: bool,
    /// A one-line description, for an error message and for documentation.
    pub describe: &'static str,
}
