use alloc::{boxed::Box, string::String, vec::Vec};

use crate::{audio_sink::AudioSink, database::DatabaseRepository, screen::Screen, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    InvalidSocket,
    NotConnected,
    WouldBlock,
    TimedOut,
    ConnectionRefused,
    HostUnreachable,
    Unsupported,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPoll<T> {
    Pending,
    Ready(Result<T, NetworkError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkEvent {
    Connected(i32),
    ConnectFailed(i32),
    Readable(i32),
    Writable(i32),
}

pub trait Network: Send + Sync {
    fn socket(&self, family: i32, socket_type: i32) -> Result<i32, NetworkError>;

    fn connect(
        &self,
        socket: i32,
        address: u32,
        port: u16,
    ) -> NetworkPoll<()>;

    fn bind(
        &self,
        socket: i32,
        address: u32,
        port: u16,
    ) -> Result<(), NetworkError>;

    fn read(&self, socket: i32, buf: &mut [u8]) -> Result<usize, NetworkError>;

    fn write(&self, socket: i32, buf: &[u8]) -> Result<usize, NetworkError>;

    fn close(&self, socket: i32) -> Result<(), NetworkError>;

    fn poll_event(&self) -> Option<NetworkEvent>;
}

pub trait Platform: Send + Sync {
    fn screen(&self) -> &dyn Screen;
    fn now(&self) -> Instant;
    fn database_repository(&self) -> &dyn DatabaseRepository;
    fn filesystem(&self) -> &dyn Filesystem;
    fn audio_sink(&self) -> Box<dyn AudioSink>;

    fn network(&self) -> Option<&dyn Network> {
        None
    }

    /// Places a telephone call through the host. Returns false when the host
    /// cannot dispatch the request.
    fn call_place(&self, _number: &str) -> bool {
        false
    }

    /// Opens a URL through the host platform. Returns false when the host
    /// cannot dispatch the request.
    fn open_url(&self, _url: &str) -> bool {
        false
    }

    fn write_stdout(&self, buf: &[u8]);
    fn write_stderr(&self, buf: &[u8]);
    fn exit(&self);
    fn vibrate(&self, duration_ms: u64, intensity: u8);
    fn set_backlight_mode(&self, mode: u8);
}

/// Platform filesystem abstraction. Every method is scoped by `aid`;
/// implementations MUST NOT cross aid boundaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesystemMkdirError {
    AlreadyExists,
    NotFound,
    NameTooLong,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesystemRmDirError {
    NotFound,
    NotEmpty,
    NameTooLong,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesystemRenameError {
    NotFound,
    AlreadyExists,
    CrossDeviceOrNotEmpty,
    NameTooLong,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesystemSetModeError {
    NotFound,
    NameTooLong,
    Other,
}

#[async_trait::async_trait]
pub trait Filesystem: Send + Sync {
    async fn exists(&self, aid: &str, path: &str) -> bool;

    async fn size(&self, aid: &str, path: &str) -> Option<usize>;

    /// Read up to `count` bytes starting at `offset` into `buf[..count]`.
    ///
    /// - File missing → `None`.
    /// - `offset >= size` (read past EOF) → `Some(0)`.
    /// - Otherwise → `Some(n)` where `0 < n <= count`. Short reads allowed
    ///   at end of file.
    /// - Caller guarantees `buf.len() >= count`. Implementations only write
    ///   to `buf[..n]`.
    async fn read(&self, aid: &str, path: &str, offset: usize, count: usize, buf: &mut [u8]) -> Option<usize>;

    /// Write `data` starting at `offset`.
    ///
    /// - Creates the file (and any missing intermediate directories) if it
    ///   does not yet exist. A zero-length `data` is a valid way to
    ///   materialize an empty file.
    /// - If `offset + data.len() > current_size` the implementation MUST
    ///   automatically extend the file, zero-filling the gap.
    /// - Returns the number of bytes actually written. On success this
    ///   equals `data.len()`.
    /// - On failure (path rejected, disk full, permission denied, etc.)
    ///   MUST return `0` and log via `tracing::warn!` or `tracing::error!`.
    ///   Silent `0` returns are forbidden.
    async fn write(&self, aid: &str, path: &str, offset: usize, data: &[u8]) -> usize;

    /// Truncate the file to exactly `len` bytes. Creates the file if
    /// missing.
    /// - `len > current_size` → zero-fill extend.
    /// - `len < current_size` → tail bytes dropped.
    async fn truncate(&self, aid: &str, path: &str, len: usize);

    async fn remove(&self, aid: &str, path: &str) -> bool;

    /// Create exactly one directory.
    ///
    /// Missing parent directories are not created implicitly.
    async fn mkdir(
        &self,
        aid: &str,
        path: &str,
    ) -> core::result::Result<(), FilesystemMkdirError>;

    /// Remove exactly one empty directory.
    ///
    /// This operation is non-recursive.
    async fn rmdir(
        &self,
        aid: &str,
        path: &str,
    ) -> core::result::Result<(), FilesystemRmDirError>;

    /// Rename or move an existing filesystem object.
    ///
    /// Implementations follow the host rename operation:
    /// - an existing regular-file destination may be replaced;
    /// - the source is removed on success;
    /// - no destination parent directories are created implicitly.
    async fn rename(
        &self,
        aid: &str,
        from: &str,
        to: &str,
    ) -> core::result::Result<(), FilesystemRenameError>;

    /// Replace the permission bits of one regular file.
    ///
    /// The LGT WIPI-C filesystem exposes only owner read/write combinations:
    /// 0400, 0200 and 0600.
    async fn set_mode(
        &self,
        aid: &str,
        path: &str,
        mode: u32,
    ) -> core::result::Result<(), FilesystemSetModeError>;

    /// Total byte capacity of the storage backing this application's filesystem.
    ///
    /// `None` means the platform could not query the backing filesystem.
    async fn total_space(&self, aid: &str) -> Option<u64>;

    /// Bytes currently available to an unprivileged application on the storage
    /// backing this application's filesystem.
    ///
    /// This corresponds to native `statfs.f_bavail * f_bsize`, not `f_bfree`.
    /// `None` means the platform could not query the backing filesystem.
    async fn available_space(&self, aid: &str) -> Option<u64>;

    /// Lists direct child basenames of a directory.
    ///
    /// Files and directories are returned alike. `Some(Vec::new())` means an
    /// existing empty directory; `None` means the directory could not be
    /// opened. Implementations preserve their native enumeration order.
    async fn list(&self, aid: &str, path: &str) -> Option<Vec<String>>;
}
