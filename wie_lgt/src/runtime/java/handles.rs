//! Guest-visible handles for JVM objects.
//!
//! An ahead-of-time compiled LGT application passes platform objects around as
//! single words. The objects themselves live on the Rust side, so each one is
//! given a small guest allocation whose address is the handle, and the
//! instance is retained here under that address.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

use jvm::ClassInstance;
use spin::Mutex;

use wie_core_arm::{Allocator, ArmCore};
use wie_util::{ByteRead, ByteWrite, Result, WieError, read_generic, write_generic};

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

/// The guest global-data region (`ArmCore` maps 16 KiB here). Scanned as GC
/// roots so an object referenced only from a global is not treated as garbage.
const GUEST_GLOBAL_DATA_BASE: u32 = 0x7fff0000;
const GUEST_GLOBAL_DATA_SIZE: u32 = 0x4000;

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
    /// Element descriptor byte (`b'C'`, `b'B'`, ...) for each array the compiled
    /// code allocated itself. A compiled array has no JVM instance to name its
    /// type, so a method that takes it as `Object` (`System.arraycopy`) needs
    /// this to wrap it as the right JVM array rather than guessing.
    array_element_types: Arc<Mutex<BTreeMap<u32, u8>>>,
    /// Every guest object/array this hands out, for the garbage collector. The
    /// key is the instance handle; the value is the block it points to and its
    /// size. Raw allocations (thread stacks, save points, firmware structures)
    /// never appear here, so the collector can only ever reclaim real objects.
    gc_objects: Arc<Mutex<BTreeMap<u32, GcObject>>>,
    /// Guest memory ranges that hold GC roots outside the thread stacks -
    /// notably each activated class's static-field block. Scanned alongside the
    /// registers and stacks so an object referenced only from a static field is
    /// not mistaken for garbage.
    gc_static_roots: Arc<Mutex<Vec<(u32, u32)>>>,
}

