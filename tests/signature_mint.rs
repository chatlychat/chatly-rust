//! `sign_chatly_v2`, and the three refusals the brief for this port calls out
//! by name.
//!
//! Two kinds of test here and the distinction matters:
//!
//!   - PINNED: the exact base string and the exact digest, taken from the
//!     cross-SDK corpus (`tests/fixtures/signature_vectors.json`) and
//!     recomputed by hand. These are what make this a port rather than an
//!     independent invention. A mint tested only by feeding its output to
//!     this crate's own verifier agrees with a wrong construction perfectly
//!     happily, which is the failure mode this repository has shipped once.
//!   - ROUND-TRIP: mint, then verify, to prove the two halves meet. Useful,
//!     but worthless without the pins above.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chatly::{
    chatly_v2_base_string, sign_chatly_v2, verify_chatly_signature, ChatlySignatureBase,
    ChatlySignatureFailure, ChatlySignatureInput, ChatlySignatureResult, ChatlySignatureSurface,
    ChatlyV2Mint, ChatlyV2MintBase, ChatlyV2MintError, CHATLY_V2_SIGNATURE_HEADER,
};

const SECRET: &str = "whsec_vector_secret";
const BODY: &str = r#"{"id":"evt_1","type":"message.created"}"#;
const SECS: i64 = 1_760_000_000;
const PATH: &str = "/hooks/chatly?tenant=acme";
const NOW_MS: i64 = 1_760_000_000_000;

/// The corpus's digest for the unified base over `BODY` at `SECS`, POST
/// `PATH` — identical to the `v1` in the "unified_v2 verifies
/// t.METHOD.path.body" vector.
const TARGET_DIGEST: &str = "666eb5b283d58737b989ca99a0652aba73754024def8bc1c857d5d179d4b640d";
/// The corpus's digest for the body base over `BODY` at `SECS`, shared by the
/// `agent_delivery`, `event_webhook` and `workflow_webhook` vectors — one
/// construction, three headers.
const BODY_DIGEST: &str = "68c8081fadfddd5bded0385d719bd284e690ec14847d59a15a30816bf65c4f1e";
/// The corpus's digest for the `ai_tool_call` GET vector: an EMPTY body, and
/// a method the corpus supplies lower-case.
const GET_DIGEST: &str = "c44ee8addde7bed00977a7250a24913142f53f0ecb4fcab6cdf2ce485dc51b66";

fn target<'a>() -> ChatlyV2Mint<'a> {
    ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, BODY, SECS).target("POST", PATH)
}

fn body_base<'a>() -> ChatlyV2Mint<'a> {
    ChatlyV2Mint::new(ChatlyV2MintBase::Body, SECRET, BODY, SECS)
}

fn base_text(mint: &ChatlyV2Mint<'_>) -> String {
    String::from_utf8(chatly_v2_base_string(mint).expect("base string")).expect("utf-8")
}

fn reason(result: &ChatlySignatureResult) -> Option<ChatlySignatureFailure> {
    result.reason()
}

/// The base string, pinned as plaintext. A wrong base string and a wrong HMAC
/// are the same symptom at a verifier and different bugs; this is the test
/// that tells them apart, and the one to read when a language disagrees.
#[test]
fn the_base_string_is_pinned() {
    assert_eq!(
        base_text(&target()),
        format!("1760000000.POST./hooks/chatly?tenant=acme.{BODY}"),
    );
    // The body base ignores a method and path even when they are supplied.
    assert_eq!(
        base_text(&body_base().target("POST", PATH)),
        format!("1760000000.{BODY}"),
    );
    // The method is upper-cased, and an empty body contributes nothing after
    // the final separator — so a GET signs a string ENDING in ".", not one
    // that silently loses it.
    let get = ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, "", SECS)
        .target("get", "/orders/4471?expand=lines");
    assert_eq!(base_text(&get), "1760000000.GET./orders/4471?expand=lines.");
    // An empty body on the body base is the empty body, not a missing one.
    let empty = ChatlyV2Mint::new(ChatlyV2MintBase::Body, SECRET, "", SECS);
    assert_eq!(base_text(&empty), "1760000000.");
}

/// A non-UTF-8 body signs the bytes as given. `String::from_utf8_lossy` here
/// would sign U+FFFD and the digest would match nothing on the wire — the
/// reason `chatly_v2_base_string` returns `Vec<u8>` rather than `String`.
#[test]
fn a_non_utf8_body_signs_the_bytes_as_given() {
    let raw: &[u8] = &[0x7b, 0xff, 0xfe, 0x7d];
    let mint = ChatlyV2Mint::new(ChatlyV2MintBase::Body, SECRET, raw, SECS);
    let base = chatly_v2_base_string(&mint).expect("base string");
    assert_eq!(&base[..11], b"1760000000.");
    assert_eq!(&base[11..], raw, "the body bytes were altered");
    assert!(
        String::from_utf8(base).is_err(),
        "the fixture must be invalid UTF-8 for this to test anything"
    );
}

