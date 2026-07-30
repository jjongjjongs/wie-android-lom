//! Routes `tracing` output to logcat, so `adb logcat -s WIE` shows the same
//! diagnostics `wie_cli` prints to stderr - and keeps a copy, so a phone with
//! no `adb` attached can still hand the log over.
//!
//! The copy is what the player's log button saves. It is cleared when a game
//! starts, so what comes out is that one run and not the whole session.

use std::{
    collections::VecDeque,
    io,
    sync::{LazyLock, Mutex, Once},
};

static INIT: Once = Once::new();

/// Installs the subscriber once. Safe to call from every entry point.
pub fn init() {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(make_writer)
            .init();
    });
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

    use super::{MAX_LINES, make_writer, reset, snapshot};

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
