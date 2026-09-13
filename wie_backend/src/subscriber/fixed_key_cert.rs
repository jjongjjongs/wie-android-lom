//! Recovery of the handset identity a title's own-key `cert.c2s` was issued for.
//!
//! Not every `cert.c2s` is the LGT ez-i certificate [`super::lgt_cert`] reads.
//! Some titles ship their own certificate under the same name, encrypted with a
//! key and IV built into the title rather than derived from the subscriber, and
//! check the handset against the fields inside it before they will start:
//! 액션퍼즐패밀리4 GS2 (AID 000315C6) reads
//! `MC_knlGetSystemProperty("MIN")`, compares it with the certificate's first
//! field, and stops at error 5001 when the two differ.
//!
//! The record is four AES-128-CBC blocks whose plaintext is
//! `<0><MIN, 12 bytes><serial, 20 bytes><four-character title tag><zero fill>`,
//! so the identity the certificate was issued for can be read straight out of
//! it. That is what the handset must be told it is: reporting the emulator's
//! usual subscriber number here is what the title sees as someone else's
//! certificate. A blob that is not this format - the LGT certificate above, or
//! any other title's - fails one of the structural checks and yields `None`, so
//! the caller keeps whatever it reports otherwise.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

/// The key the titles that use this certificate build in.
const KEY: &[u8; 16] = b"32184a2de4tj6ffc";
/// The CBC initialisation vector they build in alongside it.
const IV: [u8; 16] = [
    0x3d, 0xaf, 0xba, 0x42, 0x9d, 0x9e, 0xb4, 0x30, 0xb4, 0x22, 0xda, 0x80, 0x2c, 0x9f, 0xac, 0x41,
];

/// Bytes of the certificate that are encrypted; a longer file is zero padding.
const RECORD_LEN: usize = 64;
/// Field offsets and lengths in the decrypted record.
const MIN_AT: usize = 1;
const MIN_LEN: usize = 12;
const SERIAL_AT: usize = 13;
const SERIAL_LEN: usize = 20;
const TAG_AT: usize = 33;
const TAG_LEN: usize = 4;

/// The handset identity a certificate names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandsetIdentity {
    /// The `MIN` the certificate was issued for, empty where it names none -
    /// which is itself the answer the title expects, so it is kept as read.
    pub min: String,
    /// The handset serial the certificate was issued for.
    pub serial: String,
}

/// Read the handset identity out of a title's own-key `cert.c2s`, or `None`
/// when the blob is not one.
pub fn recover_identity(cert: &[u8]) -> Option<HandsetIdentity> {
    if cert.len() < RECORD_LEN {
        return None;
    }

    let plain = decrypt_cbc(&cert[..RECORD_LEN]);

    // Structure the title itself relies on: a zero lead byte, two NUL-terminated
    // ASCII fields, a printable four-character tag, and nothing but zeroes after
    // it. Enough to tell this record apart from a blob that merely decrypts.
    if plain[0] != 0 || plain[TAG_AT + TAG_LEN..].iter().any(|&b| b != 0) {
        return None;
    }
    if !plain[TAG_AT..TAG_AT + TAG_LEN].iter().all(u8::is_ascii_graphic) {
        return None;
    }

    let min = field(&plain[MIN_AT..MIN_AT + MIN_LEN])?;
    let serial = field(&plain[SERIAL_AT..SERIAL_AT + SERIAL_LEN])?;

    Some(HandsetIdentity { min, serial })
}

/// One NUL-terminated field: printable ASCII up to the terminator, zeroes after
/// it. `None` when the bytes are anything else, which is how a blob that is not
/// this certificate is rejected.
fn field(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0)?;
    if bytes[end..].iter().any(|&b| b != 0) || !bytes[..end].iter().all(u8::is_ascii_graphic) {
        return None;
    }

    Some(core::str::from_utf8(&bytes[..end]).ok()?.to_string())
}

/// AES-128-CBC decryption of a whole number of blocks under [`KEY`]/[`IV`].
fn decrypt_cbc(cipher: &[u8]) -> Vec<u8> {
    let round_keys = expand_key(KEY);

    let mut plain = Vec::with_capacity(cipher.len());
    let mut chain = IV;
    for block in cipher.chunks_exact(16) {
        let mut state = [0u8; 16];
        state.copy_from_slice(block);
        decrypt_block(&mut state, &round_keys);
        for (out, previous) in state.iter().zip(chain) {
            plain.push(out ^ previous);
        }
        chain.copy_from_slice(block);
    }

    plain
}

/// AES S-box, and the inverse used by the decryption rounds.
#[rustfmt::skip]
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