/// The digests, pinned against the corpus every other SDK runs. These three
/// values are the whole claim that this port agrees byte for byte.
#[test]
fn the_digests_match_the_corpus() {
    assert_eq!(
        sign_chatly_v2(&target()).expect("mint"),
        format!("t=1760000000,v1={TARGET_DIGEST},b=t"),
    );
    assert_eq!(
        sign_chatly_v2(&body_base()).expect("mint"),
        format!("t=1760000000,v1={BODY_DIGEST},b=b"),
    );
    let get = ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, "", SECS)
        .target("get", "/orders/4471?expand=lines");
    assert_eq!(
        sign_chatly_v2(&get).expect("mint"),
        format!("t=1760000000,v1={GET_DIGEST},b=t"),
    );
}

/// Mint, then verify: the two halves meet, and a preferred v2 header reports
/// what actually verified rather than the surface asked for.
#[test]
fn a_minted_value_round_trips_through_the_verifier() {
    let value = sign_chatly_v2(&target()).expect("mint");
    for (surface, want_base) in [
        (
            ChatlySignatureSurface::UnifiedV2,
            ChatlySignatureBase::Target,
        ),
        (
            ChatlySignatureSurface::WorkflowWebhook,
            ChatlySignatureBase::Target,
        ),
    ] {
        let input = ChatlySignatureInput::new(surface, BODY)
            .header(CHATLY_V2_SIGNATURE_HEADER, &value)
            .secret(SECRET)
            .target("POST", PATH)
            .now_ms(NOW_MS);
        match verify_chatly_signature(&input) {
            ChatlySignatureResult::Verified(v) => assert_eq!(v.base, want_base),
            ChatlySignatureResult::Refused(r) => {
                panic!(
                    "{} refused a minted target value: {} — {}",
                    surface.as_str(),
                    r.reason.as_str(),
                    r.message
                )
            }
        }
    }

    // The workflow action's own base, on the same header.
    let body_value = sign_chatly_v2(&body_base()).expect("mint");
    let input = ChatlySignatureInput::new(ChatlySignatureSurface::WorkflowWebhook, BODY)
        .header(CHATLY_V2_SIGNATURE_HEADER, &body_value)
        .secret(SECRET)
        .now_ms(NOW_MS);
    match verify_chatly_signature(&input) {
        ChatlySignatureResult::Verified(v) => assert_eq!(v.base, ChatlySignatureBase::Body),
        ChatlySignatureResult::Refused(r) => {
            panic!("refused: {} — {}", r.reason.as_str(), r.message)
        }
    }

    // A LEGACY surface: the v2 value arrives beside x-chatly-signature and
    // `Prefer` picks it up, reporting unified_v2.
    let input = ChatlySignatureInput::new(ChatlySignatureSurface::AgentDelivery, BODY)
        .header(CHATLY_V2_SIGNATURE_HEADER, &value)
        .secret(SECRET)
        .target("POST", PATH)
        .now_ms(NOW_MS);
    match verify_chatly_signature(&input) {
        ChatlySignatureResult::Verified(v) => assert_eq!(
            v.surface,
            ChatlySignatureSurface::UnifiedV2,
            "a preferred v2 must report what actually verified"
        ),
        ChatlySignatureResult::Refused(r) => {
            panic!("refused: {} — {}", r.reason.as_str(), r.message)
        }
    }
}

