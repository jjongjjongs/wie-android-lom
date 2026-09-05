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

use jvm::{ClassInstance, Jvm};
use spin::Mutex;

use wie_core_arm::{Allocator, ArmCore};
use wie_util::{ByteRead, ByteWrite, Result, WieError, read_generic, write_generic};

use crate::runtime::savepoint::SavePointState;

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
    /// Per concrete class, the subset of `field_bindings` that applies to it
    /// (its own and inherited imported fields), resolved once and reused. The
    /// field sync that brackets every bridged method call would otherwise
    /// re-scan the whole binding list — an `is_instance` hierarchy walk per
    /// binding, twice a call — which on a title that calls into the JVM
    /// thousands of times a second is the dominant bridge cost.
    applied_field_bindings: Arc<Mutex<BTreeMap<String, Arc<Vec<JavaFieldBinding>>>>>,
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
    /// The JVM whose own reachability pins objects the guest roots miss. An
    /// object can be dead to every guest root while the JVM still holds it
    /// (mid-construction, or inside a JVM-side collection); freeing it would
    /// corrupt live state, so the sweep never reclaims a JVM-reachable object.
    /// Set once the JVM exists; until then the sweep stays disabled.
    jvm: Arc<Mutex<Option<Jvm>>>,
    /// Instance identities explicitly pinned across a bridge crossing. While the
    /// compiled code is inside a bridged JVM method, that method's receiver and
    /// object arguments live only as Rust-stack values (or in a suspended
    /// future) - reachable from neither the guest roots nor a JVM thread frame
    /// the collector scans. The sweep would free one and hand the guest a freed
    /// address (observed: a live String reclaimed mid-`StringBuffer.<init>`,
    /// then reused for a byte array, so `value.toCharArray()` hit `[B`). Each
    /// crossing pins its objects here for its duration. Identity to nesting
    /// count, since the same object can be pinned by nested crossings.
    bridge_pins: Arc<Mutex<BTreeMap<usize, u32>>>,
    /// The live save points, whose jmp_buf blocks and captured register
    /// contexts are GC roots: a longjmp restores those registers, so an object
    /// named only there is still reachable. Set once the state exists.
    save_points: Arc<Mutex<Option<SavePointState>>>,
}

/// Keeps a set of instance identities pinned against the collector for as long
/// as it is held, then releases them - so a bridge crossing can protect its
/// receiver and arguments for exactly the duration of the call, across `await`s.
#[must_use = "the pin is released as soon as the guard is dropped"]
pub struct BridgePin {
    pins: Arc<Mutex<BTreeMap<usize, u32>>>,
    identities: Vec<usize>,
}