#[rustfmt::skip]
const INV_SBOX: [u8; 256] = [
    0x52, 0x09, 0x6a, 0xd5, 0x30, 0x36, 0xa5, 0x38, 0xbf, 0x40, 0xa3, 0x9e, 0x81, 0xf3, 0xd7, 0xfb,
    0x7c, 0xe3, 0x39, 0x82, 0x9b, 0x2f, 0xff, 0x87, 0x34, 0x8e, 0x43, 0x44, 0xc4, 0xde, 0xe9, 0xcb,
    0x54, 0x7b, 0x94, 0x32, 0xa6, 0xc2, 0x23, 0x3d, 0xee, 0x4c, 0x95, 0x0b, 0x42, 0xfa, 0xc3, 0x4e,
    0x08, 0x2e, 0xa1, 0x66, 0x28, 0xd9, 0x24, 0xb2, 0x76, 0x5b, 0xa2, 0x49, 0x6d, 0x8b, 0xd1, 0x25,
    0x72, 0xf8, 0xf6, 0x64, 0x86, 0x68, 0x98, 0x16, 0xd4, 0xa4, 0x5c, 0xcc, 0x5d, 0x65, 0xb6, 0x92,
    0x6c, 0x70, 0x48, 0x50, 0xfd, 0xed, 0xb9, 0xda, 0x5e, 0x15, 0x46, 0x57, 0xa7, 0x8d, 0x9d, 0x84,
    0x90, 0xd8, 0xab, 0x00, 0x8c, 0xbc, 0xd3, 0x0a, 0xf7, 0xe4, 0x58, 0x05, 0xb8, 0xb3, 0x45, 0x06,
    0xd0, 0x2c, 0x1e, 0x8f, 0xca, 0x3f, 0x0f, 0x02, 0xc1, 0xaf, 0xbd, 0x03, 0x01, 0x13, 0x8a, 0x6b,
    0x3a, 0x91, 0x11, 0x41, 0x4f, 0x67, 0xdc, 0xea, 0x97, 0xf2, 0xcf, 0xce, 0xf0, 0xb4, 0xe6, 0x73,
    0x96, 0xac, 0x74, 0x22, 0xe7, 0xad, 0x35, 0x85, 0xe2, 0xf9, 0x37, 0xe8, 0x1c, 0x75, 0xdf, 0x6e,
    0x47, 0xf1, 0x1a, 0x71, 0x1d, 0x29, 0xc5, 0x89, 0x6f, 0xb7, 0x62, 0x0e, 0xaa, 0x18, 0xbe, 0x1b,
    0xfc, 0x56, 0x3e, 0x4b, 0xc6, 0xd2, 0x79, 0x20, 0x9a, 0xdb, 0xc0, 0xfe, 0x78, 0xcd, 0x5a, 0xf4,
    0x1f, 0xdd, 0xa8, 0x33, 0x88, 0x07, 0xc7, 0x31, 0xb1, 0x12, 0x10, 0x59, 0x27, 0x80, 0xec, 0x5f,
    0x60, 0x51, 0x7f, 0xa9, 0x19, 0xb5, 0x4a, 0x0d, 0x2d, 0xe5, 0x7a, 0x9f, 0x93, 0xc9, 0x9c, 0xef,
    0xa0, 0xe0, 0x3b, 0x4d, 0xae, 0x2a, 0xf5, 0xb0, 0xc8, 0xeb, 0xbb, 0x3c, 0x83, 0x53, 0x99, 0x61,
    0x17, 0x2b, 0x04, 0x7e, 0xba, 0x77, 0xd6, 0x26, 0xe1, 0x69, 0x14, 0x63, 0x55, 0x21, 0x0c, 0x7d,
];

/// The eleven round keys of the AES-128 schedule, in encryption order.
fn expand_key(key: &[u8; 16]) -> [[u8; 16]; 11] {
    let mut keys = [[0u8; 16]; 11];
    keys[0].copy_from_slice(key);

    let mut rcon = 1u8;
    for round in 1..=10 {
        let previous = keys[round - 1];
        let mut word = [
            SBOX[previous[13] as usize] ^ rcon,
            SBOX[previous[14] as usize],
            SBOX[previous[15] as usize],
            SBOX[previous[12] as usize],
        ];
        rcon = xtime(rcon);

        for column in 0..4 {
            for byte in 0..4 {
                word[byte] ^= previous[column * 4 + byte];
                keys[round][column * 4 + byte] = word[byte];
            }
        }
    }

    keys
}

/// Multiplication by x in GF(2^8) with the AES polynomial.
fn xtime(byte: u8) -> u8 {
    (byte << 1) ^ if byte & 0x80 != 0 { 0x1b } else { 0 }
}

