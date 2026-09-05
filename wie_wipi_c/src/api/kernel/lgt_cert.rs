//! Recovery of the LGT ez-i subscriber phone number a title's `cert.c2s` was
//! issued for.
//!
//! LGT ez-i titles authenticate at start-up by reading the subscriber phone
//! number via `MC_knlGetSystemProperty("PHONENUMBER")` and using it as the key
//! that decrypts `cert.c2s`. The certificate plaintext is
//! `<8-byte app id><phone number><checksums>`, so the phone number is both the
//! key and part of the plaintext. The title decrypts with whatever number the
//! platform reports and rejects the result (error 3100) unless the recovered
//! bytes are self-consistent — so the number the emulator returns must match
//! the one the certificate was issued for.
//!
//! Because the plaintext repeats the key (the decrypted tail equals the key)
//! and the leading app id is a fixed 8-byte field, the number can be recovered
//! from `cert.c2s` alone, without knowing it in advance: this lets the emulator
//! serve the correct number per title instead of a hardcoded guess. The
//! recovery is deliberately conservative — it returns a number only when
//! exactly one candidate satisfies every check the title itself would apply,
//! and otherwise reports `None` so the caller can fall back.

use alloc::{string::String, vec, vec::Vec};

/// S-box of the LGT ez-i `cert.c2s` stream cipher (a fixed SDK constant).
#[rustfmt::skip]
const SBOX: [u8; 256] = [
    0x8c, 0x0d, 0xae, 0x4f, 0xe8, 0x81, 0xd2, 0x53, 0x10, 0x35, 0xd6, 0x77, 0x64, 0xa5, 0x96, 0x2b,
    0x34, 0x1d, 0x9e, 0x5f, 0x60, 0x81, 0xbe, 0x73, 0xe4, 0x35, 0xa2, 0x87, 0x28, 0x69, 0x2a, 0x9b,
    0x78, 0xfd, 0x4e, 0xbf, 0x30, 0x51, 0x8e, 0xe3, 0x74, 0xf5, 0x32, 0xa7, 0xf8, 0xc9, 0xb6, 0x9b,
    0x3c, 0x7d, 0x3e, 0x5f, 0x30, 0x01, 0xde, 0x23, 0x04, 0x85, 0x56, 0x57, 0x48, 0x69, 0xaa, 0x4b,
    0x5c, 0xcd, 0xaa, 0x6d, 0xb2, 0x2b, 0x08, 0xe5, 0x82, 0x57, 0xe0, 0xef, 0xee, 0x1b, 0x34, 0x9d,
    0x62, 0x7b, 0x2e, 0x85, 0x5e, 0xbf, 0x34, 0x0d, 0x7a, 0x61, 0x94, 0xb9, 0xa2, 0x9f, 0x7a, 0xcf,
    0x3a, 0x4f, 0x88, 0xa9, 0xe6, 0x8f, 0xb2, 0x79, 0xb6, 0x2f, 0x78, 0xfd, 0x76, 0xbb, 0x48, 0xcb,
    0xca, 0xff, 0x18, 0xb5, 0x80, 0xe7, 0x50, 0x05, 0x6e, 0x5b, 0x68, 0x5d, 0x06, 0xa9, 0x30, 0xc9,
    0xce, 0x93, 0x20, 0x99, 0xd8, 0x6f, 0xa0, 0x91, 0xe6, 0x3b, 0x94, 0xe3, 0x46, 0xf7, 0xdc, 0x2d,
    0x94, 0xfb, 0xac, 0xa1, 0xe6, 0x43, 0x08, 0x23, 0x68, 0xb3, 0x88, 0xc1, 0xde, 0x2b, 0xd0, 0xeb,
    0xd2, 0xab, 0x0c, 0x7d, 0x32, 0x03, 0x3c, 0x8d, 0xfe, 0xeb, 0xf6, 0x65, 0x16, 0xd7, 0x88, 0xed,
    0xea, 0xbd, 0x2c, 0xe1, 0xda, 0xb7, 0xd0, 0xf1, 0x16, 0xab, 0xe4, 0x15, 0x8a, 0x57, 0x16, 0x17,
    0x4a, 0x6b, 0x6c, 0x9d, 0x7e, 0x1f, 0xae, 0xa1, 0x72, 0x37, 0x20, 0xef, 0xee, 0x9b, 0xf4, 0x11,
    0xec, 0x03, 0xec, 0x81, 0x66, 0x03, 0x30, 0x15, 0x6a, 0x65, 0xea, 0x35, 0xc6, 0x07, 0x38, 0x8d,
    0x3e, 0x75, 0x26, 0x6d, 0x9e, 0x53, 0x44, 0x79, 0x12, 0xf1, 0xa4, 0x45, 0xb6, 0x47, 0xf8, 0x29,
    0xae, 0xab, 0x7c, 0x2d, 0x72, 0x2f, 0x12, 0x51, 0x86, 0x9d, 0x3a, 0x03, 0x64, 0xe9, 0xe2, 0xdb,
];

