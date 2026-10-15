//! `verify_chatly_signature` against the cross-SDK signature corpus.
//!
//! `tests/fixtures/signature_vectors.json` is a BYTE-IDENTICAL copy of the
//! corpus sdk-node generates; sdk-node's own spec fails if this copy drifts,
//! so it must never be edited here. Every vector's every expected field is
//! asserted — `ok`, `surface`, and `reason`, or `base`, `secretIndex` and
//! `timestampMs` — plus a non-empty `message` on every refusal.
//!
//! The path comes from `CARGO_MANIFEST_DIR`, never `../sdk-node` or
//! `../sdk-spec`: in the public mirror this crate's directory IS the repo
//! root and has no siblings. Three SDK mirrors once failed CI on exactly that.
//!
//! Every vector is run and the failures are reported TOGETHER, so a broken
//! port shows its whole shape at once rather than one vector per run.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chatly::{
    verify_chatly_signature, ChatlySignatureFailure, ChatlySignatureInput, ChatlySignatureResult,
    ChatlySignatureSurface, ChatlyV2Policy,
};
use serde_json::Value;

/// A floor, so a loader that silently reads nothing cannot pass on zero.
/// It is the corpus's CURRENT size, 61. It read 59 — the size when the Go
/// port was written — while the corpus held 61, so a loader that dropped two
/// vectors passed. Raise it when the corpus grows.
const VECTOR_FLOOR: usize = 61;

fn corpus() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/signature_vectors.json"
    );
    let text = std::fs::read_to_string(path).expect("read the signature corpus");
    let doc: Value = serde_json::from_str(&text).expect("the corpus is JSON");
    assert_eq!(doc["version"], 1, "corpus version");
    doc["vectors"].as_array().expect("vectors array").clone()
}

/// A string, or every string in an array — the two shapes the corpus uses
/// for header values and for `secret`.
fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().expect("array of strings"))
            .collect(),
        other => panic!("expected a string or an array of strings, got {other}"),
    }
}

fn policy(name: &str) -> ChatlyV2Policy {
    match name {
        "prefer" => ChatlyV2Policy::Prefer,
        "require" => ChatlyV2Policy::Require,
        "ignore" => ChatlyV2Policy::Ignore,
        other => panic!("unknown v2 policy {other:?} in the corpus"),
    }
}

/// Run one vector; `Err` describes every field that disagreed.
fn run(vector: &Value) -> Result<(), String> {
    let input = &vector["input"];
    let expect = &vector["expect"];

    let mut req = ChatlySignatureInput::named(
        input["surface"].as_str().expect("surface"),
        input["body"].as_str().expect("body is a string"),
    );
    // serde_json sorts object keys (no `preserve_order`), which is harmless:
    // lookup collects every case-insensitive match whatever the order.
    for (name, value) in input["headers"].as_object().expect("headers") {
        for v in strings(value) {
            req = req.header(name, v);
        }
    }
    for s in strings(&input["secret"]) {
        req = req.secret(s);
    }
    if let Some(m) = input["method"].as_str() {
        req.method = Some(m);
    }
    if let Some(p) = input["path"].as_str() {
        req.path = Some(p);
    }
    if let Some(v2) = input["v2"].as_str() {
        req = req.v2(policy(v2));
    }
    if let Some(allow) = input["allowUnprotectedReplay"].as_bool() {
        req = req.allow_unprotected_replay(allow);
    }
    if let Some(t) = input["toleranceSeconds"].as_u64() {
        req = req.tolerance_seconds(t);
    }
    req = req.now_ms(input["nowMs"].as_i64().expect("nowMs"));

    let got = verify_chatly_signature(&req);
    let mut wrong = Vec::new();
    let want_ok = expect["ok"].as_bool().expect("expect.ok");
    if got.is_ok() != want_ok {
        wrong.push(format!("ok: want {want_ok}, got {}", got.is_ok()));
    }
    let want_surface = expect["surface"].as_str().expect("expect.surface");
    if got.surface() != want_surface {
        wrong.push(format!(
            "surface: want {want_surface}, got {}",
            got.surface()
        ));
    }
    match &got {
        ChatlySignatureResult::Verified(v) => {
            if want_ok {
                let base = expect["base"].as_str().expect("expect.base");
                if v.base.as_str() != base {
                    wrong.push(format!("base: want {base}, got {}", v.base));
                }
                let index = expect["secretIndex"].as_u64().expect("expect.secretIndex");
                if v.secret_index as u64 != index {
                    wrong.push(format!("secretIndex: want {index}, got {}", v.secret_index));
                }
                let ts = expect["timestampMs"].as_u64();
                if !expect["timestampMs"].is_null() && ts.is_none() {
                    wrong.push("expect.timestampMs is neither null nor a u64".to_owned());
                }
                if v.timestamp_ms != ts {
                    wrong.push(format!(
                        "timestampMs: want {ts:?}, got {:?}",
                        v.timestamp_ms
                    ));
                }
            }
        }
        ChatlySignatureResult::Refused(r) => {
            if r.message.trim().is_empty() {
                wrong.push("refusal message is empty".to_owned());
            }
            if let Some(reason) = expect["reason"].as_str() {
                if r.reason.as_str() != reason {
                    wrong.push(format!("reason: want {reason}, got {}", r.reason));
                }
            } else if !want_ok {
                wrong.push("refusal vector has no expect.reason".to_owned());
            }
            if want_ok {
                wrong.push(format!("refused with {}: {}", r.reason, r.message));
            }
        }
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(wrong.join("; "))
    }
}

