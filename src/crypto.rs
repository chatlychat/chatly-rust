//! SHA-256, HMAC-SHA256, hex, constant-time compare, entropy and UUID v4 —
//! all on `std` alone.
//!
//! This module is private. It exists so the crate does not pull in `sha2`,
//! `hmac`, `uuid` or `rand`: `packages/sdk-spec/CONTRACT.md` §9 permits Rust
//! exactly one runtime dependency family (`ureq` plus `serde`), and a
//! customer should not inherit four more crates to verify a webhook.
//!
//! Correctness is not taken on trust. The SHA-256 core is pinned by the three
//! FIPS 180-4 vectors in this file's own tests, the HMAC by RFC 4231 case 2,
//! and both public helpers that use it ([`crate::identity_hash`] and
//! [`crate::verify_webhook_signature`]) are pinned by the cross-language
//! vectors in CONTRACT.md §10.8 and §10.9.

use std::sync::atomic::{AtomicU64, Ordering};

/// SHA-256 round constants (FIPS 180-4 §4.2.2).
#[rustfmt::skip]
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 initial hash value (FIPS 180-4 §5.3.3).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// The SHA-256 block size, in bytes. Also HMAC's key block size.
pub(crate) const BLOCK_LEN: usize = 64;
/// The SHA-256 digest size, in bytes.
pub(crate) const DIGEST_LEN: usize = 32;

/// A streaming SHA-256 hasher.
///
/// Streaming rather than one-shot so HMAC can feed `key_pad` then the message
/// without allocating their concatenation, and so a multi-megabyte webhook
/// body is never copied just to prepend `"<t>."`.
pub(crate) struct Sha256 {
    state: [u32; 8],
    block: [u8; BLOCK_LEN],
    block_len: usize,
    total_bits: u64,
}