/// Leading app-id field length in the decrypted certificate.
const APP_ID_LEN: usize = 8;
/// Plausible subscriber-number lengths to consider.
const MIN_KEY_LEN: usize = 5;
const MAX_KEY_LEN: usize = 15;

/// Decrypt certificate byte `i` under `key` (matches the title's cipher: a
/// 32-bit `sbox + key` add, XOR with the ciphertext byte, truncated to 8 bits).
#[inline]
fn decrypt_byte(cert: &[u8], key: &[u8], off: usize, i: usize) -> u8 {
    let s = SBOX[(i + off) & 0xff] as u32;
    let k = key[(i + off) % key.len()] as u32;
    (((s + k) ^ cert[i] as u32) & 0xff) as u8
}

/// Recover the subscriber phone number `cert.c2s` was issued for, or `None`
/// when the blob is not a recognisable certificate or the number cannot be
/// pinned to a single candidate.
pub fn recover_phone_number(cert: &[u8]) -> Option<String> {
    // Layout: [0..len) ciphertext, [len] input checksum, [len+1] cipher offset,
    // [len+2] decrypted checksum. Need room for the app id and a shortest key.
    if cert.len() < APP_ID_LEN + MIN_KEY_LEN + 3 {
        return None;
    }
    let length = cert.len() - 3;
    let off = cert[length + 1] as usize;

    // Input-byte checksum is key independent; a cheap gate that rejects blobs
    // that are not this certificate format before any search.
    let cs1 = cert[..length].iter().fold(0u32, |acc, &b| acc + b as u32) & 0xff;
    if cs1 as u8 != cert[length] {
        return None;
    }

    let mut found: Vec<Vec<u8>> = Vec::new();
    for key_len in MIN_KEY_LEN..=MAX_KEY_LEN {
        if APP_ID_LEN + key_len > length {
            break;
        }
        collect_keys(cert, off, length, key_len, &mut found);
        // More than one distinct candidate means we cannot be sure; bail early.
        if found.len() > 1 {
            return None;
        }
    }

    match found.len() {
        1 => String::from_utf8(found.pop().unwrap()).ok(),
        _ => None,
    }
}

