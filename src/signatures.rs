//! One verifier for every signature Chatly puts on an outbound request.
//!
//! A port of `verifyChatlySignature` in `packages/sdk-node/src/signatures.ts`,
//! which is the reference: where this file and that one could disagree, the
//! cross-SDK corpus in `tests/fixtures/signature_vectors.json` decides, and
//! every SDK runs the same copy.
//!
//! # Why it takes a surface
//!
//! Because the header name does not determine the construction. Three
//! surfaces send `x-chatly-signature` and sign three different strings, and
//! two of the eight send MILLISECONDS. The caller knows which route they are
//! terminating; this function cannot. See [`crate::ChatlySignatureSurface`].
//!
//! # Why it returns a reason instead of a `bool`
//!
//! [`crate::verify_webhook_signature`] returns `false` for nine distinct
//! failures, and an integrator debugging a rejected delivery cannot tell
//! "your clock is wrong" from "your secret is wrong" from "you passed the
//! parsed body". Each of those is a support ticket. This returns a
//! [`ChatlySignatureResult`], and the refusal's `message` is written to be
//! printed.
//!
//! # Why it never panics
//!
//! CONTRACT.md §6: "a verifier that throws is an accepted webhook in a
//! handler that forgot to catch". In Rust the equivalent is an unwind out of
//! a handler, or an abort of the whole server under `panic = "abort"`. Every
//! value read off the request is shape-checked before it is used: digests
//! are checked as exactly 64 hex characters BEFORE decoding, lengths are
//! compared before the constant-time compare, a timestamp that does not fit
//! a `u64` (or overflows when seconds become milliseconds) is
//! `malformed_timestamp`, and all clock arithmetic is done in `i128`, where
//! no `u64` or `i64` input can overflow it.

use crate::crypto;
use crate::signature_input::{
    ChatlySignatureInput, ChatlySignatureRefusal, ChatlySignatureResult, ChatlySignatureVerified,
};
use crate::signature_parse::{
    capitalise, header_values, is_hex_sha256, js_trim, parse_tv1, preview, system_now_ms,
};
use crate::signature_table::{
    ChatlySignatureBase, ChatlySignatureFailure, ChatlySignatureSurface, ChatlyV2Policy,
    SignatureFormat, SurfaceSpec, TimestampFrom, TimestampUnit, CHATLY_V2_SIGNATURE_HEADER,
};

fn fail(surface: &str, reason: ChatlySignatureFailure, message: String) -> ChatlySignatureResult {
    ChatlySignatureResult::Refused(ChatlySignatureRefusal {
        surface: surface.to_owned(),
        reason,
        message,
    })
}

/// Verify one signed request from Chatly. Never panics.
///
/// ```
/// use chatly::{verify_chatly_signature, ChatlySignatureInput, ChatlySignatureSurface};
///
/// let body = br#"{"id":"evt_1"}"#;
/// let header = "t=1700000000,v1=c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925";
/// let result = verify_chatly_signature(
///     &ChatlySignatureInput::new(ChatlySignatureSurface::EventWebhook, body)
///         .header("x-livechat-signature", header)
///         .secret("whsec_test")
///         .now_ms(1_700_000_000_000),
/// );
/// assert!(result.is_ok());
/// ```
pub fn verify_chatly_signature(input: &ChatlySignatureInput<'_>) -> ChatlySignatureResult {
    let Some(surface) = ChatlySignatureSurface::from_name(input.surface) else {
        let known: Vec<&str> = ChatlySignatureSurface::ALL
            .iter()
            .map(|s| s.as_str())
            .collect();
        return fail(
            input.surface,
            ChatlySignatureFailure::UnknownSurface,
            format!(
                "{:?} is not a Chatly signature surface. One of: {}.",
                preview(input.surface),
                known.join(", ")
            ),
        );
    };
    let spec = surface.spec();
    let name = surface.as_str();

    // The CALLER's index survives the filter: `["", s]` must report 1 for
    // `s`, or a receiver retiring the old secret reads the wrong slot.
    let secrets: Vec<(usize, &str)> = input
        .secrets
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| (i, *s))
        .collect();
    if secrets.is_empty() {
        return fail(
            name,
            ChatlySignatureFailure::MissingSecret,
            format!(
                "No secret given for {name}. Each surface has its own secret — {}. \
                 A secret from another surface will never verify.",
                spec.describe
            ),
        );
    }

    if surface == ChatlySignatureSurface::WorkflowWebhookLegacy && !input.allow_unprotected_replay {
        return fail(
            name,
            ChatlySignatureFailure::ReplayUnprotected,
            "The legacy workflow header signs the body with no timestamp, so a captured request \
             verifies forever and nothing here can bound a replay. The same call also sends \
             x-chatly-signature-v2 — verify that instead, with surface \"workflow_webhook\". If \
             you must accept the legacy header, set allow_unprotected_replay and make the \
             endpoint idempotent."
                .to_owned(),
        );
    }

    if let Some(answered) = apply_v2_policy(input, surface, &spec, &secrets) {
        return answered;
    }
    verify_resolved(input, surface, input.v2, &secrets)
}

