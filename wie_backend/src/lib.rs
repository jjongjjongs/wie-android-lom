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
    platform::{Filesystem, Platform},
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
