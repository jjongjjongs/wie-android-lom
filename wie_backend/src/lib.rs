#![no_std]
extern crate alloc;

mod audio_sink;
pub mod canvas;
mod database;
mod executor;
mod platform;
mod screen;
mod system;
mod task;
mod task_runner;
mod time;

pub use self::{
    audio_sink::AudioSink,
    database::{Database, DatabaseRepository, RecordId},
    executor::{AsyncCallable, AsyncCallableResult},
    platform::{
        Filesystem, FilesystemMkdirError, FilesystemRenameError, FilesystemRmDirError, FilesystemSetModeError, Network, NetworkError, NetworkEvent,
        NetworkPoll, Platform,
    },
    screen::Screen,
    system::{Event, FilesystemOverlay, KeyCode, System},
    task_runner::{DefaultTaskRunner, TaskRunner},
    time::Instant,
};

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

use wie_util::{Result, WieError};

pub trait Emulator {
    fn handle_event(&mut self, event: Event);
    fn tick(&mut self) -> Result<()>;
    /// Whether the emulator has no work to run until a timer fires. A host that
    /// runs `tick` to a time budget can stop as soon as this is true and sleep
    /// the remainder instead of busy-waiting. The default is a conservative
    /// `false` (never idle), so a host keeps its existing run-to-budget
    /// behaviour for any emulator that does not report idleness.
    fn is_idle(&self) -> bool {
        false
    }
}

pub struct ProfileSample {
    /// Leaf-first call stack: [pc, lr, lr_prev, ...].
    pub stack: Vec<u32>,
    pub count: u64,
}

/// Called periodically during execution with a batch of samples that the
/// profiler accumulated since the previous flush. The callback also fires once
/// more when the runtime shuts down to drain anything still in the buffer.
pub type ProfileCallback = Box<dyn FnMut(Vec<ProfileSample>) + Send + Sync>;

pub struct Options {
    pub enable_gdbserver: bool,
    pub profile: Option<ProfileCallback>,
}

pub fn extract_zip(zip: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    extern crate std; // XXX

    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    // Korean handset archives carry filenames in EUC-KR alongside an Info-ZIP
    // Unicode Path extra field whose checksum does not match, because the name
    // it was computed over is not the one in the header. That is fatal to a
    // strict reader, so the field is dropped and the archive read again.
    let patched;
    let zip = match ZipArchive::new(Cursor::new(zip)) {
        Ok(_) => zip,
        Err(error) => {
            patched = drop_unicode_path_fields(zip).ok_or_else(|| WieError::FatalError(format!("Invalid zip archive: {error}")))?;

            tracing::warn!("Rereading archive without its Unicode path fields: {error}");

            &patched
        }
    };

    let mut archive = ZipArchive::new(Cursor::new(zip)).map_err(|x| WieError::FatalError(format!("Invalid zip archive: {x}")))?;

    (0..archive.len())
        .filter_map(|x| {
            let mut file = match archive.by_index(x) {
                Ok(file) => file,
                Err(err) => return Some(Err(WieError::FatalError(format!("Failed to read zip entry {x}: {err}")))),
            };
            if !file.is_file() {
                return None;
            }

            let mut data = Vec::new();
            if let Err(err) = file.read_to_end(&mut data) {
                return Some(Err(WieError::FatalError(format!("Failed to read zip entry {}: {err}", file.name()))));
            }

            Some(Ok((file.name().to_string(), data)))
        })
        .collect::<Result<_>>()
        .map(strip_common_directory)
}

/// Info-ZIP Unicode Path, the extra field whose checksum these archives get
/// wrong.
const EXTRA_FIELD_UNICODE_PATH: u16 = 0x7075;

/// Header id no writer assigns, so a reader skips the field instead of
/// checking it.
const EXTRA_FIELD_IGNORED: u16 = 0x9999;

const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?))
}