impl Sha256 {
    pub(crate) fn new() -> Self {
        Self { state: H0, block: [0u8; BLOCK_LEN], block_len: 0, total_bits: 0 }
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        // `wrapping_mul` only to keep the arithmetic total; a 2^61-byte input
        // is not reachable and the length field is defined modulo 2^64 anyway.
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64).wrapping_mul(8));
        while !data.is_empty() {
            let take = core::cmp::min(BLOCK_LEN - self.block_len, data.len());
            self.block[self.block_len..self.block_len + take].copy_from_slice(&data[..take]);
            self.block_len += take;
            data = &data[take..];
            if self.block_len == BLOCK_LEN {
                let block = self.block;
                self.compress(&block);
                self.block_len = 0;
            }
        }
    }

    pub(crate) fn finalize(mut self) -> [u8; DIGEST_LEN] {
        let bits = self.total_bits;
        // 0x80, then zeros, then the 64-bit big-endian bit length.
        self.block[self.block_len] = 0x80;
        self.block_len += 1;
        if self.block_len > BLOCK_LEN - 8 {
            for byte in self.block[self.block_len..].iter_mut() {
                *byte = 0;
            }
            let block = self.block;
            self.compress(&block);
            self.block_len = 0;
        }
        for byte in self.block[self.block_len..BLOCK_LEN - 8].iter_mut() {
            *byte = 0;
        }
        self.block[BLOCK_LEN - 8..].copy_from_slice(&bits.to_be_bytes());
        let block = self.block;
        self.compress(&block);

        let mut out = [0u8; DIGEST_LEN];
        for (chunk, word) in out.chunks_mut(4).zip(self.state.iter()) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; BLOCK_LEN]) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let b = i * 4;
            *word = u32::from_be_bytes([block[b], block[b + 1], block[b + 2], block[b + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in self
            .state
            .iter_mut()
            .zip([a, b, c, d, e, f, g, h])
        {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// One-shot SHA-256.
pub(crate) fn sha256(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

/// HMAC-SHA256 of `parts` concatenated, under `key` (RFC 2104).
///
/// `parts` is a slice of slices so the caller can sign `"<t>."` followed by a
/// borrowed body without building the joined buffer — see CONTRACT.md §6,
/// which requires the digest be taken over the bytes as received.
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; DIGEST_LEN] {
    let mut block_key = [0u8; BLOCK_LEN];
    if key.len() > BLOCK_LEN {
        block_key[..DIGEST_LEN].copy_from_slice(&sha256(key));
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36u8; BLOCK_LEN];
    let mut outer_pad = [0x5cu8; BLOCK_LEN];
    for i in 0..BLOCK_LEN {
        inner_pad[i] ^= block_key[i];
        outer_pad[i] ^= block_key[i];
    }

    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    for part in parts {
        inner.update(part);
    }
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&outer_pad);
    outer.update(&inner_digest);
    outer.finalize()
}

/// Lowercase hex.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Decode hex, case-insensitively. `None` for an odd length or a non-hex byte.
///
/// Returns `None` rather than panicking: it is reached from
/// [`crate::verify_webhook_signature`], which must never unwind on attacker-
/// controlled input (CONTRACT.md §6 — "a verifier that throws is an accepted
/// webhook in a handler that forgot to catch").
pub(crate) fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = nibble(pair[0])?;
        let lo = nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Compare two byte strings without an early return on the first difference.
///
/// Length is compared first and returns immediately — the lengths of a digest
/// and of a submitted tag are not secret, and a fixed-length loop over
/// mismatched slices would need padding that leaks nothing but costs clarity.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    // `read_volatile` is not available on a safe path; the fold above is not
    // short-circuiting, which is the property that matters here.
    diff == 0
}

/// Monotonic counter folded into the fallback seed so two keys minted in the
/// same nanosecond by the same process still differ.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Fill `out` with random bytes.
///
/// Prefers the operating system's CSPRNG via `/dev/urandom`. When that is not
/// readable (Windows, or a sandbox with no `/dev`) it falls back to SplitMix64
/// over the wall clock, the process id, a thread-local address and a global
/// counter.
///
/// The fallback is **not** cryptographic, and nothing in this crate treats it
/// as such: its only consumers are idempotency keys (which need uniqueness,
/// not unpredictability) and retry jitter. No signing key, nonce or secret is
/// generated here.
pub(crate) fn random_bytes(out: &mut [u8]) {
    use std::io::Read;

    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        if file.read_exact(out).is_ok() {
            return;
        }
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let stack_probe = &nanos as *const u64 as u64;
    let mut seed = nanos
        ^ (u64::from(std::process::id()) << 32)
        ^ stack_probe
        ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9e37_79b9_7f4a_7c15);

    for chunk in out.chunks_mut(8) {
        // SplitMix64.
        seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let bytes = z.to_le_bytes();
        let take = chunk.len();
        chunk.copy_from_slice(&bytes[..take]);
    }
}

/// A random v4 UUID in the canonical lowercase hyphenated form.
///
/// Used for `Idempotency-Key`. Version and variant bits are set so the value
/// is a well-formed UUID and not merely 32 hex characters.
pub(crate) fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex_encode(&bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A uniform `u64`, for retry jitter.
pub(crate) fn random_u64() -> u64 {
    let mut bytes = [0u8; 8];
    random_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// FIPS 180-4 and the two standard empty/long vectors.
    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            hex_encode(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_encode(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_encode(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// Crosses the 56-byte padding boundary and the 64-byte block boundary,
    /// which is where a hand-written padding routine goes wrong.
    #[test]
    fn sha256_handles_padding_boundaries() {
        // Lengths 55/56/63/64/65 straddle both the "does the length field
        // still fit in this block" boundary (56) and the block boundary (64).
        // Independently generated with `openssl dgst -sha256`.
        for (len, expected) in [
            (55usize, "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"),
            (56, "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"),
            (63, "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"),
            (64, "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"),
            (65, "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0"),
        ] {
            assert_eq!(hex_encode(&sha256(&vec![b'a'; len])), expected, "len {len}");
        }
        // 1,000,000 'a' — FIPS 180-4's long message vector.
        let mut hasher = Sha256::new();
        for _ in 0..1000 {
            hasher.update(&[b'a'; 1000]);
        }
        assert_eq!(
            hex_encode(&hasher.finalize()),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// RFC 4231 test case 2.
    #[test]
    fn hmac_matches_rfc4231_case_2() {
        assert_eq!(
            hex_encode(&hmac_sha256(b"Jefe", &[b"what do ya want for nothing?"])),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// RFC 4231 test case 3 uses a 131-byte key, exercising the key > block
    /// branch where the key is itself hashed first.
    #[test]
    fn hmac_hashes_an_oversized_key() {
        let key = [0xaau8; 131];
        assert_eq!(
            hex_encode(&hmac_sha256(&key, &[b"Test Using Larger Than Block-Size Key - Hash Key First"])),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    /// Multi-part input must digest identically to the concatenation. This is
    /// the property the webhook verifier relies on to avoid copying the body.
    #[test]
    fn hmac_parts_equal_concatenation() {
        let split = hmac_sha256(b"k", &[b"1700000000.", br#"{"id":"evt_1"}"#]);
        let joined = hmac_sha256(b"k", &[br#"1700000000.{"id":"evt_1"}"#]);
        assert_eq!(split, joined);
    }

    #[test]
    fn hex_round_trips_and_rejects_malformed() {
        assert_eq!(hex_decode("00ff10").unwrap(), vec![0x00, 0xff, 0x10]);
        assert_eq!(hex_decode("00FF10").unwrap(), vec![0x00, 0xff, 0x10]);
        assert!(hex_decode("abc").is_none(), "odd length");
        assert!(hex_decode("zz").is_none(), "non-hex");
        assert!(hex_decode("").is_none(), "empty");
    }

    #[test]
    fn constant_time_eq_is_length_safe() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn uuid_v4_is_well_formed_and_unique() {
        let a = uuid_v4();
        let b = uuid_v4();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let fields: Vec<&str> = a.split('-').collect();
        assert_eq!(
            fields.iter().map(|f| f.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(&a[14..15], "4", "version nibble");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"), "variant nibble");
    }

    #[test]
    fn random_bytes_fills_odd_lengths() {
        // The chunked SplitMix64 loop has to cope with a tail shorter than 8.
        let mut buf = [0u8; 13];
        random_bytes(&mut buf);
        let mut other = [0u8; 13];
        random_bytes(&mut other);
        assert_ne!(buf, other);
    }
}