/// The v2 decision, taken before anything is parsed. `Some` when it has
/// answered the whole question, `None` to continue on the surface asked for.
fn apply_v2_policy(
    input: &ChatlySignatureInput<'_>,
    surface: ChatlySignatureSurface,
    spec: &SurfaceSpec,
    secrets: &[(usize, &str)],
) -> Option<ChatlySignatureResult> {
    let policy = input.v2;
    // A surface whose OWN header is the v2 header handles both bases in its
    // candidate list instead; there is nothing to delegate to.
    if policy == ChatlyV2Policy::Ignore
        || surface == ChatlySignatureSurface::UnifiedV2
        || spec.signature_header == CHATLY_V2_SIGNATURE_HEADER
    {
        return None;
    }
    let name = surface.as_str();
    let present = !header_values(&input.headers, CHATLY_V2_SIGNATURE_HEADER).is_empty();
    if !present {
        if policy == ChatlyV2Policy::Require {
            return Some(fail(
                name,
                ChatlySignatureFailure::MissingSignatureHeader,
                format!(
                    "v2 Require was asked for and this request carries no \
                     {CHATLY_V2_SIGNATURE_HEADER} header. {} alone means the sender has not \
                     moved to the unified construction yet — use the default v2 Prefer until \
                     it has.",
                    spec.signature_header
                ),
            ));
        }
        return None;
    }
    if !input.has_target() {
        if policy == ChatlyV2Policy::Require {
            return Some(fail(
                name,
                ChatlySignatureFailure::MissingRequestTarget,
                format!(
                    "The unified {CHATLY_V2_SIGNATURE_HEADER} signs <t>.<METHOD>.<path>.<rawBody>, \
                     so both method and path are required to verify it. path is pathname + \
                     search of the request as it arrived — not the host, not the scheme."
                ),
            ));
        }
        // Prefer, with no target: the legacy header is still sent beside it,
        // so fall through rather than refuse a request the caller can verify.
        return None;
    }

    // Once v2 is chosen there is NO cross-fall-back to the legacy header.
    // Both come from the same secret, so a good legacy digest beside a bad v2
    // one is tampering or a sender bug, and accepting it would hide both.
    match verify_resolved(
        input,
        ChatlySignatureSurface::UnifiedV2,
        ChatlyV2Policy::Ignore,
        secrets,
    ) {
        ok @ ChatlySignatureResult::Verified(_) => Some(ok),
        ChatlySignatureResult::Refused(r) => Some(fail(
            ChatlySignatureSurface::UnifiedV2.as_str(),
            r.reason,
            format!(
                "{} (This request carried {CHATLY_V2_SIGNATURE_HEADER}, so it was verified as \
                 the unified v2 construction rather than as {name}. Use v2 Ignore to verify {} \
                 instead.)",
                r.message, spec.signature_header
            ),
        )),
    }
}

