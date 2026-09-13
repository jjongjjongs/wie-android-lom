//! The subscriber phone number a title is told it is running on.
//!
//! LGT titles use it as more than a number: it is the key that decrypts their
//! `cert.c2s`, and several check it against a certificate of their own before
//! they will start. Reporting the wrong one is what a title sees as a pirated
//! copy, so it is recovered from the archive rather than invented - and both
//! the WIPI-C `MC_knlGetSystemProperty("PHONENUMBER")` path and the WIPI-Java
//! `HandsetProperty.getSystemProperty` one have to answer the same thing, or a
//! title that reads it through both disagrees with itself.

mod fixed_key_cert;
mod lgt_cert;

pub use self::{
    fixed_key_cert::{HandsetIdentity, recover_identity as identity_from_cert},
    lgt_cert::recover_phone_number as from_cert,
};

use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
};

/// The number reported when nothing in the archive names one.
///
/// A collection dumped from one handset shares its number, so this is a working
/// default for such a set as well as a valid-looking value for a title that
/// only checks it has one.
pub const FALLBACK: &str = "01046119269";

/// The subscriber number an LGT archive descriptor was downloaded for: the
/// `ctn` query parameter of the `DDurl` it carries.
///
/// Matched only where the URL itself starts a parameter (`?ctn=` / `&ctn=`), so
/// the `send_ctn` of a gifted copy - the sender's number, not the subscriber's -
/// is not mistaken for it. Returns `None` unless the value is a plausible
/// subscriber number, so a descriptor without one falls back to the caller's
/// placeholder.
///
/// The store wrote the value in the carrier's padded twelve-digit form more
/// often than in the subscriber's own, so what it names is [`unpad_mdn`]'d back
/// to the number a handset would report.
pub fn from_descriptor(app_info: &[u8]) -> Option<String> {
    // Scanned as bytes: a descriptor's name and vendor fields are EUC-KR, so it
    // is not valid UTF-8 as a whole.
    const KEY: &[u8] = b"ctn=";

    app_info
        .windows(KEY.len())
        .enumerate()
        .filter(|&(at, window)| window == KEY && matches!(at.checked_sub(1).map(|before| app_info[before]), Some(b'?' | b'&')))
        .map(|(at, _)| {
            let digits = &app_info[at + KEY.len()..];
            let end = digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
            &digits[..end]
        })
        .find(|number| (10..=12).contains(&number.len()) && number.first() == Some(&b'0'))
        .map(|number| unpad_mdn(&String::from_utf8_lossy(number)))
}

/// The subscriber's own number behind a twelve-digit MDN.
///
/// The store wrote the `ctn` in the carrier's twelve-digit form, which is the
/// subscriber number with its three-digit prefix padded back out: `WPBill_Write`
/// builds it by inserting `"00"` after the prefix of a ten-digit number and
/// `"0"` after that of an eleven-digit one. A handset reports the number it was
/// padded from, not the padding - so undo it, and let the billing header pad it
/// again when that is what the header wants.
///
/// The two paddings are told apart the way they were made: a `"00"` after the
/// prefix came from a ten-digit number, a single `"0"` from an eleven-digit one.
/// Anything that is not twelve digits, or that carries no padding to remove, is
/// already the number and is returned unchanged.
fn unpad_mdn(number: &str) -> String {
    if number.len() != 12 || !number.is_char_boundary(5) {
        return number.to_owned();
    }

    match &number[3..5] {
        "00" => alloc::format!("{}{}", &number[..3], &number[5..]),
        padded if padded.starts_with('0') => alloc::format!("{}{}", &number[..3], &number[4..]),
        _ => number.to_owned(),
    }
}

/// Reads a `certification` file that is just the subscriber number as ASCII
/// digits (optionally NUL-terminated or padded). Returns `None` when it is not a
/// plain phone number, so a differently-formatted certification is ignored and
/// the caller falls back to its placeholder.
pub fn from_certification(data: &[u8]) -> Option<String> {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let number = core::str::from_utf8(&data[..end]).ok()?.trim();
    if (10..=15).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit()) {
        Some(number.to_string())
    } else {
        None
    }
}

