//! Routes `tracing` output to logcat, so `adb logcat -s WIE` shows the same
//! diagnostics `wie_cli` prints to stderr - and keeps a copy, so a phone with
//! no `adb` attached can still hand the log over.
//!
//! The copy is what the player's log button saves. It is cleared when a game
//! starts, so what comes out is that one run and not the whole session.

use std::{
    collections::VecDeque,
    io,
    sync::{LazyLock, Mutex, Once, OnceLock},
};

use tracing_subscriber::{EnvFilter, Registry, prelude::*, reload};

static INIT: Once = Once::new();

/// Default log directive used when `RUST_LOG` is unset.
///
/// Captures as much as is useful with a single press of the log button, so most
/// debugging needs no filter change: everything at debug, plus trace for the
/// platform and loader crates - the low-frequency lifecycle, resource and
/// class-loading detail that explains why a title fails to start or misbehaves.
///
/// Only the two known floods are held back, because at debug/trace they bury the
/// rest and slow the game: the ARM interpreter's per-instruction trace
/// (`arm32_cpu`) and the JIT's per-block bookkeeping (`wie_core_arm`) drop to
/// info/warn, and the graphics service's per-frame drawing stays at info. Faults
/// and unimplemented calls in those crates log at warn/error, so they still
/// show. Setting `RUST_LOG`, or the in-app log filter, overrides this entirely.
const DEFAULT_LOG_DIRECTIVE: &str =
    "debug,wie_lgt=trace,wie_ktf=trace,wie_j2me=trace,wie_skt=trace,wie_core_arm=info,wie_wipi_c::api::graphics=info,arm32_cpu=warn";

/// Lets the player swap the log filter at runtime, so capturing a module's
/// debug/trace detail no longer means editing the default above and rebuilding.
static RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

/// The directive the filter is currently running, so the UI can show it.
static CURRENT_DIRECTIVE: Mutex<String> = Mutex::new(String::new());

/// Installs the subscriber once. Safe to call from every entry point.
pub fn init() {
    INIT.call_once(|| {
        // Honour `RUST_LOG` when it is set and non-empty; otherwise the default.
        let directive = std::env::var(EnvFilter::DEFAULT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_LOG_DIRECTIVE.to_owned());

        // `EnvFilter::new` is lenient, matching the previous startup behaviour:
        // a bad directive here degrades rather than dropping logging entirely.
        let (filter, handle) = reload::Layer::new(EnvFilter::new(&directive));

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(make_writer))
            .init();

        let _ = RELOAD_HANDLE.set(handle);
        *current_directive() = directive;
    });
}

fn current_directive() -> std::sync::MutexGuard<'static, String> {
    CURRENT_DIRECTIVE.lock().unwrap_or_else(|x| x.into_inner())
}

/// The log filter the capture is currently running.
pub fn filter() -> String {
    current_directive().clone()
}

/// Swaps the live log filter without a rebuild. An empty directive restores the
/// built-in default, so the caller has a single source of truth for it. Returns
/// the reason parsing rejected the directive, so it can be shown. Parsing is
/// strict here (unlike startup) so a typo is reported instead of silently
/// dropped.
pub fn set_filter(directive: &str) -> core::result::Result<(), String> {
    let directive = if directive.trim().is_empty() { DEFAULT_LOG_DIRECTIVE } else { directive };

    let parsed = EnvFilter::builder().parse(directive).map_err(|error| error.to_string())?;

    let handle = RELOAD_HANDLE.get().ok_or_else(|| "logging is not initialised yet".to_owned())?;
    handle.reload(parsed).map_err(|error| error.to_string())?;

    *current_directive() = directive.to_owned();
    Ok(())
}

/// Lines kept, and bytes. A run that overruns either drops its oldest, which
/// is the right end to lose: a title that fails at startup fits whole, and one
/// that hangs is diagnosed from where it stopped.
const MAX_LINES: usize = 20_000;
const MAX_BYTES: usize = 2 << 20;

#[derive(Default)]
struct Record {
    lines: VecDeque<String>,
    bytes: usize,
    /// Lines dropped to stay inside the bounds, so the snapshot can say so.
    dropped: usize,
}

impl Record {
    fn push(&mut self, line: String) {
        self.bytes += line.len() + 1;
        self.lines.push_back(line);

        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            let Some(oldest) = self.lines.pop_front() else {
                break;
            };
            self.bytes -= oldest.len() + 1;
            self.dropped += 1;
        }
    }
}

static RECORD: LazyLock<Mutex<Record>> = LazyLock::new(|| Mutex::new(Record::default()));

fn record() -> std::sync::MutexGuard<'static, Record> {
    RECORD.lock().unwrap_or_else(|x| x.into_inner())
}