/// Steps 5–12 of the reference, for a known surface and a resolved policy.
fn verify_resolved(
    input: &ChatlySignatureInput<'_>,
    surface: ChatlySignatureSurface,
    policy: ChatlyV2Policy,
    secrets: &[(usize, &str)],
) -> ChatlySignatureResult {
    let spec = surface.spec();
    let name = surface.as_str();

    // ── The digest header ──────────────────────────────────────────────────
    let sig_headers = header_values(&input.headers, spec.signature_header);
    let raw_signature = match sig_headers.as_slice() {
        [] => {
            return fail(
                name,
                ChatlySignatureFailure::MissingSignatureHeader,
                format!(
                    "No {} header. {} always sends one; if the request carries a differently \
                     named signature header, you have the wrong surface.",
                    spec.signature_header,
                    capitalise(spec.describe)
                ),
            )
        }
        [one] => js_trim(one),
        many => {
            // Two values means something between Chatly and you merged or
            // injected one, and picking either is a guess. Refuse rather than
            // trial-verify both: trial-verifying lets an attacker who can add
            // a header ride along on a genuine one.
            return fail(
                name,
                ChatlySignatureFailure::DuplicateSignatureHeader,
                format!(
                    "{} {} headers arrived. Chatly sends exactly one; a proxy merged or added \
                     one, and choosing between them would be a guess.",
                    many.len(),
                    spec.signature_header
                ),
            );
        }
    };

    let digests_hex: Vec<&str>;
    let mut raw_timestamp: Option<&str> = None;
    let mut marker: Option<&str> = None;
    match spec.format {
        SignatureFormat::Tv1 => {
            let Some(parsed) = parse_tv1(raw_signature) else {
                return fail(
                    name,
                    ChatlySignatureFailure::MalformedSignatureHeader,
                    format!(
                        "{} should be \"t=<unix seconds>,v1=<hex>\" on {name}; got {:?}.",
                        spec.signature_header,
                        preview(raw_signature)
                    ),
                );
            };
            digests_hex = parsed.v1;
            raw_timestamp = Some(parsed.t);
            marker = parsed.b;
        }
        SignatureFormat::Sha256 => {
            let Some(rest) = raw_signature.strip_prefix("sha256=") else {
                return fail(
                    name,
                    ChatlySignatureFailure::MalformedSignatureHeader,
                    format!(
                        "{} should be \"sha256=<hex>\" on {name}; got {:?}. A \"t=…,v1=…\" value \
                         here means you have the wrong surface — that is the other signature \
                         family.",
                        spec.signature_header,
                        preview(raw_signature)
                    ),
                );
            };
            digests_hex = vec![js_trim(rest)];
        }
    }

    // Shape-checked BEFORE decoding, so a garbage digest is reported as
    // malformed rather than reaching the compare as a length mismatch and
    // being reported as a wrong secret.
    if let Some(bad) = digests_hex.iter().find(|d| !is_hex_sha256(d)) {
        let many = if digests_hex.len() > 1 {
            format!(
                " {} v1 values arrived; every one has to be well formed.",
                digests_hex.len()
            )
        } else {
            String::new()
        };
        return fail(
            name,
            ChatlySignatureFailure::MalformedSignatureHeader,
            format!(
                "A digest in {} is not 64 hex characters (got {}).{many}",
                spec.signature_header,
                // UTF-16 length, matching the reference's `.length`.
                bad.encode_utf16().count()
            ),
        );
    }

    // ── The timestamp, and its unit ────────────────────────────────────────
    let mut timestamp_ms: Option<u64> = None;
    if let Some(unit) = spec.unit {
        if let TimestampFrom::Header(ts_header) = spec.timestamp_from {
            let values = header_values(&input.headers, ts_header);
            let Some(first) = values.first() else {
                return fail(
                    name,
                    ChatlySignatureFailure::MissingTimestamp,
                    format!(
                        "No {ts_header} header. On {name} the timestamp travels in its own header \
                         and is part of the signed string, so it cannot be reconstructed."
                    ),
                );
            };
            raw_timestamp = Some(js_trim(first));
        }
        let raw = match raw_timestamp {
            Some(raw) if !raw.is_empty() => raw,
            _ => {
                return fail(
                    name,
                    ChatlySignatureFailure::MissingTimestamp,
                    format!("No timestamp found for {name}."),
                )
            }
        };
        // ASCII digits only, checked before `parse`: `u64::from_str` accepts
        // a leading `+`, and the reference's `^\d+$` does not.
        if !raw.bytes().all(|b| b.is_ascii_digit()) {
            return fail(
                name,
                ChatlySignatureFailure::MalformedTimestamp,
                format!(
                    "The timestamp on {name} is not a run of digits: {:?}.",
                    preview(raw)
                ),
            );
        }
        let scaled = raw.parse::<u64>().ok().and_then(|n| match unit {
            TimestampUnit::Seconds => n.checked_mul(1000),
            TimestampUnit::Milliseconds => Some(n),
        });
        let Some(signed_ms) = scaled else {
            // The reference reads this as a finite JavaScript number and then
            // refuses it on time; here it does not fit the type, which is the
            // same request refused one step earlier and named for what it is.
            return fail(
                name,
                ChatlySignatureFailure::MalformedTimestamp,
                format!(
                    "The timestamp on {name} does not fit in 64 bits as milliseconds: {:?}.",
                    preview(raw)
                ),
            );
        };
        timestamp_ms = Some(signed_ms);

        let now_ms = i128::from(input.now_ms.unwrap_or_else(system_now_ms));
        // i128 throughout: no u64 timestamp, i64 clock or u64 tolerance can
        // overflow it, and a wrapped difference is a request accepted outside
        // the window.
        let drift_ms = (now_ms - i128::from(signed_ms)).abs();
        let tolerance_ms = i128::from(input.tolerance_seconds) * 1000;
        if drift_ms > tolerance_ms {
            // Both directions are refused, and the message says which. A
            // FAR-FUTURE timestamp is usually the wrong unit, which lands
            // ~54,000 years out one way or ~54 years the other and reads as
            // "expired" unless something says so.
            let year_ms: i128 = 1000 * 60 * 60 * 24 * 365;
            let drift_years = drift_ms / year_ms;
            let unit_hint = if drift_ms > year_ms {
                format!(
                    " That is {drift_years} year(s) out, which is what a wrong UNIT looks like: \
                     {name} sends {}.",
                    match unit {
                        TimestampUnit::Seconds => "seconds",
                        TimestampUnit::Milliseconds => "milliseconds",
                    }
                )
            } else {
                String::new()
            };
            let direction = if i128::from(signed_ms) > now_ms {
                "in the FUTURE"
            } else {
                "in the past"
            };
            return fail(
                name,
                ChatlySignatureFailure::TimestampOutsideTolerance,
                format!(
                    "The timestamp on {name} is {}s {direction}, outside the {}s tolerance.\
                     {unit_hint}",
                    drift_ms / 1000,
                    input.tolerance_seconds
                ),
            );
        }
    }

    // ── The base string, and the compare ───────────────────────────────────
    let bases = match candidate_bases(input, surface, &spec, policy, marker) {
        Ok(bases) => bases,
        Err(refusal) => return refusal,
    };
    if bases.contains(&ChatlySignatureBase::Target) && !input.has_target() {
        return fail(
            name,
            ChatlySignatureFailure::MissingRequestTarget,
            format!(
                "{name} signs <t>.<METHOD>.<path>.<rawBody>, so both method and path are \
                 required. path is pathname + search of the request as Chatly rendered it — not \
                 the host, not the scheme."
            ),
        );
    }

    // Every digest already passed `is_hex_sha256`, so none decodes to
    // `None`; `filter_map` rather than an unwrap keeps that a non-panic even
    // if the check above were ever loosened.
    let given: Vec<Vec<u8>> = digests_hex
        .iter()
        .filter_map(|d| crypto::hex_decode(d))
        .collect();
    let t = raw_timestamp.unwrap_or("");
    for base in bases {
        let prefix = base_prefix(base, t, input);
        for &(index, secret) in secrets {
            let expected = crypto::hmac_sha256(secret.as_bytes(), &[prefix.as_bytes(), input.body]);
            // `constant_time_eq` compares lengths first and then folds every
            // byte without an early exit.
            if given.iter().any(|g| crypto::constant_time_eq(g, &expected)) {
                return ChatlySignatureResult::Verified(ChatlySignatureVerified {
                    surface,
                    timestamp_ms,
                    base,
                    secret_index: index,
                });
            }
        }
    }

    let tried = if secrets.len() > 1 {
        format!(" None of the {} secrets matched.", secrets.len())
    } else {
        String::new()
    };
    fail(
        name,
        ChatlySignatureFailure::SignatureMismatch,
        format!(
            "The digest does not match for {name}. The three usual causes, in order: a \
             re-serialised body (pass the RAW bytes, not a re-encoding of the parsed JSON), the \
             secret from a different surface, or the wrong surface — {}.{tried}",
            spec.describe
        ),
    )
}