#[test]
fn every_corpus_vector_agrees() {
    let vectors = corpus();
    assert!(
        vectors.len() >= VECTOR_FLOOR,
        "the corpus loaded {} vectors; the floor is {VECTOR_FLOOR}",
        vectors.len()
    );
    let failures: Vec<String> = vectors
        .iter()
        .filter_map(|v| {
            let name = v["name"].as_str().unwrap_or("<unnamed>");
            run(v).err().map(|e| format!("  {name}: {e}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} vectors disagree:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
}

#[test]
fn the_corpus_covers_every_surface_and_every_reason() {
    let vectors = corpus();
    for surface in ChatlySignatureSurface::ALL {
        assert!(
            vectors
                .iter()
                .any(|v| v["input"]["surface"] == surface.as_str()),
            "no vector for {surface}"
        );
    }
    for reason in ChatlySignatureFailure::ALL {
        assert!(
            vectors
                .iter()
                .any(|v| v["expect"]["reason"] == reason.as_str()),
            "no vector for {reason}"
        );
    }
}

#[test]
fn surface_names_round_trip_and_reasons_are_distinct() {
    for surface in ChatlySignatureSurface::ALL {
        assert_eq!(
            surface.as_str().parse::<ChatlySignatureSurface>(),
            Ok(surface)
        );
        assert_eq!(surface.to_string(), surface.as_str());
    }
    assert!("Event_Webhook".parse::<ChatlySignatureSurface>().is_err());
    let mut names: Vec<&str> = ChatlySignatureFailure::ALL
        .iter()
        .map(|r| r.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 11);
}

const ZEROS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn reason_of(result: &ChatlySignatureResult) -> Option<&'static str> {
    result.reason().map(|r| r.as_str())
}

#[test]
fn a_timestamp_past_64_bits_is_malformed_not_a_panic() {
    for (surface, header, t) in [
        // Fits u64 as seconds, overflows on the * 1000.
        (
            ChatlySignatureSurface::EventWebhook,
            "x-livechat-signature",
            "18446744073709551615",
        ),
        // Does not fit u64 at all.
        (
            ChatlySignatureSurface::EventWebhook,
            "x-livechat-signature",
            "99999999999999999999999999999",
        ),
    ] {
        let value = format!("t={t},v1={ZEROS}");
        let input = ChatlySignatureInput::new(surface, "{}")
            .header(header, &value)
            .secret("s")
            .now_ms(0);
        assert_eq!(
            reason_of(&verify_chatly_signature(&input)),
            Some("malformed_timestamp"),
            "{t}"
        );
    }
    // A milliseconds surface reads its header value as ms: 2^64 overflows.
    let sig = format!("sha256={ZEROS}");
    let input = ChatlySignatureInput::new(ChatlySignatureSurface::CustomerCard, "{}")
        .header("x-chatly-signature", &sig)
        .header("x-chatly-timestamp", "18446744073709551616")
        .secret("s")
        .now_ms(0);
    assert_eq!(
        reason_of(&verify_chatly_signature(&input)),
        Some("malformed_timestamp")
    );
}

#[test]
fn extreme_clocks_and_tolerances_do_not_overflow() {
    let value = format!("t=18446744073709551,v1={ZEROS}");
    for now in [i64::MIN, -1, 0, i64::MAX] {
        for tolerance in [0, 300, u64::MAX] {
            let input = ChatlySignatureInput::new(ChatlySignatureSurface::EventWebhook, "")
                .header("x-livechat-signature", &value)
                .secret("s")
                .tolerance_seconds(tolerance)
                .now_ms(now);
            let result = verify_chatly_signature(&input);
            assert!(!result.is_ok());
            if tolerance == u64::MAX {
                // Inside the window, so it got as far as the compare.
                assert_eq!(reason_of(&result), Some("signature_mismatch"));
            }
        }
    }
}

/// Values chosen to break naive parsing: empty pieces, bare `=`, multi-byte
/// characters at the 48-char preview cut, a `+` the integer parser would
/// accept, non-hex the size of a digest, and NUL.
fn garbage() -> Vec<String> {
    let mut out: Vec<String> = [
        "",
        " ",
        "=",
        ",,,",
        "t=",
        "v1=",
        "t=,v1=",
        "=t=1,=v1=2",
        "sha256=",
        "sha256",
        "t=1,v1=zz",
        "t=+1,v1=0",
        "t=-1,v1=0",
        "t=1e9,v1=0",
        "t=\u{0}",
        "b=t,b=b",
        "\u{FEFF}t=1\u{FEFF}",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    out.push(format!("t=+1700000000,v1={ZEROS}"));
    out.push(format!("t=１２３,v1={ZEROS}")); // full-width digits
    out.push(format!("t=1,v1={},b=t", "é".repeat(32)));
    out.push(format!("sha256={}", "é".repeat(64)));
    out.push(format!("sha256={}", "g".repeat(64)));
    out.push("é".repeat(200));
    out.push("t=".to_owned() + &"9".repeat(5000));
    out
}

#[test]
fn garbage_never_panics_on_any_surface_or_policy() {
    let surfaces: Vec<&str> = ChatlySignatureSurface::ALL
        .iter()
        .map(|s| s.as_str())
        .chain(["", "nope", "UNIFIED_V2", "é"])
        .collect();
    let header_names = [
        "x-chatly-signature",
        "x-chatly-signature-v2",
        "x-livechat-signature",
        "x-chatly-canvas-signature",
        "X-CHATLY-TIMESTAMP",
        "x-chatly-canvas-ts",
        "",
    ];
    let bodies: [&[u8]; 3] = [b"", b"{}", &[0xff, 0xfe, 0x00]];
    let mut runs = 0usize;
    for surface in &surfaces {
        for value in garbage() {
            for policy in [
                ChatlyV2Policy::Prefer,
                ChatlyV2Policy::Require,
                ChatlyV2Policy::Ignore,
            ] {
                for body in bodies {
                    let mut input = ChatlySignatureInput::named(surface, body)
                        .secret("")
                        .secret("whsec_x")
                        .v2(policy)
                        .allow_unprotected_replay(true)
                        .now_ms(1_760_000_000_000);
                    for name in header_names {
                        input = input.header(name, &value);
                    }
                    let with_target = input.clone().target("pöst", "/é?q=1");
                    for candidate in [input, with_target] {
                        let result = verify_chatly_signature(&candidate);
                        // Nothing here is a valid signature.
                        assert!(!result.is_ok(), "{surface} {value:?}");
                        if let ChatlySignatureResult::Refused(r) = &result {
                            assert!(!r.message.is_empty());
                        }
                        runs += 1;
                    }
                }
            }
        }
    }
    assert!(runs > 1000, "only {runs} garbage runs");
}

#[test]
fn each_garbage_value_alone_never_panics() {
    // One header at a time, so a value reaches the parser instead of being
    // stopped at duplicate_signature_header by its neighbours.
    for surface in ChatlySignatureSurface::ALL {
        let spec = surface.spec();
        for value in garbage() {
            let input = ChatlySignatureInput::new(surface, "{}")
                .header(spec.signature_header, &value)
                .header("x-chatly-timestamp", &value)
                .header("x-chatly-canvas-ts", &value)
                .secret("whsec_x")
                .allow_unprotected_replay(true)
                .target("POST", "/")
                .now_ms(1_760_000_000_000);
            assert!(
                !verify_chatly_signature(&input).is_ok(),
                "{surface} {value:?}"
            );
        }
    }
}

#[test]
fn the_system_clock_default_refuses_a_2025_vector() {
    // nowMs unset: the real clock is used, and a 2025 timestamp is stale.
    let vectors = corpus();
    let v = vectors
        .iter()
        .find(|v| v["name"] == "agent_delivery verifies t.body")
        .expect("vector");
    let header = v["input"]["headers"]["x-chatly-signature"]
        .as_str()
        .unwrap();
    let input = ChatlySignatureInput::new(
        ChatlySignatureSurface::AgentDelivery,
        v["input"]["body"].as_str().unwrap(),
    )
    .header("x-chatly-signature", header)
    .secret(v["input"]["secret"].as_str().unwrap());
    assert_eq!(
        reason_of(&verify_chatly_signature(&input)),
        Some("timestamp_outside_tolerance")
    );
}
