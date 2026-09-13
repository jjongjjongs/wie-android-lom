//! What subscriber number an archive will report.
//!
//! The number is not a constant: it is recovered from the archive itself, and a
//! title uses it as the key that decrypts its certificate, as the value
//! `MC_knlGetSystemProperty("PHONENUMBER")` answers, and as the `MDN` the
//! billing header carries. Saying which number a given archive lands on - and
//! which of its files decided - is otherwise only visible by running the title
//! and hoping it logs.
//!
//! Diagnostic; retail archives are not in the repository.
//!
//! ```text
//! WIE_ARCHIVE=/path/to/title.zip cargo test -p wie_lgt --test subscriber_number -- --ignored --nocapture
//! ```

use wie_backend::{extract_zip, subscriber};

/// The digits the billing header carries, which is the number padded back to
/// the twelve the carrier's own format uses: the three-digit prefix, then a
/// zero for each digit the subscriber's own number leaves out, then the rest.
///
/// Mirrors `normalize_lgt_bill_mdn`, which reproduces the reference firmware's
/// `WPBill_SetHeader`.
fn billing_digits(number: &str) -> String {
    match number.len() {
        10 => format!("{}00{}", &number[..3], &number[3..]),
        11 => format!("{}0{}", &number[..3], &number[3..]),
        _ => number.to_owned(),
    }
}

#[test]
#[ignore = "diagnostic"]
fn report_subscriber_number() {
    let Ok(path) = std::env::var("WIE_ARCHIVE") else {
        eprintln!("Set WIE_ARCHIVE to an archive to report the number it names");
        return;
    };

    let archive = std::fs::read(&path).expect("archive");
    let files = extract_zip(&archive).expect("archive contents");

    let file = |name: &str| {
        files
            .iter()
            .find(|(path, _)| *path == name || path.ends_with(&format!("/{name}")))
            .map(|(_, data)| data.clone())
    };

    let cert = file("cert.c2s");
    let certification = file("certification");
    let app_info = file("app_info");

    let number = subscriber::subscriber_number(cert.as_deref(), certification.as_deref(), app_info.as_deref());

    let source = if cert.as_deref().and_then(subscriber::from_cert).is_some() {
        "cert.c2s"
    } else if certification.as_deref().and_then(subscriber::from_certification).is_some() {
        "certification"
    } else if app_info.as_deref().and_then(subscriber::from_descriptor).is_some() {
        "app_info"
    } else {
        "fallback"
    };

    println!(
        "{}: {number} ({} digits, from {source}) -> billing header {}",
        path.rsplit('/').next().unwrap_or(&path),
        number.len(),
        billing_digits(&number)
    );
    println!(
        "  files present: cert.c2s {} certification {} app_info {}",
        cert.is_some(),
        certification.is_some(),
        app_info.is_some()
    );
}