/// The mid-rotation dual-digest value: one `t`, two `v1` fields, no marker.
/// The sender signs with the old and the new secret; a receiver holding
/// either must accept it. Built from two REAL mints rather than from a
/// literal, so it cannot drift from the construction.
#[test]
fn a_dual_digest_value_verifies_from_either_secret() {
    const OLDER: &str = "whsec_old";
    const NEWER: &str = "whsec_new";
    let digest = |secret: &str| -> String {
        let mint = ChatlyV2Mint::new(ChatlyV2MintBase::Body, secret, BODY, SECS);
        let value = sign_chatly_v2(&mint).expect("mint");
        value
            .split("v1=")
            .nth(1)
            .expect("a v1 field")
            .trim_end_matches(",b=b")
            .to_owned()
    };
    // No `b=` marker, which is what the workflow sender emits today and
    // which a verifier must read as the body base rather than as "unknown".
    let dual = format!("t=1760000000,v1={},v1={}", digest(OLDER), digest(NEWER));

    for secrets in [
        vec![OLDER],
        vec![NEWER],
        vec![OLDER, NEWER],
        vec![NEWER, OLDER],
    ] {
        let mut input = ChatlySignatureInput::new(ChatlySignatureSurface::WorkflowWebhook, BODY)
            .header(CHATLY_V2_SIGNATURE_HEADER, &dual)
            .now_ms(NOW_MS);
        input.secrets = secrets.clone();
        match verify_chatly_signature(&input) {
            ChatlySignatureResult::Verified(v) => assert_eq!(
                v.base,
                ChatlySignatureBase::Body,
                "an absent marker means the body base"
            ),
            ChatlySignatureResult::Refused(r) => panic!(
                "{secrets:?} refused the dual-digest value: {} — {}",
                r.reason.as_str(),
                r.message
            ),
        }
    }

    // The control: two digests from secrets the receiver does NOT hold are
    // still a mismatch, so the loop above is not passing because any
    // two-digest value is waved through.
    let input = ChatlySignatureInput::new(ChatlySignatureSurface::WorkflowWebhook, BODY)
        .header(CHATLY_V2_SIGNATURE_HEADER, &dual)
        .secret("whsec_neither")
        .now_ms(NOW_MS);
    assert_eq!(
        reason(&verify_chatly_signature(&input)),
        Some(ChatlySignatureFailure::SignatureMismatch),
    );
}

// ── The three hard requirements, stated as tests ────────────────────────────

/// A one-byte body change must fail. The corpus's "re-serialised body" vector
/// changes whitespace in several places at once; this changes exactly one
/// byte in the middle of a value, which is the tamper a length check or a
/// prefix comparison would miss.
#[test]
fn a_one_byte_body_change_fails() {
    let value = sign_chatly_v2(&target()).expect("mint");
    let tampered = BODY.replacen("\"evt_1\"", "\"evt_2\"", 1);
    assert_eq!(
        tampered.len(),
        BODY.len(),
        "the tamper must be one byte so a length check cannot catch it"
    );
    let verify = |body: &str| {
        let input = ChatlySignatureInput::new(ChatlySignatureSurface::UnifiedV2, body)
            .header(CHATLY_V2_SIGNATURE_HEADER, &value)
            .secret(SECRET)
            .target("POST", PATH)
            .now_ms(NOW_MS);
        verify_chatly_signature(&input)
    };
    // The control, in the same test: the untampered body DOES verify, so the
    // refusal below is the one changed byte's doing, not a broken setup.
    assert!(verify(BODY).is_ok(), "the untampered body was refused");
    assert_eq!(
        reason(&verify(&tampered)),
        Some(ChatlySignatureFailure::SignatureMismatch),
    );
}

/// A `t` 301 seconds old must give `timestamp_outside_tolerance` — and 300
/// exactly must not, because the boundary is where an off-by-one lives and
/// "stale" is otherwise indistinguishable from a clock drift of one second.
#[test]
fn the_tolerance_boundary_is_300_seconds() {
    let value = sign_chatly_v2(&target()).expect("mint");
    let at = |offset_ms: i64| {
        let input = ChatlySignatureInput::new(ChatlySignatureSurface::UnifiedV2, BODY)
            .header(CHATLY_V2_SIGNATURE_HEADER, &value)
            .secret(SECRET)
            .target("POST", PATH)
            .now_ms(NOW_MS + offset_ms);
        verify_chatly_signature(&input)
    };
    for offset in [299_000, 300_000, -300_000] {
        assert!(
            at(offset).is_ok(),
            "{offset}ms is inside the window and was refused"
        );
    }
    // Both directions: a far-FUTURE t is a sender clock or the wrong unit,
    // and reads as "expired" unless it is refused for its own reason.
    for offset in [301_000, -301_000] {
        assert_eq!(
            reason(&at(offset)),
            Some(ChatlySignatureFailure::TimestampOutsideTolerance),
            "{offset}ms should be outside the window",
        );
    }
}