/// The number to report, from whichever of the archive's files names it.
///
/// The order is the order of authority: a certificate the number was issued
/// for, then a plain `certification` holding it, then the descriptor's own
/// download URL. Each argument is the file's bytes, or `None` where the archive
/// has no such file.
pub fn subscriber_number(cert: Option<&[u8]>, certification: Option<&[u8]>, app_info: Option<&[u8]>) -> String {
    if let Some(number) = cert.and_then(from_cert) {
        tracing::info!("recovered subscriber number from cert.c2s: {number:?}");
        return number;
    }

    if let Some(number) = certification.and_then(from_certification) {
        tracing::info!("recovered subscriber number from certification: {number:?}");
        return number;
    }

    if let Some(number) = app_info.and_then(from_descriptor) {
        tracing::info!("recovered subscriber number from app_info: {number:?}");
        return number;
    }

    FALLBACK.to_string()
}

#[cfg(test)]
mod tests {
    use super::{FALLBACK, from_certification, from_descriptor, subscriber_number};

    #[test]
    fn a_certification_that_is_just_the_number_is_read_as_one() {
        assert_eq!(from_certification(b"01000000000\0").as_deref(), Some("01000000000"));
        assert_eq!(from_certification(b"01046119269").as_deref(), Some("01046119269"));

        assert_eq!(from_certification(b"not-a-number"), None);
        assert_eq!(from_certification(b"123"), None);
        assert_eq!(from_certification(b""), None);
    }

    #[test]
    fn a_descriptor_names_the_number_its_copy_was_downloaded_for() {
        let app_info = b"AID:000315C6\r\nDDurl:http://omadn.ez-i.co.kr:9089/oma_dd.dn?ctn=010085300848&req_pltf=1\r\n";
        assert_eq!(from_descriptor(app_info).as_deref(), Some("01085300848"));

        // A descriptor with no `ctn`, and a gifted copy's `send_ctn` - the
        // sender's number, not the subscriber's.
        assert_eq!(from_descriptor(b"AID:000315C6\r\nDDurl:http://example/dd.dn?pid=1\r\n"), None);
        assert_eq!(from_descriptor(b"DDurl:http://example/dd.dn?a=1&send_ctn=010022055752"), None);

        // Values that are not plausible subscriber numbers.
        assert_eq!(from_descriptor(b"DDurl:http://example/dd.dn?ctn=0100&b=2"), None);
        assert_eq!(from_descriptor(b"DDurl:http://example/dd.dn?ctn=910085300848"), None);
    }

    #[test]
    fn a_descriptor_number_comes_back_as_the_subscriber_s_own_not_the_padded_one() {
        // Every `ctn` measured off these archives is the twelve-digit carrier
        // form; a handset reports the eleven it was padded from.
        for (padded, subscriber) in [
            ("010055452383", "01055452383"),
            ("010024882970", "01024882970"),
            ("010023276993", "01023276993"),
            ("010068780247", "01068780247"),
            ("010077405004", "01077405004"),
            ("010024341091", "01024341091"),
        ] {
            let app_info = alloc::format!("DDurl:http://example/dd.dn?ctn={padded}");
            assert_eq!(from_descriptor(app_info.as_bytes()).as_deref(), Some(subscriber), "{padded}");
        }
    }

    #[test]
    fn a_ten_digit_number_is_told_from_an_eleven_digit_one_by_its_padding() {
        // `"00"` after the prefix is what a ten-digit number was padded with.
        assert_eq!(
            from_descriptor(b"DDurl:http://example/dd.dn?ctn=011001234567").as_deref(),
            Some("0111234567")
        );

        // A single `"0"` is an eleven-digit number's padding.
        assert_eq!(
            from_descriptor(b"DDurl:http://example/dd.dn?ctn=011012345678").as_deref(),
            Some("01112345678")
        );
    }

    #[test]
    fn a_number_that_carries_no_padding_is_left_as_it_is() {
        // Eleven digits already, and twelve with no zero where padding would be.
        assert_eq!(
            from_descriptor(b"DDurl:http://example/dd.dn?ctn=01055452383").as_deref(),
            Some("01055452383")
        );
        assert_eq!(
            from_descriptor(b"DDurl:http://example/dd.dn?ctn=010155452383").as_deref(),
            Some("010155452383")
        );
    }

    #[test]
    fn the_files_are_consulted_in_the_order_of_their_authority() {
        let certification = b"01011112222";
        let app_info = b"DDurl:http://example/dd.dn?ctn=010085300848";

        // The certificate outranks both, but a `cert.c2s` this cannot recover
        // steps aside rather than blocking them.
        assert_eq!(
            subscriber_number(Some(b"not a certificate"), Some(certification), Some(app_info)),
            "01011112222"
        );
        assert_eq!(subscriber_number(None, None, Some(app_info)), "01085300848");
        assert_eq!(subscriber_number(None, None, None), FALLBACK);
    }
}
