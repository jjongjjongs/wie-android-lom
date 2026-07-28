//! Guest-visible handles for JVM objects.
//!
//! An ahead-of-time compiled LGT application passes platform objects around as
//! single words. The objects themselves live on the Rust side, so each one is
//! given a small guest allocation whose address is the handle, and the
//! instance is retained here under that address.

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};

use jvm::ClassInstance;
use spin::Mutex;

use wie_core_arm::{Allocator, ArmCore};
use wie_util::{Result, write_generic};

/// Size of the guest-side stand-in. The original VM uses a twelve byte native
/// object header, and compiled code copies handles around by that size.
const HANDLE_SIZE: u32 = 12;

#[derive(Clone)]
pub struct JavaHandles {
    core: ArmCore,
    entries: Arc<Mutex<BTreeMap<u32, Box<dyn ClassInstance>>>>,
}

impl JavaHandles {
    pub fn new(core: ArmCore) -> Self {
        Self {
            core,
            entries: Default::default(),
        }
    }

    /// Allocates a handle for `instance` and retains it.
    pub fn insert(&self, instance: Box<dyn ClassInstance>) -> Result<u32> {
        let mut core = self.core.clone();

        let handle = Allocator::alloc(&mut core, HANDLE_SIZE)?;
        write_generic(&mut core, handle, 0u32)?;
        write_generic(&mut core, handle + 4, 0u32)?;
        write_generic(&mut core, handle + 8, 0xffff_ffffu32)?;

        self.entries.lock().insert(handle, instance);

        Ok(handle)
    }

    /// Retains `instance` under an address the guest already owns.
    ///
    /// The compiled code allocates an object, prepares it, then calls the
    /// constructor on it, so the JVM instance only exists once the guest
    /// address does.
    pub fn bind(&self, handle: u32, instance: Box<dyn ClassInstance>) {
        self.entries.lock().insert(handle, instance);
    }

    /// Instances are reference counted internally, so the clone shares state
    /// with the retained object rather than copying it.
    pub fn get(&self, handle: u32) -> Option<Box<dyn ClassInstance>> {
        self.entries.lock().get(&handle).cloned()
    }
}