/// Multiplication in GF(2^8), used by `InvMixColumns`.
fn mul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0;
    while b != 0 {
        if b & 1 != 0 {
            product ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }

    product
}

/// One AES-128 block decryption, in place.
fn decrypt_block(state: &mut [u8; 16], keys: &[[u8; 16]; 11]) {
    add_round_key(state, &keys[10]);
    for round in (1..10).rev() {
        inv_shift_rows(state);
        inv_sub_bytes(state);
        add_round_key(state, &keys[round]);
        inv_mix_columns(state);
    }
    inv_shift_rows(state);
    inv_sub_bytes(state);
    add_round_key(state, &keys[0]);
}

fn add_round_key(state: &mut [u8; 16], key: &[u8; 16]) {
    for (byte, key_byte) in state.iter_mut().zip(key) {
        *byte ^= key_byte;
    }
}

fn inv_sub_bytes(state: &mut [u8; 16]) {
    for byte in state.iter_mut() {
        *byte = INV_SBOX[*byte as usize];
    }
}

/// Rows 1..3 of the state rotate right by their row number; the state is held
/// column-major, so byte `column * 4 + row` moves to `(column + row) % 4`.
fn inv_shift_rows(state: &mut [u8; 16]) {
    let source = *state;
    for column in 0..4 {
        for row in 0..4 {
            state[((column + row) % 4) * 4 + row] = source[column * 4 + row];
        }
    }
}

fn inv_mix_columns(state: &mut [u8; 16]) {
    for column in state.chunks_exact_mut(4) {
        let [a, b, c, d] = [column[0], column[1], column[2], column[3]];
        column[0] = mul(a, 14) ^ mul(b, 11) ^ mul(c, 13) ^ mul(d, 9);
        column[1] = mul(a, 9) ^ mul(b, 14) ^ mul(c, 11) ^ mul(d, 13);
        column[2] = mul(a, 13) ^ mul(b, 9) ^ mul(c, 14) ^ mul(d, 11);
        column[3] = mul(a, 11) ^ mul(b, 13) ^ mul(c, 9) ^ mul(d, 14);
    }
}

#[cfg(test)]
mod tests {
    use super::{HandsetIdentity, decrypt_cbc, recover_identity};
    use alloc::string::ToString;

    /// The real `cert.c2s` shipped with 액션퍼즐패밀리4 GS2 (AID 000315C6).
    #[rustfmt::skip]
    const APF4: [u8; 76] = [
        0x7c, 0x37, 0x17, 0x2b, 0x26, 0x54, 0x08, 0xfd, 0x4f, 0xdc, 0x4f, 0x39, 0x5b, 0x6b, 0xe7, 0xf1,
        0x0a, 0x11, 0xce, 0x03, 0xd9, 0x34, 0x03, 0xbc, 0x12, 0x3c, 0x13, 0xc8, 0xc6, 0x33, 0x92, 0xf5,
        0x7c, 0x9a, 0x27, 0x6e, 0x80, 0xa4, 0xc5, 0xae, 0xb0, 0x12, 0x0e, 0x93, 0xa2, 0x2b, 0xe3, 0x40,
        0x91, 0xcb, 0xe9, 0x86, 0x7a, 0xe5, 0xad, 0x5d, 0xd8, 0x08, 0xd6, 0x53, 0x44, 0x19, 0x20, 0x64,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    /// The plaintext the title's own decryption produces, field by field: a zero
    /// lead byte, an empty `MIN`, the serial, and the `APF4` tag it checks.
    #[test]
    fn the_record_decrypts_to_the_layout_the_title_reads() {
        let plain = decrypt_cbc(&APF4[..64]);

        assert_eq!(plain[0], 0);
        assert_eq!(&plain[1..13], &[0u8; 12]);
        assert_eq!(&plain[13..21], b"01042875");
        assert_eq!(&plain[33..37], b"APF4");
        assert!(plain[37..].iter().all(|&b| b == 0));
    }

    #[test]
    fn the_identity_is_read_out_of_the_certificate() {
        assert_eq!(
            recover_identity(&APF4),
            Some(HandsetIdentity {
                min: "".to_string(),
                serial: "01042875".to_string()
            })
        );
    }

    /// A blob that is not this certificate decrypts to noise, which the
    /// structural checks reject rather than reporting a made-up identity.
    #[test]
    fn anything_that_is_not_this_certificate_is_not_read_as_one() {
        assert_eq!(recover_identity(&[0u8; 76]), None);
        assert_eq!(recover_identity(&APF4[..48]), None);

        let mut damaged = APF4;
        damaged[0] ^= 1;
        assert_eq!(recover_identity(&damaged), None);
    }
}