/// Starts the log over, so what is saved covers one run of one game.
pub fn reset() {
    *record() = Record::default();
}

/// Everything logged since the last [`reset`].
pub fn snapshot() -> String {
    let record = record();

    let mut out = String::with_capacity(record.bytes + 128);
    if record.dropped > 0 {
        out.push_str(&format!("[{} earlier lines dropped to stay inside the log's bounds]\n", record.dropped));
    }
    for line in &record.lines {
        out.push_str(line);
        out.push('\n');
    }

    out
}

fn make_writer() -> Tee {
    Tee {
        inner: platform_writer(),
        pending: Vec::new(),
    }
}

/// Writes each line to the platform's log and to [`RECORD`].
struct Tee {
    inner: PlatformWriter,
    /// `tracing` writes a line in several pieces, and a line is the unit both
    /// destinations want.
    pending: Vec<u8>,
}

impl io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;

        self.pending.extend_from_slice(buf);
        while let Some(end) = self.pending.iter().position(|&x| x == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).take(end).collect();
            record().push(String::from_utf8_lossy(&line).into_owned());
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            record().push(String::from_utf8_lossy(&line).into_owned());
        }

        self.inner.flush()
    }
}

impl Drop for Tee {
    fn drop(&mut self) {
        use io::Write as _;

        let _ = self.flush();
    }
}

#[cfg(target_os = "android")]
type PlatformWriter = logcat::Writer;

#[cfg(not(target_os = "android"))]
type PlatformWriter = io::Stderr;

#[cfg(target_os = "android")]
fn platform_writer() -> PlatformWriter {
    logcat::Writer::default()
}

#[cfg(not(target_os = "android"))]
fn platform_writer() -> PlatformWriter {
    io::stderr()
}

#[cfg(target_os = "android")]
mod logcat {
    use std::{
        ffi::{CString, c_char, c_int},
        io,
    };

    // liblog is part of the NDK sysroot and always present on device.
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    /// ANDROID_LOG_INFO. The level is already in the formatted line, and
    /// logcat filtering happens by tag here.
    const PRIORITY_INFO: c_int = 4;

    /// Buffers until a newline, because logcat treats every write as one
    /// entry while `tracing` writes a line in several pieces.
    #[derive(Default)]
    pub struct Writer {
        buffer: Vec<u8>,
    }

    impl Writer {
        fn emit(&self, line: &[u8]) {
            // Interior nul bytes would truncate the line; they never appear in
            // formatted output, but a lossy replacement is better than a panic.
            let Ok(tag) = CString::new("WIE") else { return };
            let Ok(text) = CString::new(line.iter().map(|&x| if x == 0 { b'?' } else { x }).collect::<Vec<_>>()) else {
                return;
            };

            unsafe { __android_log_write(PRIORITY_INFO, tag.as_ptr(), text.as_ptr()) };
        }
    }

    impl io::Write for Writer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);

            while let Some(end) = self.buffer.iter().position(|&x| x == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).take(end).collect();
                self.emit(&line);
            }

            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if !self.buffer.is_empty() {
                let line = std::mem::take(&mut self.buffer);
                self.emit(&line);
            }

            Ok(())
        }
    }

    impl Drop for Writer {
        fn drop(&mut self) {
            use io::Write as _;

            let _ = self.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::{DEFAULT_LOG_DIRECTIVE, MAX_LINES, make_writer, reset, snapshot};

    #[test]
    fn default_log_directive_parses() {
        // A malformed directive would be dropped, quietly reverting the full-log
        // capture to bare info and losing the debug detail it is meant to keep.
        tracing_subscriber::EnvFilter::builder()
            .parse(DEFAULT_LOG_DIRECTIVE)
            .expect("default log directive must be valid");
    }

    /// The tests share one global record, so they run as one.
    #[test]
    fn the_log_is_kept_and_bounded() {
        reset();
        assert_eq!(snapshot(), "");

        let mut writer = make_writer();
        // A line arriving in pieces, which is how `tracing` writes one.
        writer.write_all(b"hello ").unwrap();
        writer.write_all(b"world\n").unwrap();

        assert_eq!(snapshot(), "hello world\n");

        // A tail with no newline is still kept, on flush.
        writer.write_all(b"unfinished").unwrap();
        writer.flush().unwrap();
        assert!(snapshot().contains("unfinished"));

        reset();
        for index in 0..MAX_LINES + 50 {
            writer.write_all(format!("line {index}\n").as_bytes()).unwrap();
        }

        let log = snapshot();
        assert!(log.starts_with("[50 earlier lines dropped"), "{}", &log[..64]);
        assert!(log.contains(&format!("line {}", MAX_LINES + 49)), "the newest line is missing");
        assert!(!log.contains("line 0\n"), "the oldest line was kept");

        reset();
    }
}