/// One collector-managed guest object: its `+0x08` payload block (fields for an
/// instance, `[length, elements]` for an array) and the byte size of that block.
#[derive(Clone, Copy)]
pub struct GcObject {
    pub payload: u32,
    pub payload_size: u32,
    pub is_array: bool,
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
            array_element_types: Default::default(),
            gc_objects: Default::default(),
            gc_static_roots: Default::default(),
        }
    }

    /// Registers a guest memory range `[start, end)` whose words the collector
    /// must treat as GC roots (a class's static-field block).
    pub fn register_gc_static_root(&self, start: u32, end: u32) {
        if end > start {
            self.gc_static_roots.lock().push((start, end));
        }
    }

    /// Records the element type of a compiled-code-allocated array, so a later
    /// call taking it as `Object` can wrap it as the right JVM array.
    pub fn record_array_element_type(&self, handle: u32, element: u8) {
        self.array_element_types.lock().insert(handle, element);
    }

    /// The element descriptor byte of a compiled array, if one was recorded.
    pub fn array_element_type(&self, handle: u32) -> Option<u8> {
        self.array_element_types.lock().get(&handle).copied()
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

    pub fn dispatch_table(&self, class: &str) -> Option<u32> {
        self.dispatch_tables.lock().get(class).copied()
    }

    /// Records the table to give instances of anything else.
    pub fn set_fallback_dispatch_table(&self, vtable: u32) {
        self.fallback_dispatch_table.store(vtable, Ordering::SeqCst);
    }

    /// Allocates an instance the compiled code can use: a header pointing at
    /// its own field array. This compatibility path keeps the global slot count
    /// used for JVM-originated objects whose exact native class layout is not
    /// available at the insertion site.
    pub fn allocate_instance(&self, vtable: u32) -> Result<u32> {
        let slots = self.field_slots.load(Ordering::SeqCst);
        self.allocate_instance_with_fields(vtable, slots)
    }

    /// Allocates one native VM object using that class's exact four-byte
    /// instance-field word count.
    pub fn allocate_instance_with_fields(&self, vtable: u32, slots: u32) -> Result<u32> {
        let mut core = self.core.clone();

        // Native vm_gc_calloc(4, 0) still has to leave the object with a valid
        // +0x08 pointer for guest code. Reserve one word while exposing zero
        // logical field slots.
        let fields = self.alloc_reporting_leaks(&mut core, slots.max(1) * 4)?;
        for slot in 0..slots {
            write_generic(&mut core, fields + slot * 4, 0u32)?;
        }

        let instance = Allocator::alloc(&mut core, INSTANCE_HEADER_SIZE)?;
        write_generic(&mut core, instance, vtable)?;
        write_generic(&mut core, instance + 4, 0u32)?;
        write_generic(&mut core, instance + INSTANCE_FIELDS_OFFSET, fields)?;

        self.gc_objects.lock().insert(
            instance,
            GcObject {
                payload: fields,
                payload_size: slots.max(1) * 4,
                is_array: false,
            },
        );

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
        let data = self.alloc_reporting_leaks(&mut core, ARRAY_HEADER_SIZE + size)?;

        write_generic(&mut core, data, length)?;
        core.write_bytes(data + ARRAY_HEADER_SIZE, &vec![0; size as usize])?;

        let instance = Allocator::alloc(&mut core, INSTANCE_HEADER_SIZE)?;
        write_generic(&mut core, instance, vtable)?;
        write_generic(&mut core, instance + 4, 0u32)?;
        write_generic(&mut core, instance + INSTANCE_FIELDS_OFFSET, data)?;

        self.gc_objects.lock().insert(
            instance,
            GcObject {
                payload: data,
                payload_size: ARRAY_HEADER_SIZE + size,
                is_array: true,
            },
        );

        Ok(instance)
    }

    /// Allocates a guest block; if the heap is exhausted, runs the collector to
    /// reclaim unreachable objects and retries once before giving up.
    fn alloc_reporting_leaks(&self, core: &mut ArmCore, size: u32) -> Result<u32> {
        match Allocator::alloc(core, size) {
            Ok(address) => Ok(address),
            Err(WieError::AllocationFailure) => {
                // Report only. A live sweep here is NOT yet safe: objects held
                // transiently on the JVM side (mid-construction, or inside a JVM
                // collection) are reachable from neither the guest registers,
                // stacks, statics nor globals this collector scans, so freeing
                // "unreachable" objects can reclaim a live one - a headless run
                // with an aggressive sweep crashed Lm.startApp on exactly that.
                // Enabling `gc_collect` needs a JVM-reachability root source.
                self.gc_report();
                Err(WieError::AllocationFailure)
            }
            Err(error) => Err(error),
        }
    }

    /// Candidate GC root words: every thread's registers, the in-use span of
    /// every stack, each registered static-field block, and the small guest
    /// global-data region. Any of these words that equals a managed handle is a
    /// live reference.
    fn gc_seeds(&self) -> Vec<u32> {
        let (registers, stack_ranges) = self.core.gc_thread_roots();
        let core = self.core.clone();

        let mut seeds: Vec<u32> = registers;
        let static_roots = self.gc_static_roots.lock().clone();
        let global_data = (GUEST_GLOBAL_DATA_BASE, GUEST_GLOBAL_DATA_BASE + GUEST_GLOBAL_DATA_SIZE);

        for &(low, high) in stack_ranges.iter().chain(static_roots.iter()).chain(core::iter::once(&global_data)) {
            let mut address = low & !3;
            while address < high {
                if let Ok(word) = read_generic::<u32, _>(&core, address) {
                    seeds.push(word);
                }
                address += 4;
            }
        }

        seeds
    }

    /// Reports what the collector would reclaim, without freeing anything.
    ///
    /// Marks conservatively from [`Self::gc_seeds`] and logs how many managed
    /// objects, and how many bytes, are unreachable. Safe to run at any time;
    /// used on allocation failure so a crash log carries the collector's view.
    fn gc_report(&self) {
        let objects = self.gc_objects.lock();
        if objects.is_empty() {
            return;
        }

        let seeds = self.gc_seeds();
        let marked = self.gc_reachable(&objects, &seeds);

        let mut dead = 0u32;
        let mut dead_arrays = 0u32;
        let mut dead_bytes: u64 = 0;
        for (handle, object) in objects.iter() {
            if !marked.contains(handle) {
                dead += 1;
                dead_arrays += u32::from(object.is_array);
                dead_bytes += u64::from(object.payload_size + INSTANCE_HEADER_SIZE);
            }
        }

        tracing::error!(
            "GC report: {} managed objects, {} reachable, {} unreachable ({} arrays, ~{} bytes)",
            objects.len(),
            marked.len(),
            dead,
            dead_arrays,
            dead_bytes,
        );
    }

    /// Reclaims every managed object unreachable from the live guest state.
    ///
    /// Marks conservatively from [`Self::gc_seeds`], then frees the guest
    /// allocations (instance header and payload block) of every unmarked object
    /// and drops its bridge bookkeeping, so a surviving JVM instance is
    /// re-materialized with a fresh handle if the compiled code ever needs it
    /// again. Only allocations handed out as objects appear in the registry, so
    /// thread stacks, save points and firmware structures are never touched.
    /// Returns the count and byte total reclaimed.
    ///
    /// NOT YET SAFE TO CALL in normal operation: objects held transiently on
    /// the JVM side (mid-construction, or inside a JVM collection) are reachable
    /// from none of the roots [`Self::gc_seeds`] scans, so this can free a live
    /// one. Enabling it needs a JVM-reachability root source; kept (and tested)
    /// as the basis for that work.
    #[allow(dead_code)]
    pub fn gc_collect(&self) -> (u32, u64) {
        let seeds = self.gc_seeds();
        let mut core = self.core.clone();

        let mut objects = self.gc_objects.lock();
        if objects.is_empty() {
            return (0, 0);
        }

        let marked = self.gc_reachable(&objects, &seeds);
        let dead: Vec<u32> = objects.keys().copied().filter(|handle| !marked.contains(handle)).collect();

        let mut freed = 0u32;
        let mut freed_bytes: u64 = 0;

        for handle in dead {
            let Some(object) = objects.remove(&handle) else {
                continue;
            };

            // Release the JVM instance held under this handle (dropping our
            // reference so RustJava can collect it) and forget its identity, so
            // a later crossing allocates a fresh handle rather than this freed
            // address.
            if let Some(instance) = self.entries.lock().remove(&handle) {
                self.addresses.lock().remove(&instance.identity());
            }
            self.array_element_types.lock().remove(&handle);

            let _ = Allocator::free(&mut core, object.payload, object.payload_size);
            let _ = Allocator::free(&mut core, handle, INSTANCE_HEADER_SIZE);

            freed += 1;
            freed_bytes += u64::from(object.payload_size + INSTANCE_HEADER_SIZE);
        }

        (freed, freed_bytes)
    }

    /// The managed objects reachable from `seeds` (candidate root words),
    /// following each object's payload words as further candidate references.
    /// Conservative: any word equal to a managed handle is treated as a live
    /// reference, so the result never under-approximates the live set.
    fn gc_reachable(&self, objects: &BTreeMap<u32, GcObject>, seeds: &[u32]) -> BTreeSet<u32> {
        let core = self.core.clone();
        let mut marked: BTreeSet<u32> = BTreeSet::new();
        let mut worklist: Vec<u32> = Vec::new();

        for &word in seeds {
            if objects.contains_key(&word) && marked.insert(word) {
                worklist.push(word);
            }
        }

        while let Some(handle) = worklist.pop() {
            let object = objects[&handle];
            let mut offset = 0;
            while offset < object.payload_size {
                if let Ok(word) = read_generic::<u32, _>(&core, object.payload + offset)
                    && objects.contains_key(&word)
                    && marked.insert(word)
                {
                    worklist.push(word);
                }
                offset += 4;
            }
        }

        marked
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

    /// Copies a guest-side `char[]` into host memory.
    ///
    /// Same header as any array, but each element is a little-endian 16-bit
    /// unit, so the block spans `count * 2` bytes rather than `count`. Used for
    /// imported methods whose descriptor says `[C` (e.g. `String.<init>([C)`),
    /// where reading the elements as bytes would keep only their low halves and
    /// hand the JVM a `char[]` half the intended length.
    pub fn read_char_array(&self, handle: u32) -> Result<Vec<u16>> {
        let core = self.core.clone();

        let data: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;
        let length: u32 = read_generic(&core, data)?;

        let mut bytes = vec![0u8; length as usize * 2];
        core.read_bytes(data + ARRAY_HEADER_SIZE, &mut bytes)?;

        Ok(bytes.chunks_exact(2).map(|unit| u16::from_le_bytes([unit[0], unit[1]])).collect())
    }

    /// Copies JVM char-array contents back into a guest-side `char[]`.
    pub fn write_char_array(&self, handle: u32, chars: &[u16]) -> Result<()> {
        let mut core = self.core.clone();

        let data: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET)?;
        let length: u32 = read_generic(&core, data)?;
        let count = chars.len().min(length as usize);

        let mut bytes = Vec::with_capacity(count * 2);
        for unit in &chars[..count] {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }

        core.write_bytes(data + ARRAY_HEADER_SIZE, &bytes)?;

        Ok(())
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

    /// Gives an array handle a `[length][elements]` block holding `elements`,
    /// so the compiled code can read its contents directly.
    ///
    /// A handle built by [`Self::insert`] carries only the generic field block
    /// [`Self::allocate_instance`] leaves at +0x08; an array returned from a
    /// platform method (`String.toCharArray`, say) whose elements the compiled
    /// code then reads itself needs the same `+0x00 length, +0x04 elements`
    /// block a [`Self::allocate_array`] array has. The JVM object stays the
    /// backing store for platform calls; this only mirrors it into guest memory.
    pub fn materialize_array_block(&self, handle: u32, length: u32, elements: &[u8]) -> Result<()> {
        let mut core = self.core.clone();

        let data = Allocator::alloc(&mut core, ARRAY_HEADER_SIZE + elements.len() as u32)?;
        write_generic(&mut core, data, length)?;
        core.write_bytes(data + ARRAY_HEADER_SIZE, elements)?;
        write_generic(&mut core, handle + INSTANCE_FIELDS_OFFSET, data)?;

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

#[cfg(test)]
mod tests {
    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::read_generic;

    use super::{INSTANCE_FIELDS_OFFSET, JavaHandles};

    #[test]
    fn materialized_array_block_reads_back_as_a_char_array() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();

        let handles = JavaHandles::new(core.clone());

        // A handle whose +0x08 block is the generic three-word field block an
        // instance built by `insert` gets: reading it as a char array sees a
        // zero-length array, exactly the empty string the word-wrapper crashed on.
        let handle = handles.allocate_instance(0).unwrap();
        assert!(handles.read_char_array(handle).unwrap().is_empty());

        // Mirroring the JVM contents into the handle gives it a real
        // `[length][elements]` block the compiled code can read directly.
        let chars = [0xac00u16, 0x0041, 0xd55c];
        let bytes: alloc::vec::Vec<u8> = chars.iter().flat_map(|value| value.to_le_bytes()).collect();
        handles.materialize_array_block(handle, chars.len() as u32, &bytes).unwrap();

        assert_eq!(handles.read_char_array(handle).unwrap(), chars);
    }

    #[test]
    fn gc_reachable_follows_references_and_drops_unreferenced_objects() {
        use wie_util::write_generic;

        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        let handles = JavaHandles::new(core.clone());

        // Three objects; make `a` reference `b` by writing b's handle into a's
        // field block. `c` is referenced by nobody.
        let a = handles.allocate_instance_with_fields(0, 4).unwrap();
        let b = handles.allocate_instance_with_fields(0, 4).unwrap();
        let c = handles.allocate_instance_with_fields(0, 4).unwrap();

        let a_fields: u32 = read_generic(&core, a + INSTANCE_FIELDS_OFFSET).unwrap();
        write_generic(&mut core, a_fields, b).unwrap();

        let objects = handles.gc_objects.lock();

        // Rooting `a` keeps a and b; c is unreachable.
        let reachable = handles.gc_reachable(&objects, &[a]);
        assert!(reachable.contains(&a));
        assert!(reachable.contains(&b));
        assert!(!reachable.contains(&c));

        // With no roots, nothing is reachable.
        assert!(handles.gc_reachable(&objects, &[]).is_empty());
    }

    #[test]
    fn gc_collect_frees_unreachable_objects_and_returns_the_memory() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        let handles = JavaHandles::new(core.clone());

        let _a = handles.allocate_instance_with_fields(0, 8).unwrap();
        let _b = handles.allocate_array(0, 64, 4).unwrap();
        assert_eq!(handles.gc_objects.lock().len(), 2);

        // No live threads means no roots, so both objects are unreachable and
        // the collector reclaims them and clears its registry.
        let (freed, freed_bytes) = handles.gc_collect();
        assert_eq!(freed, 2);
        assert!(freed_bytes > 0);
        assert!(handles.gc_objects.lock().is_empty());

        // The freed memory is usable again.
        let c = handles.allocate_instance_with_fields(0, 8).unwrap();
        assert_ne!(c, 0);
        assert_eq!(handles.gc_objects.lock().len(), 1);
    }
}
