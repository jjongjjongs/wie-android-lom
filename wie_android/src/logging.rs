//! Routes `tracing` output to logcat, so `adb logcat -s WIE` shows the same
//! diagnostics `wie_cli` prints to stderr.

use std::sync::Once;

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

#[cfg(target_os = "android")]
fn make_writer() -> impl std::io::Write {
    logcat::Writer::default()
}

#[cfg(not(target_os = "android"))]
fn make_writer() -> impl std::io::Write {
    std::io::stderr()
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
