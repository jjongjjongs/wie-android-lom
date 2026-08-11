//! Guest-visible handles for JVM objects.
//!
//! An ahead-of-time compiled LGT application passes platform objects around as
//! single words. The objects themselves live on the Rust side, so each one is
//! given a small guest allocation whose address is the handle, and the
//! instance is retained here under that address.

use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use jvm::ClassInstance;
use spin::Mutex;

use wie_core_arm::{Allocator, ArmCore};
use wie_util::{ByteRead, ByteWrite, Result, read_generic, write_generic};

/// Instance header the compiled code relies on:
///
/// ```text
/// +0x00 dispatch table
/// +0x04 unused so far
/// +0x08 word array holding the fields
/// ```
const INSTANCE_HEADER_SIZE: u32 = 12;
const INSTANCE_FIELDS_OFFSET: u32 = 8;

/// Words an array's block spends on its length before the elements start.
const ARRAY_HEADER_SIZE: u32 = 4;

#[derive(Clone, Debug)]
pub struct JavaFieldBinding {
    pub class_name: String,
    pub name: String,
    pub descriptor: String,
    pub slot: u32,
}

#[derive(Clone)]
pub struct JavaHandles {
    core: ArmCore,
    /// Words every instance's field array holds, set once the class table is
    /// known.
    field_slots: Arc<AtomicU32>,
    /// Imported platform fields whose native ABI slot must stay coherent with
    /// the JVM instance retained under the same handle.
    field_bindings: Arc<Mutex<Vec<JavaFieldBinding>>>,
    /// Class name to dispatch table, so an object handed to the compiled code
    /// can be given the table its virtual calls will go through.
    dispatch_tables: Arc<Mutex<BTreeMap<String, u32>>>,
    /// Table for a class the application never declared, so a call on one of
    /// its objects is reported rather than branching to zero.
    fallback_dispatch_table: Arc<AtomicU32>,
    entries: Arc<Mutex<BTreeMap<u32, Box<dyn ClassInstance>>>>,
    /// Instance identity to handle, so a value coming back from the JVM can be
    /// handed to the compiled code as the address it already knows.
    addresses: Arc<Mutex<BTreeMap<usize, u32>>>,
}

impl JavaHandles {
    pub fn new(core: ArmCore) -> Self {
        Self {
            core,
            field_slots: Arc::new(AtomicU32::new(0)),
            field_bindings: Default::default(),
            dispatch_tables: Default::default(),
            fallback_dispatch_table: Arc::new(AtomicU32::new(0)),
            entries: Default::default(),
            addresses: Default::default(),
        }
    }

    /// Records how many words an instance's field array needs.
    pub fn set_field_slots(&self, slots: u32) {
        self.field_slots.store(slots, Ordering::SeqCst);
    }

    /// Records imported platform fields that have native guest ABI slots.
    pub fn set_field_bindings(&self, bindings: Vec<JavaFieldBinding>) {
        *self.field_bindings.lock() = bindings;
    }

    pub fn field_bindings(&self) -> Vec<JavaFieldBinding> {
        self.field_bindings.lock().clone()
    }

    /// Reads one word from an instance's guest-side field block.
    pub fn read_field_word(&self, handle: u32, slot: u32) -> Result<u32> {
        let core = self.core.clone();
        let fields: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;

        read_generic(&core, fields + slot * 4)
    }

    /// Writes one word to an instance's guest-side field block.
    pub fn write_field_word(&self, handle: u32, slot: u32, value: u32) -> Result<()> {
        let mut core = self.core.clone();
        let fields: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;

        write_generic(&mut core, fields + slot * 4, value)
    }

    /// Records the dispatch table to give instances of `class`.
    pub fn set_dispatch_table(&self, class: &str, vtable: u32) {
        self.dispatch_tables.lock().insert(class.into(), vtable);
    }

    /// Records the table to give instances of anything else.
    pub fn set_fallback_dispatch_table(&self, vtable: u32) {
        self.fallback_dispatch_table.store(vtable, Ordering::SeqCst);
    }

    /// Allocates an instance the compiled code can use: a header pointing at
    /// its own field array.
    pub fn allocate_instance(&self, vtable: u32) -> Result<u32> {
        let mut core = self.core.clone();

        let slots = self.field_slots.load(Ordering::SeqCst);
        let fields = Allocator::alloc(&mut core, slots.max(1) * 4)?;
        for slot in 0..slots {
            write_generic(&mut core, fields + slot * 4, 0u32)?;
        }

        let instance = Allocator::alloc(&mut core, INSTANCE_HEADER_SIZE)?;
        write_generic(&mut core, instance, vtable)?;
        write_generic(&mut core, instance + 4, 0u32)?;
        write_generic(&mut core, instance + INSTANCE_FIELDS_OFFSET, fields)?;

        Ok(instance)
    }