/// Which strings the MAC may have been taken over, most specific first — or
/// a refusal when the value's `b=` marker rules every candidate out.
///
/// The marker is read ONLY off `x-chatly-signature-v2` (no legacy header
/// carries one) and it NARROWS the candidates, never widens them past what
/// the surface and the policy allow, because anyone on the wire can write a
/// marker. An absent or unrecognised marker changes nothing: every v2 value
/// minted before the marker existed is absent.
fn candidate_bases(
    input: &ChatlySignatureInput<'_>,
    surface: ChatlySignatureSurface,
    spec: &SurfaceSpec,
    policy: ChatlyV2Policy,
    marker: Option<&str>,
) -> Result<Vec<ChatlySignatureBase>, ChatlySignatureResult> {
    let from_v2 = spec.signature_header == CHATLY_V2_SIGNATURE_HEADER;
    if from_v2
        && marker == Some("b")
        && (spec.base == ChatlySignatureBase::Target || policy == ChatlyV2Policy::Require)
    {
        let tail = if surface == ChatlySignatureSurface::UnifiedV2 {
            "unified_v2 accepts only b=t. Verify it with surface \"workflow_webhook\", which \
             accepts both."
        } else {
            "v2 Require accepts only the unified base (b=t)."
        };
        return Err(fail(
            surface.as_str(),
            ChatlySignatureFailure::SignatureMismatch,
            format!(
                "This {CHATLY_V2_SIGNATURE_HEADER} is marked b=b: it was signed over \
                 <t>.<rawBody>, the workflow action's pre-unified base. {tail}"
            ),
        ));
    }
    if !spec.also_accepts_target_base || policy == ChatlyV2Policy::Ignore {
        return Ok(vec![spec.base]);
    }
    if marker == Some("t") {
        // Narrowed to the unified base; with no target this becomes
        // `missing_request_target`, which is the error that names the fix.
        return Ok(vec![ChatlySignatureBase::Target]);
    }
    if marker == Some("b") || !input.has_target() {
        return Ok(vec![spec.base]);
    }
    if policy == ChatlyV2Policy::Require {
        return Ok(vec![ChatlySignatureBase::Target]);
    }
    Ok(vec![ChatlySignatureBase::Target, spec.base])
}

/// Everything before the raw body in the signed string. `t` is the
/// timestamp text EXACTLY as received — re-rendering a parsed integer would
/// change the digest for a zero-padded value the real producer signed.
fn base_prefix(base: ChatlySignatureBase, t: &str, input: &ChatlySignatureInput<'_>) -> String {
    match base {
        ChatlySignatureBase::BodyOnly => String::new(),
        ChatlySignatureBase::Body => format!("{t}."),
        ChatlySignatureBase::Target => format!(
            "{t}.{}.{}.",
            input.method.unwrap_or("").to_uppercase(),
            input.path.unwrap_or("")
        ),
    }
}