/// Append every valid key of length `key_len` (usually zero or one) to `found`.
///
/// The decrypted tail equals the key, giving `key[j]` in terms of another key
/// digit; those relations form cycles that can be solved digit by digit. Each
/// solved key is then held to the same two checks the title applies: the
/// decrypted checksum and the tail-equals-key identity.
fn collect_keys(cert: &[u8], off: usize, length: usize, key_len: usize, found: &mut Vec<Vec<u8>>) {
    // nxt[j] is the key index that key digit j depends on.
    let nxt: Vec<usize> = (0..key_len).map(|j| (APP_ID_LEN + j + off) % key_len).collect();

    // Solve each dependency cycle independently and assemble the full key. A
    // cycle with no single digit-consistent solution means no key of this length.
    let mut key = vec![0u8; key_len];
    let mut visited = vec![false; key_len];
    for start in 0..key_len {
        if visited[start] {
            continue;
        }
        let mut cycle = Vec::new();
        let mut x = start;
        while !visited[x] {
            visited[x] = true;
            cycle.push(x);
            x = nxt[x];
        }
        let sol = match solve_cycle(cert, off, key_len, &cycle) {
            Some(sol) => sol,
            None => return,
        };
        for (pos, val) in cycle.iter().zip(sol) {
            key[*pos] = val;
        }
    }

    // Final verification: exactly what the title checks.
    let mut cs2 = 0u32;
    let mut dec_tail = Vec::with_capacity(key_len);
    for i in 0..length {
        let d = decrypt_byte(cert, &key, off, i);
        cs2 = (cs2 + d as u32) & 0xff;
        if (APP_ID_LEN..APP_ID_LEN + key_len).contains(&i) {
            dec_tail.push(d);
        }
    }
    if cs2 as u8 == cert[length + 2] && dec_tail == key && !found.contains(&key) {
        found.push(key);
    }
}

/// Return the unique all-digit assignment for `cycle`, or `None`. `cycle` lists
/// key indices in dependency order (`nxt[cycle[t]] == cycle[t + 1]`, wrapping).
fn solve_cycle(cert: &[u8], off: usize, key_len: usize, cycle: &[usize]) -> Option<Vec<u8>> {
    let k = cycle.len();
    let mut solutions: Vec<Vec<u8>> = Vec::new();

    for seed in b'0'..=b'9' {
        // key[pos] holds the working assignment for this whole key length.
        let mut key = vec![0u8; key_len];
        key[cycle[0]] = seed;

        // key[j] = decrypt at position (APP_ID_LEN + j) depends on key[nxt[j]],
        // which is the next cycle member, so resolve from the tail inward.
        let mut ok = true;
        for t in (1..k).rev() {
            let j = cycle[t];
            let d = decrypt_byte(cert, &key, off, APP_ID_LEN + j);
            if !d.is_ascii_digit() {
                ok = false;
                break;
            }
            key[j] = d;
        }
        if !ok {
            continue;
        }
        // The cycle must close back onto the chosen seed.
        let closing = decrypt_byte(cert, &key, off, APP_ID_LEN + cycle[0]);
        if closing != seed {
            continue;
        }

        let assignment: Vec<u8> = cycle.iter().map(|&pos| key[pos]).collect();
        if !solutions.contains(&assignment) {
            solutions.push(assignment);
        }
    }

    match solutions.len() {
        1 => solutions.pop(),
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Real cert.c2s from 이노티아 연대기 2 (app 0002BA13); issued for 01046119269.
    const INOTIA2_CERT: [u8; 24] = [
        0x5d, 0x8d, 0x02, 0x13, 0x65, 0xd7, 0x7e, 0x3a, 0x8e, 0x17, 0x2a, 0xda, 0x6a, 0x24, 0x21, 0xd1, 0x33, 0x1c, 0x71, 0xe1, 0x1c, 0xd9, 0xa6,
        0xe1,
    ];

    #[test]
    fn recovers_inotia2_number() {
        assert_eq!(recover_phone_number(&INOTIA2_CERT).as_deref(), Some("01046119269"));
    }

    #[test]
    fn rejects_non_certificate_blobs() {
        assert_eq!(recover_phone_number(&[]), None);
        assert_eq!(recover_phone_number(&[0u8; 24]), None); // checksum gate fails
        assert_eq!(recover_phone_number(b"not a certificate blob!!"), None);
    }

    #[test]
    fn rejects_corrupted_certificate() {
        let mut cert = INOTIA2_CERT;
        cert[23] ^= 0xff; // break the decrypted checksum
        assert_eq!(recover_phone_number(&cert), None);
    }
}