/// Renames every Unicode Path extra field so a reader ignores it.
///
/// The edit is in place and the same length, so nothing an archive records
/// about where its entries are moves. `None` if the bytes do not walk cleanly
/// as a zip, in which case the caller reports the original error.
fn drop_unicode_path_fields(zip: &[u8]) -> Option<Vec<u8>> {
    let mut patched = zip.to_vec();

    // Walk the local headers, which sit at the front and are self describing
    // as long as none of them is corrupt.
    let mut offset = 0;
    while read_u32(&patched, offset) == Some(LOCAL_HEADER_SIGNATURE) {
        let compressed_size = read_u32(&patched, offset + 18)? as usize;
        let name_length = read_u16(&patched, offset + 26)? as usize;
        let extra_length = read_u16(&patched, offset + 28)? as usize;
        let extra = offset + 30 + name_length;

        patch_extra_field(&mut patched, extra, extra_length)?;

        offset = extra + extra_length + compressed_size;
    }

    // Then the central directory, wherever the walk left off.
    while read_u32(&patched, offset) == Some(CENTRAL_HEADER_SIGNATURE) {
        let name_length = read_u16(&patched, offset + 28)? as usize;
        let extra_length = read_u16(&patched, offset + 30)? as usize;
        let comment_length = read_u16(&patched, offset + 32)? as usize;
        let extra = offset + 46 + name_length;

        patch_extra_field(&mut patched, extra, extra_length)?;

        offset = extra + extra_length + comment_length;
    }

    Some(patched)
}

/// Rewrites the ids in one extra field block.
fn patch_extra_field(data: &mut [u8], start: usize, length: usize) -> Option<()> {
    let mut offset = start;
    let end = start + length;

    while offset + 4 <= end {
        let id = read_u16(data, offset)?;
        let size = read_u16(data, offset + 2)? as usize;

        if id == EXTRA_FIELD_UNICODE_PATH {
            data.get_mut(offset..offset + 2)?.copy_from_slice(&EXTRA_FIELD_IGNORED.to_le_bytes());
        }

        offset += 4 + size;
    }

    Some(())
}

/// Feature phone archives are sometimes repacked with every entry below a
/// single directory (`P/app_info`, `0002A4B1/app_info`, ...). The loaders all
/// expect the app descriptor at the archive root, so drop a leading directory
/// component when *every* entry shares it.
fn strip_common_directory(files: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    let mut prefix: Option<&str> = None;

    for name in files.keys() {
        let Some((directory, rest)) = name.split_once('/') else {
            return files;
        };
        if rest.is_empty() {
            return files;
        }
        match prefix {
            Some(prefix) if prefix != directory => return files,
            Some(_) => {}
            None => prefix = Some(directory),
        }
    }

    let Some(prefix) = prefix.map(|x| x.to_string()) else {
        return files;
    };

    tracing::debug!("Stripping common archive directory {prefix}/");

    files
        .into_iter()
        .map(|(name, data)| (name[prefix.len() + 1..].to_string(), data))
        .collect()
}

#[cfg(test)]
mod tests {
    use alloc::{
        collections::BTreeMap,
        string::{String, ToString},
        vec::Vec,
    };

    use super::strip_common_directory;

    fn archive(names: &[&str]) -> BTreeMap<String, Vec<u8>> {
        names.iter().map(|x| (x.to_string(), Vec::new())).collect()
    }

    #[test]
    fn strips_shared_directory() {
        let files = strip_common_directory(archive(&["P/app_info", "P/0002A4B1.jar"]));

        assert!(files.contains_key("app_info"));
        assert!(files.contains_key("0002A4B1.jar"));
    }

    #[test]
    fn keeps_root_entries() {
        let files = strip_common_directory(archive(&["app_info", "res/0.png"]));

        assert!(files.contains_key("app_info"));
        assert!(files.contains_key("res/0.png"));
    }

    #[test]
    fn keeps_multiple_directories() {
        let files = strip_common_directory(archive(&["a/app_info", "b/0002A4B1.jar"]));

        assert!(files.contains_key("a/app_info"));
        assert!(files.contains_key("b/0002A4B1.jar"));
    }
}
