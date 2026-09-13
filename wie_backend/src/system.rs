mod audio;
mod event_queue;
mod file_system;
mod input_method;

use alloc::{borrow::ToOwned, boxed::Box, string::String, sync::Arc};

use spin::{RwLock, RwLockWriteGuard};

use wie_util::Result;

use crate::{
    AsyncCallable,
    executor::Executor,
    local_network::LocalNetwork,
    platform::Platform,
    task::{SleepFuture, YieldFuture},
    task_runner::TaskRunner,
};

use self::{audio::Audio, event_queue::EventQueue, input_method::InputMethod};

pub use self::{
    event_queue::{Event, KeyCode},
    file_system::FilesystemOverlay,
    input_method::InputMethodOutput,
};

#[derive(Clone)]
pub struct System {
    pid: String,
    aid: String,
    executor: Executor,
    platform: Arc<Box<dyn Platform>>,
    filesystem: FilesystemOverlay,
    event_queue: Arc<RwLock<EventQueue>>,
    audio: Arc<RwLock<Audio>>,
    input_method: Arc<RwLock<InputMethod>>,
    task_runner: Arc<dyn TaskRunner>,
    /// The servers this run answers for itself, in place of ones that have been
    /// switched off for years. Empty unless the host registered one.
    local_network: Arc<RwLock<LocalNetwork>>,
}

impl System {
    pub fn new<T>(platform: Box<dyn Platform>, pid: &str, aid: &str, task_runner: T) -> Self
    where
        T: TaskRunner + 'static,
    {
        let audio_sink = platform.audio_sink();

        let mut local_network = LocalNetwork::new();
        for endpoint in platform.local_endpoints() {
            local_network.register(endpoint);
        }

        let platform = Arc::new(platform);

        Self {
            pid: pid.to_owned(),
            aid: aid.to_owned(), // TODO create metadata dictionary or something
            executor: Executor::new(),
            filesystem: FilesystemOverlay::new(platform.clone(), aid),
            platform,
            event_queue: Arc::new(RwLock::new(EventQueue::new())),
            audio: Arc::new(RwLock::new(Audio::new(audio_sink))),
            input_method: Arc::new(RwLock::new(InputMethod::new())),
            task_runner: Arc::new(task_runner),
            local_network: Arc::new(RwLock::new(local_network)),
        }
    }

    pub fn tick(&mut self) -> Result<()> {
        let platform = self.platform.clone();
        self.executor.tick(move || platform.now())
    }

    /// Whether the emulator has nothing runnable until a timer fires (every
    /// task asleep with its wake-up in the future). The host loop uses this to
    /// stop early and sleep the leftover budget rather than busy-waiting.
    pub fn is_idle(&self) -> bool {
        self.executor.is_idle()
    }

    pub fn spawn<C>(&self, callable: C)
    where
        C: AsyncCallable<Result<()>> + 'static + Send,
    {
        let runner_clone = self.task_runner.clone();
        self.executor.spawn(async move || runner_clone.run(Box::pin(callable.call())).await);
    }

    pub fn sleep(&self, timeout: u64) -> SleepFuture {
        SleepFuture::new(timeout, &self.executor)
    }

    pub fn current_task_id(&self) -> u64 {
        self.executor.current_task_id()
    }

    pub fn yield_now(&self) -> YieldFuture {
        YieldFuture::new()
    }

    /// Unified filesystem view. Reads consult the persistent platform
    /// backend first and fall back to the in-memory virtual layer loaded
    /// from archives; writes always hit the platform backend.
    pub fn filesystem(&self) -> &FilesystemOverlay {
        &self.filesystem
    }

    pub fn pid(&self) -> &str {
        &self.pid
    }

    pub fn aid(&self) -> &str {
        &self.aid
    }

    pub fn local_network(&self) -> RwLockWriteGuard<'_, LocalNetwork> {
        self.local_network.write()
    }

    pub fn platform(&self) -> &dyn Platform {
        self.platform.as_ref().as_ref()
    }

    pub fn audio(&self) -> RwLockWriteGuard<'_, Audio> {
        self.audio.as_ref().write()
    }

    pub fn event_queue(&self) -> RwLockWriteGuard<'_, EventQueue> {
        self.event_queue.write()
    }

    pub fn current_input_mode(&self) -> u32 {
        self.input_method.read().current_mode()
    }

    pub fn set_current_input_mode(&self, mode: u32) {
        self.input_method.write().set_current_mode(mode);
    }

    pub fn input_composition_size(&self) -> usize {
        self.input_method.read().composition_size()
    }

    pub fn set_input_composition_size(&self, size: usize) {
        self.input_method.write().set_composition_size(size);
    }

    /// Feeds a keypress to the handset's input method.
    ///
    /// The guest clock goes with it: multi-tap finishes a character when the
    /// same key is left alone long enough, and measuring that on guest time
    /// rather than the host's keeps a frontend that runs ticks in batches
    /// typing the same text as one running live.
    pub fn handle_input_method(&self, key: i8, event: u32) -> InputMethodOutput {
        let now = self.platform().now();

        self.input_method.write().handle_input(key, event, now)
    }
}