    /// Allocates an array the compiled code can use.
    ///
    /// An array has the same header as any other instance, and what `+0x08`
    /// points at is:
    ///
    /// ```text
    /// +0x00 element count
    /// +0x04 elements
    /// ```
    ///
    /// The count is read straight off that block for every bounds check the
    /// compiled code makes - `ldr r3, [r0]; cmp index, r3` - so an array whose
    /// block does not start with its length reads as empty and every access
    /// throws.
    pub fn allocate_array(&self, vtable: u32, length: u32, element_size: u32) -> Result<u32> {
        let mut core = self.core.clone();

        let size = length * element_size;
        let data = Allocator::alloc(&mut core, ARRAY_HEADER_SIZE + size)?;

        write_generic(&mut core, data, length)?;
        core.write_bytes(data + ARRAY_HEADER_SIZE, &vec![0; size as usize])?;

        let instance = Allocator::alloc(&mut core, INSTANCE_HEADER_SIZE)?;
        write_generic(&mut core, instance, vtable)?;
        write_generic(&mut core, instance + 4, 0u32)?;
        write_generic(&mut core, instance + INSTANCE_FIELDS_OFFSET, data)?;

        Ok(instance)
    }

    /// Copies a guest-side byte array into host memory.
    ///
    /// LGT arrays are ordinary guest instances whose +0x08 word points to:
    ///
    /// ```text
    /// +0x00 element count
    /// +0x04 elements
    /// ```
    ///
    /// This currently treats the first `length` bytes as byte-array data.
    /// It is intended for imported methods whose descriptor explicitly says
    /// `[B`, so the descriptor supplies the element type the handle lacks.
    pub fn read_byte_array(&self, handle: u32) -> Result<Vec<i8>> {
        let core = self.core.clone();

        let data: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;
        let length: u32 = read_generic(&core, data)?;

        let mut bytes = vec![0u8; length as usize];
        core.read_bytes(data + ARRAY_HEADER_SIZE, &mut bytes)?;

        Ok(bytes.into_iter().map(|value| value as i8).collect())
    }

    /// Copies JVM byte-array contents back into a guest-side byte array.
    pub fn write_byte_array(&self, handle: u32, bytes: &[i8]) -> Result<()> {
        let mut core = self.core.clone();

        let data: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;
        let length: u32 = read_generic(&core, data)?;
        let count = bytes.len().min(length as usize);

        let bytes: Vec<u8> = bytes[..count].iter().map(|value| *value as u8).collect();

        core.write_bytes(data + ARRAY_HEADER_SIZE, &bytes)?;

        Ok(())
    }

    /// The table to give an object whose class declares none.
    pub fn fallback_dispatch_table(&self) -> u32 {
        self.fallback_dispatch_table.load(Ordering::SeqCst)
    }

    /// Allocates a handle for `instance` and retains it.
    pub fn insert(&self, instance: Box<dyn ClassInstance>) -> Result<u32> {
        // An object crossing to the compiled code has its virtual calls
        // dispatched through its own class's table, so it has to carry one.
        let class = instance.class_definition().name();
        let declared = self.dispatch_tables.lock().get(&class).copied();

        let vtable = match declared {
            Some(vtable) => vtable,
            None => {
                tracing::debug!("{class} declares no dispatch table; using the fallback");

                self.fallback_dispatch_table.load(Ordering::SeqCst)
            }
        };

        let handle = self.allocate_instance(vtable)?;

        self.addresses.lock().insert(instance.identity(), handle);
        self.entries.lock().insert(handle, instance);

        Ok(handle)
    }

    /// Retains `instance` under an address the guest already owns.
    ///
    /// The compiled code allocates an object, prepares it, then calls the
    /// constructor on it, so the JVM instance only exists once the guest
    /// address does.
    pub fn bind(&self, handle: u32, instance: Box<dyn ClassInstance>) {
        self.addresses.lock().insert(instance.identity(), handle);
        self.entries.lock().insert(handle, instance);
    }

    /// The address the compiled code knows `instance` by, allocating one if it
    /// has not crossed the boundary before.
    pub fn address_of(&self, instance: Box<dyn ClassInstance>) -> Result<u32> {
        if let Some(handle) = self.addresses.lock().get(&instance.identity()).copied() {
            return Ok(handle);
        }

        self.insert(instance)
    }

    /// Instances are reference counted internally, so the clone shares state
    /// with the retained object rather than copying it.
    pub fn get(&self, handle: u32) -> Option<Box<dyn ClassInstance>> {
        self.entries.lock().get(&handle).cloned()
    }
}