/// Two `x-chatly-signature-v2` headers must give `duplicate_signature_header`.
///
/// The corpus pins this on `x-livechat-signature` only, where the check is
/// reached directly. On the v2 header it is reached through the v2 policy —
/// the policy sees one header PRESENT and delegates to `unified_v2`, which is
/// where the count happens — so this is a genuinely different path, and on a
/// legacy surface the refusal is reported against `unified_v2` rather than
/// against the surface asked for.
#[test]
fn two_v2_headers_are_refused() {
    let value = sign_chatly_v2(&target()).expect("mint");
    // Every case sends `value` TWICE; only the second header's spelling and
    // the surface differ. Carrying the pairs in the table instead would need
    // a `Vec<(&str, &str)>` inside the tuple, which clippy's type_complexity
    // rejects and which hides the one thing that varies.
    let cases = [
        (
            ChatlySignatureSurface::UnifiedV2,
            CHATLY_V2_SIGNATURE_HEADER,
            "unified_v2",
        ),
        (
            // Case-insensitively the same header, so still twice.
            ChatlySignatureSurface::UnifiedV2,
            "X-Chatly-Signature-V2",
            "unified_v2",
        ),
        (
            // Through the v2 policy; reported against what was verified,
            // which is unified_v2 and not the surface asked for.
            ChatlySignatureSurface::AgentDelivery,
            CHATLY_V2_SIGNATURE_HEADER,
            "unified_v2",
        ),
        (
            // This surface's OWN header is the v2 header, so the count is
            // reached directly and the refusal names the surface itself.
            ChatlySignatureSurface::WorkflowWebhook,
            CHATLY_V2_SIGNATURE_HEADER,
            "workflow_webhook",
        ),
    ];
    for (surface, second, want_surface) in cases {
        let input = ChatlySignatureInput::new(surface, BODY)
            .header(CHATLY_V2_SIGNATURE_HEADER, &value)
            .header(second, &value)
            .secret(SECRET)
            .target("POST", PATH)
            .now_ms(NOW_MS);
        let result = verify_chatly_signature(&input);
        assert_eq!(
            reason(&result),
            Some(ChatlySignatureFailure::DuplicateSignatureHeader),
            "{} with two v2 headers",
            surface.as_str(),
        );
        assert_eq!(result.surface(), want_surface);
    }

    // The control: ONE of those headers verifies. Without this the loop
    // would pass against a verifier that refuses the v2 header outright,
    // which is a different bug with the same green tick.
    let input = ChatlySignatureInput::new(ChatlySignatureSurface::AgentDelivery, BODY)
        .header(CHATLY_V2_SIGNATURE_HEADER, &value)
        .secret(SECRET)
        .target("POST", PATH)
        .now_ms(NOW_MS);
    assert!(verify_chatly_signature(&input).is_ok());
}

// ── Minting refusals ───────────────────────────────────────────────────────

/// Every way of asking for a signature that would cover the wrong bytes is an
/// error, not a quietly-wrong digest. Each case asserts BOTH functions,
/// because `chatly_v2_base_string` is the one a cross-language comparison
/// calls and it must not be the lenient door.
#[test]
fn an_input_that_would_sign_the_wrong_string_is_refused() {
    let no_method = {
        let mut m = ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, BODY, SECS);
        m.path = Some(PATH);
        m
    };
    let no_path = {
        let mut m = ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, BODY, SECS);
        m.method = Some("POST");
        m
    };
    // Empty is absent, the same test the verifier's `has_target` applies.
    let empty_path =
        ChatlyV2Mint::new(ChatlyV2MintBase::Target, SECRET, BODY, SECS).target("POST", "");
    let negative = ChatlyV2Mint::new(ChatlyV2MintBase::Body, SECRET, BODY, -1);

    for (name, mint, want) in [
        (
            "no method",
            &no_method,
            ChatlyV2MintError::MissingTarget {
                method: None,
                path: Some(PATH.to_owned()),
            },
        ),
        (
            "no path",
            &no_path,
            ChatlyV2MintError::MissingTarget {
                method: Some("POST".to_owned()),
                path: None,
            },
        ),
        (
            "an empty path",
            &empty_path,
            ChatlyV2MintError::MissingTarget {
                method: Some("POST".to_owned()),
                path: Some(String::new()),
            },
        ),
        (
            "a negative timestamp",
            &negative,
            ChatlyV2MintError::NegativeTimestamp(-1),
        ),
    ] {
        assert_eq!(
            sign_chatly_v2(mint).err(),
            Some(want.clone()),
            "{name}: sign_chatly_v2"
        );
        assert_eq!(
            chatly_v2_base_string(mint).err(),
            Some(want.clone()),
            "{name}: chatly_v2_base_string must refuse too, not be the lenient door",
        );
        // The message is what an integrator reads; an error whose Display is
        // empty or generic sends them hunting their secret instead.
        let text = want.to_string();
        assert!(
            text.len() > 40,
            "{name}: Display is too terse to help: {text:?}"
        );
    }

    // An EMPTY secret deliberately mints: HMAC takes a zero-length key, the
    // reference mints with one, and a test proving a verifier answers
    // missing_secret has to be able to produce the input.
    let blank = ChatlyV2Mint::new(ChatlyV2MintBase::Body, "", BODY, SECS);
    assert!(
        sign_chatly_v2(&blank).is_ok(),
        "an empty secret should mint, not refuse"
    );
    // And a zero timestamp is a real instant, not a sentinel.
    let epoch = ChatlyV2Mint::new(ChatlyV2MintBase::Body, SECRET, BODY, 0);
    assert_eq!(
        sign_chatly_v2(&epoch).expect("mint").split(',').next(),
        Some("t=0"),
    );
}