impl Drop for BridgePin {
    fn drop(&mut self) {
        let mut pins = self.pins.lock();
        for identity in &self.identities {
            if let Some(count) = pins.get_mut(identity) {
                *count -= 1;
                if *count == 0 {
                    pins.remove(identity);
                }
            }
        }
    }
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
            applied_field_bindings: Default::default(),
            dispatch_tables: Default::default(),
            fallback_dispatch_table: Arc::new(AtomicU32::new(0)),
            entries: Default::default(),
            addresses: Default::default(),
            array_element_types: Default::default(),
            gc_objects: Default::default(),
            gc_static_roots: Default::default(),
            jvm: Default::default(),
            bridge_pins: Default::default(),
            save_points: Default::default(),
        }
    }

    /// Records the save-point state whose jmp_buf blocks and captured register
    /// contexts the collector scans as roots.
    pub fn set_save_points(&self, save_points: SavePointState) {
        *self.save_points.lock() = Some(save_points);
    }

    /// Records the JVM whose reachability pins live objects during a sweep.
    /// Until this is set, [`Self::gc_collect`] reclaims nothing.
    pub fn set_jvm(&self, jvm: Jvm) {
        *self.jvm.lock() = Some(jvm);
    }

    /// Pins the given instance identities against the collector until the
    /// returned guard is dropped. A bridge crossing pins its receiver and object
    /// arguments so the sweep cannot reclaim them while they live only on the
    /// Rust call stack (or in a suspended future), invisible to the guest and
    /// JVM roots the collector scans.
    pub fn pin_identities(&self, identities: Vec<usize>) -> BridgePin {
        {
            let mut pins = self.bridge_pins.lock();
            for &identity in &identities {
                *pins.entry(identity).or_insert(0) += 1;
            }
        }
        BridgePin {
            pins: self.bridge_pins.clone(),
            identities,
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
        self.applied_field_bindings.lock().clear();
    }

    /// The field bindings that apply to `instance`'s concrete class — its own
    /// and inherited imported fields — resolved on first use and cached by class
    /// name. Replaces a full-list `is_instance` scan on every bridged call with
    /// a single map lookup. `is_instance` is synchronous, so the one-time scan
    /// holds the binding lock without crossing an `await`.
    pub fn applied_field_bindings(&self, jvm: &Jvm, instance: &dyn ClassInstance) -> Arc<Vec<JavaFieldBinding>> {
        let class_name = instance.class_definition().name();

        if let Some(cached) = self.applied_field_bindings.lock().get(&class_name) {
            return cached.clone();
        }

        let applied = Arc::new(
            self.field_bindings
                .lock()
                .iter()
                .filter(|binding| jvm.is_instance(instance, &binding.class_name))
                .cloned()
                .collect::<Vec<_>>(),
        );
        self.applied_field_bindings.lock().insert(class_name, applied.clone());
        applied
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
                // Heap exhausted: reclaim objects dead to both the guest roots
                // and the JVM's own graph, then retry once. The JVM pin keeps
                // the sweep from freeing an object the JVM still holds (see
                // `gc_collect`); if the JVM is not yet registered the sweep is a
                // no-op and this still surfaces the failure.
                let (freed, freed_bytes) = self.gc_collect();
                if freed > 0 {
                    tracing::info!("GC reclaimed {freed} objects ({freed_bytes} bytes) on allocation failure; retrying");
                    if let Ok(address) = Allocator::alloc(core, size) {
                        return Ok(address);
                    }
                }
                // Still no room (or nothing to reclaim): log the collector's
                // view to make a leak visible in the crash log.
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

        let (save_point_registers, save_point_ranges) = match self.save_points.lock().as_ref() {
            Some(save_points) => save_points.gc_roots(),
            None => (Vec::new(), Vec::new()),
        };
        seeds.extend_from_slice(&save_point_registers);

        for &(low, high) in stack_ranges
            .iter()
            .chain(static_roots.iter())
            .chain(save_point_ranges.iter())
            .chain(core::iter::once(&global_data))
        {
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

    /// Reclaims every managed object unreachable from **both** the live guest
    /// state and the JVM's own object graph.
    ///
    /// Marks conservatively from [`Self::gc_seeds`], additionally pins every
    /// object the JVM still reaches (via [`Jvm::gc_reachable_identities`]), then
    /// frees the guest allocations (instance header and payload block) of every
    /// object that survives neither and drops its bridge bookkeeping, so a
    /// surviving JVM instance is re-materialized with a fresh handle if the
    /// compiled code ever needs it again. Only allocations handed out as objects
    /// appear in the registry, so thread stacks, save points and firmware
    /// structures are never touched. Returns the count and byte total reclaimed.
    ///
    /// Two pins make this safe. The JVM pin: an object can be dead to every
    /// guest root while the JVM holds it transiently (mid-construction, or inside
    /// a JVM-side collection), reachable from none of the roots
    /// [`Self::gc_seeds`] scans. The bridge pin ([`Self::pin_identities`]): an
    /// object passed across a bridge crossing lives only as a Rust-stack value
    /// for the call's duration, reachable from neither the guest nor a scanned
    /// JVM frame. Freeing either corrupts live state - a guest-only sweep crashed
    /// `Lm.startApp`, and a JVM-pinned-only sweep freed a `StringBuffer.<init>`
    /// String argument mid-call. Until the JVM is registered with
    /// [`Self::set_jvm`], this reclaims nothing.
    pub fn gc_collect(&self) -> (u32, u64) {
        let Some(jvm) = self.jvm.lock().clone() else {
            // No JVM-reachability source yet: a guest-only sweep is unsafe, so
            // reclaim nothing rather than risk freeing a JVM-held object.
            return (0, 0);
        };

        // Pin objects the JVM still reaches. Gathered before taking the object
        // lock so a JVM read lock is never held across our heap mutation.
        let jvm_pinned: BTreeSet<usize> = jvm.gc_reachable_identities().into_iter().collect();
        self.gc_collect_with_pins(&jvm_pinned)
    }

    /// The sweep itself: frees every managed object reachable from neither the
    /// guest roots nor `jvm_pinned` (JVM instance identities to keep). Split out
    /// so the mechanics can be exercised without standing up a JVM.
    fn gc_collect_with_pins(&self, jvm_pinned: &BTreeSet<usize>) -> (u32, u64) {
        let seeds = self.gc_seeds();
        let mut core = self.core.clone();

        let mut objects = self.gc_objects.lock();
        if objects.is_empty() {
            return (0, 0);
        }

        let marked = self.gc_reachable(&objects, &seeds);
        let bridge_pins = self.bridge_pins.lock();
        let entries = self.entries.lock();
        let dead: Vec<u32> = objects
            .keys()
            .copied()
            .filter(|handle| {
                if marked.contains(handle) {
                    return false;
                }
                // Guest-unreachable, but keep it if the JVM still holds it or a
                // bridge crossing has pinned it (its Rust-stack argument).
                match entries.get(handle) {
                    Some(instance) => {
                        let identity = instance.identity();
                        !jvm_pinned.contains(&identity) && !bridge_pins.contains_key(&identity)
                    }
                    None => true,
                }
            })
            .collect();
        drop(entries);
        drop(bridge_pins);

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

        let size = ARRAY_HEADER_SIZE + elements.len() as u32;

        // Re-mirroring an array of the same length writes over the block it
        // already has, so a pointer the compiled code kept into the elements
        // stays good.
        let reusable = self
            .gc_objects
            .lock()
            .get(&handle)
            .copied()
            .filter(|object| object.is_array && object.payload_size == size)
            .map(|object| object.payload);

        let data = match reusable {
            Some(data) => data,
            None => {
                let data = self.alloc_reporting_leaks(&mut core, size)?;

                // Swap the array block in for whatever the object pointed at -
                // for a handle from `insert` that is the generic field block,
                // one word per platform field row, a couple of kilobytes on a
                // title with a wide import table. Leaving it in place orphaned
                // it on every crossing: the collector went on scanning and
                // freeing the field block while the array block was never
                // reclaimed at all, so a title that mirrors a `char[]` per
                // drawn string ate the heap in minutes. Read back under the
                // same lock as the insert, since the allocation above can run a
                // sweep that changes what this handle points at.
                let previous = self.gc_objects.lock().insert(
                    handle,
                    GcObject {
                        payload: data,
                        payload_size: size,
                        is_array: true,
                    },
                );
                if let Some(previous) = previous {
                    let _ = Allocator::free(&mut core, previous.payload, previous.payload_size);
                }

                data
            }
        };

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
    use alloc::collections::BTreeSet;

    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::{read_generic, write_generic};

    use crate::runtime::savepoint::SavePointState;

    use super::{INSTANCE_FIELDS_OFFSET, JavaHandles};

    /// A save point is a jmp_buf a longjmp will restore, so a handle sitting in
    /// its block is still a live reference - even though nothing else names it.
    /// Without this root the sweep frees the object and the resumed frame reads
    /// a hollow one, which is how a title's state turns into shells.
    #[test]
    fn a_handle_left_in_a_save_point_survives_the_sweep() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();

        let handles = JavaHandles::new(core.clone());
        let save_points = SavePointState::default();
        handles.set_save_points(save_points.clone());

        let kept = handles.allocate_instance(0).unwrap();
        let orphan = handles.allocate_instance(0).unwrap();

        // A compiled frame armed a catch and left the object in a saved
        // register; the jmp_buf block is the only place that names it.
        let point = save_points.alloc(&mut core, 0).unwrap();
        write_generic(&mut core, point + 0x20, kept).unwrap();

        handles.gc_collect_with_pins(&BTreeSet::new());

        assert!(
            handles.gc_objects.lock().contains_key(&kept),
            "an object named only by a live save point must survive"
        );
        assert!(
            !handles.gc_objects.lock().contains_key(&orphan),
            "an object no root names is still garbage"
        );
    }

    /// A guest word registered as a static root keeps the object its handle
    /// names alive, exactly how an interned constant string survives once its
    /// cache slot is rooted. An object reachable through no root is reclaimed.
    #[test]
    fn a_registered_static_root_word_keeps_its_object_alive() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();

        let handles = JavaHandles::new(core.clone());

        let kept = handles.allocate_instance(0).unwrap();
        let orphan = handles.allocate_instance(0).unwrap();

        // `kept`'s handle lives in a guest word (a constant-string cache slot);
        // registering that word roots it.
        let slot = Allocator::alloc(&mut core, 4).unwrap();
        write_generic(&mut core, slot, kept).unwrap();
        handles.register_gc_static_root(slot, slot + 4);

        handles.gc_collect_with_pins(&BTreeSet::new());

        assert!(
            handles.gc_objects.lock().contains_key(&kept),
            "an object rooted through a static-root word must survive"
        );
        assert!(
            !handles.gc_objects.lock().contains_key(&orphan),
            "an object reachable through no root is reclaimed"
        );
    }

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

    /// Mirroring a JVM array into a handle replaces the generic field block
    /// with the array block, so the collector tracks (and can reclaim) what the
    /// object actually points at, and the field block goes straight back to the
    /// heap. Repeating the crossing must not grow the heap.
    #[test]
    fn materializing_an_array_block_hands_the_field_block_back() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        let handles = JavaHandles::new(core.clone());
        handles.set_field_slots(256);

        let handle = handles.allocate_instance(0).unwrap();
        let field_block: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET).unwrap();
        assert_eq!(handles.gc_objects.lock().get(&handle).unwrap().payload, field_block);

        let bytes = [1u8, 0, 2, 0, 3, 0];
        handles.materialize_array_block(handle, 3, &bytes).unwrap();

        let array_block: u32 = read_generic(&core, handle + INSTANCE_FIELDS_OFFSET).unwrap();
        let tracked = handles.gc_objects.lock().get(&handle).copied().unwrap();
        assert_eq!(tracked.payload, array_block, "the array block is what the object now points at");
        assert_eq!(tracked.payload_size, 4 + bytes.len() as u32);
        assert!(tracked.is_array);

        // The 1 KiB field block is free again: a fresh allocation of that size
        // gets it back rather than growing the heap.
        assert_eq!(Allocator::alloc(&mut core, 256 * 4).unwrap(), field_block);

        // Mirroring the same array again reuses the block it already has, so a
        // pointer the compiled code kept into the elements stays valid.
        handles.materialize_array_block(handle, 3, &[4, 0, 5, 0, 6, 0]).unwrap();
        assert_eq!(read_generic::<u32, _>(&core, handle + INSTANCE_FIELDS_OFFSET).unwrap(), array_block);
        assert_eq!(handles.read_char_array(handle).unwrap(), [4u16, 5, 6]);
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

        // No live threads means no roots, and no JVM pins, so both objects are
        // unreachable and the collector reclaims them and clears its registry.
        let (freed, freed_bytes) = handles.gc_collect_with_pins(&alloc::collections::BTreeSet::new());
        assert_eq!(freed, 2);
        assert!(freed_bytes > 0);
        assert!(handles.gc_objects.lock().is_empty());

        // The freed memory is usable again.
        let c = handles.allocate_instance_with_fields(0, 8).unwrap();
        assert_ne!(c, 0);
        assert_eq!(handles.gc_objects.lock().len(), 1);
    }

    #[test]
    fn bridge_pins_are_reference_counted_across_nested_guards() {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        let handles = JavaHandles::new(core.clone());

        // The same identity pinned by two overlapping crossings stays pinned
        // until BOTH release it, so an inner crossing returning does not expose
        // an argument the outer crossing still holds.
        let outer = handles.pin_identities(alloc::vec![0x111, 0x222]);
        {
            let inner = handles.pin_identities(alloc::vec![0x222]);
            assert_eq!(handles.bridge_pins.lock().get(&0x222).copied(), Some(2));
            assert_eq!(handles.bridge_pins.lock().get(&0x111).copied(), Some(1));
            drop(inner);
        }
        // Inner released: 0x222 back to a single pin, 0x111 untouched.
        assert_eq!(handles.bridge_pins.lock().get(&0x222).copied(), Some(1));
        assert_eq!(handles.bridge_pins.lock().get(&0x111).copied(), Some(1));

        drop(outer);
        // Both released: the map is empty again, nothing left pinned.
        assert!(handles.bridge_pins.lock().is_empty());
    }
}
