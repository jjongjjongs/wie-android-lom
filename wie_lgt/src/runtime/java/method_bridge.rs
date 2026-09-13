//! Connects the platform methods an ahead-of-time compiled LGT application
//! imports to the classes `wie_wipi_java` already implements.
//!
//! At load time every row of the application's method tables gets an entry in
//! the corresponding output array:
//!
//! - **static methods** get a function pointer to an SVC stub. The compiled
//!   code loads it and branches, so the stub is the method as far as it is
//!   concerned.
//! - **virtual methods** get the method's slot number within its own class.
//!   The compiled code indexes the receiver's vtable with it, so dispatch has
//!   to go through an object built by [`super::instance`].
//! - **fields** get a word slot into the instance's field block.
//!
//! Arguments arrive under the ARM procedure call standard: the first four
//! words in `r0`-`r3`, the rest on the stack, with `long` and `double` taking
//! two slots each. They are converted using the method descriptor, since the
//! raw words carry no type information.

use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::{format, string::String, vec::Vec};

use jvm::{Array, ClassInstanceRef, JavaError, JavaValue, Jvm, Result as JvmResult};
use spin::Mutex;

use wie_core_arm::ArmCore;
use wie_jvm_support::JvmSupport;
use wie_util::{Result, WieError, read_generic};

use super::{
    class_table::{ClassTable, is_wide, split_descriptor},
    handles::JavaHandles,
    platform_metadata::platform_class,
};

/// Per-bridged-Java-method call counts (`class.name`), so the perf meter can
/// name which methods a JVM-bound title spends its time invoking. When two
/// SVC-plumbing optimizations left an `init`-bound title's MIPS unmoved, the
/// remaining cost has to be the RustJava execution of the methods the game
/// calls — this says which ones, so the next step targets real hot methods
/// instead of guessing.
static JAVA_CALL_COUNT: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

fn note_java_call(class_name: &str, name: &str) {
    let mut counts = JAVA_CALL_COUNT.lock();
    // `entry` needs an owned key; only the first call for a method allocates,
    // afterwards it is a lookup-and-increment.
    if let Some(count) = counts.get_mut(&*format!("{class_name}.{name}")) {
        *count += 1;
    } else {
        counts.insert(format!("{class_name}.{name}"), 1);
    }
}

/// Log the top bridged Java methods by call rate, draining the counters.
pub fn report_hot_java(dt_ms: u64) {
    use core::fmt::Write;

    let drained: Vec<(String, u64)> = {
        let mut counts = JAVA_CALL_COUNT.lock();
        core::mem::take(&mut *counts).into_iter().collect()
    };
    if drained.is_empty() {
        return;
    }

    let mut top: Vec<(String, u64)> = drained;
    top.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    top.truncate(6);

    let mut line = "[java]".to_string();
    for (name, count) in &top {
        let per_s = *count as f64 * 1000.0 / dt_ms as f64;
        let _ = write!(line, " {name}={per_s:.0}/s");
    }
    tracing::info!("{line}");
}

/// A row of the class table, lifted out so the table's lock is not held while
/// the JVM runs - a call can re-enter the runtime and want it again.
pub struct ResolvedMember {
    pub class_name: String,
    pub name: String,
    pub descriptor: String,
}

impl ResolvedMember {
    /// Reads one row of the static method table.
    pub fn static_method(table: &ClassTable, index: u32) -> Option<Self> {
        let member = table.static_methods.get(index as usize)?.as_ref()?;

        Some(Self {
            class_name: table.class_name(member.class_index).into(),
            name: member.name.clone(),
            descriptor: member.descriptor.clone(),
        })
    }

    /// Reads one row of the virtual method table.
    pub fn virtual_method(table: &ClassTable, index: u32) -> Option<Self> {
        let member = table.virtual_methods.get(index as usize)?.as_ref()?;

        Some(Self {
            class_name: table.class_name(member.class_index).into(),
            name: member.name.clone(),
            descriptor: member.descriptor.clone(),
        })
    }

    /// Reads one row of the interface-method table.
    pub fn interface_method(table: &ClassTable, index: u32) -> Option<Self> {
        let member = table.interface_methods.get(index as usize)?.as_ref()?;

        Some(Self {
            class_name: table.class_name(member.class_index).into(),
            name: member.name.clone(),
            descriptor: member.descriptor.clone(),
        })
    }
}

/// Whether the platform declares `name`/`descriptor` on `class_name` or one of
/// its superclasses as an *instance* method.
///
/// The compiled code reaches a `super.m(...)` through a static import row, so a
/// static row naming an instance method is a non-virtual call with `this` in
/// the first word rather than a real static call.
fn platform_declares_instance_method(class_name: &str, name: &str, descriptor: &str) -> bool {
    const ACC_STATIC: u32 = 0x0008;
    const MAX_DEPTH: usize = 32;

    let mut current = class_name;
    for _ in 0..MAX_DEPTH {
        let Some(class) = platform_class(current) else {
            return false;
        };

        if let Some(method) = class.methods.iter().find(|method| method.name == name && method.descriptor == descriptor) {
            return method.flags & ACC_STATIC == 0;
        }

        let Some(superclass) = class.superclass else {
            return false;
        };
        current = superclass;
    }

    false
}

/// Reads the `count` argument words a call was made with.
fn read_arguments(core: &ArmCore, count: usize) -> Result<Vec<u32>> {
    (0..count).map(|index| core.read_param(index)).collect()
}

/// Upper bound on a guest array wrapped as a byte array from an `Object`
/// parameter. A plausible I/O or copy buffer is well under this; a larger
/// "length" means the handle is not really an array, so it is not chased.
const MAX_WRAPPED_ARRAY_BYTES: u32 = 0x0010_0000;

struct ByteArrayWriteback {
    guest_handle: u32,
    array: ClassInstanceRef<Array<i8>>,
    length: usize,
}

struct CharArrayWriteback {
    guest_handle: u32,
    array: ClassInstanceRef<Array<u16>>,
    length: usize,
}

struct PrimitiveArrayWriteback {
    guest_handle: u32,
    array: ClassInstanceRef<Array<()>>,
    /// The element's JVM descriptor byte, which says both how to read the JVM
    /// array back and how wide the guest elements are.
    element: u8,
    count: usize,
}

struct ReferenceArrayWriteback {
    guest_handle: u32,
    array: ClassInstanceRef<Array<ClassInstanceRef<()>>>,
    /// The handles the guest array held when it was wrapped, so an element the
    /// JVM only ever saw as null - because no instance was registered under it -
    /// is left as it was instead of being cleared.
    original: Vec<u32>,
}

/// The temporary JVM arrays a call's guest array arguments were copied into,
/// and the guest arrays to copy them back to once the call returns.
#[derive(Default)]
struct GuestArrayWritebacks {
    bytes: Vec<ByteArrayWriteback>,
    chars: Vec<CharArrayWriteback>,
    primitives: Vec<PrimitiveArrayWriteback>,
    references: Vec<ReferenceArrayWriteback>,
}

impl GuestArrayWritebacks {
    async fn write_back(&self, jvm: &Jvm, handles: &JavaHandles) -> Result<()> {
        for writeback in &self.bytes {
            let bytes: Vec<i8> = match jvm.load_array(&writeback.array, 0, writeback.length).await {
                Ok(bytes) => bytes,
                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
            };

            handles.write_byte_array(writeback.guest_handle, &bytes)?;
        }

        for writeback in &self.chars {
            let chars: Vec<u16> = match jvm.load_array(&writeback.array, 0, writeback.length).await {
                Ok(chars) => chars,
                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
            };

            handles.write_char_array(writeback.guest_handle, &chars)?;
        }

        for writeback in &self.primitives {
            // Each arm loads the JVM array back as the type it was made with
            // and lays the elements down at the guest's own width.
            macro_rules! write_back {
                ($ty:ty, $width:literal) => {{
                    let values: Vec<$ty> = match jvm.load_array(&writeback.array, 0, writeback.count).await {
                        Ok(values) => values,
                        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                    };

                    let mut bytes = Vec::with_capacity(values.len() * $width);
                    for value in values {
                        bytes.extend_from_slice(&value.to_le_bytes());
                    }

                    handles.write_array_bytes(writeback.guest_handle, $width, &bytes)?;
                }};
            }

            match writeback.element {
                b'S' => write_back!(i16, 2),
                b'I' => write_back!(i32, 4),
                b'F' => write_back!(f32, 4),
                b'J' => write_back!(i64, 8),
                b'D' => write_back!(f64, 8),
                element => {
                    return Err(WieError::FatalError(format!(
                        "Guest array {:#x} was wrapped as element type {element:#x}, which has no writeback",
                        writeback.guest_handle
                    )));
                }
            }
        }

        for writeback in &self.references {
            let elements: Vec<ClassInstanceRef<()>> = match jvm.load_array(&writeback.array, 0, writeback.original.len()).await {
                Ok(elements) => elements,
                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
            };

            let mut references = Vec::with_capacity(elements.len());
            for (element, &original) in elements.into_iter().zip(&writeback.original) {
                references.push(match element.instance {
                    Some(instance) => handles.address_of(instance)?,
                    // The JVM saw null here only because nothing was registered
                    // under the handle the guest had, so it never had the chance
                    // to replace it. Keep what the guest wrote.
                    None if handles.get(original).is_none() => original,
                    None => 0,
                });
            }

            handles.write_reference_array(writeback.guest_handle, &references)?;
        }

        Ok(())
    }
}

/// Copies imported native-ABI instance fields from the guest object's word
/// block into the JVM object before an imported Java method observes them.
async fn sync_guest_fields_to_jvm(jvm: &Jvm, handles: &JavaHandles, handle: u32) -> Result<()> {
    let Some(mut instance) = handles.get(handle) else {
        return Ok(());
    };

    for binding in handles.applied_field_bindings(jvm, &*instance).iter() {
        let word = handles.read_field_word(handle, binding.slot)?;

        let result = match binding.descriptor.as_bytes()[0] {
            b'Z' => jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word != 0).await,
            b'B' => jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word as i8).await,
            b'C' => jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word as u16).await,
            b'S' => jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word as i16).await,
            b'I' => jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word as i32).await,
            b'F' => {
                jvm.put_field(&mut instance, &binding.name, &binding.descriptor, f32::from_bits(word))
                    .await
            }
            // A reference field holds a handle, which names the instance the
            // JVM side has to see. 서든어택 포켓's loader thread touches a card
            // whose imported `Lorg/kwis/msp/lcdui/InputMethodHandler;` field
            // ended the whole thread when this was fatal.
            //
            // A zero word is the one thing it cannot say. It names no object,
            // and a platform class routinely fills such a field from its own
            // constructor - 오즈's text field has its `imHandler` built there -
            // so the guest has not been told the handle yet and a call that
            // reaches back into the platform mid-construction would clear what
            // was just built. Leave a reference the JVM has and the guest does
            // not; the guest's own null is the case this cannot carry across.
            b'L' | b'[' => match handles.get(word) {
                Some(referent) => {
                    jvm.put_field(
                        &mut instance,
                        &binding.name,
                        &binding.descriptor,
                        ClassInstanceRef::<()>::new(Some(referent)),
                    )
                    .await
                }
                None => {
                    let held: ClassInstanceRef<()> = match jvm.get_field(&instance, &binding.name, &binding.descriptor).await {
                        Ok(held) => held,
                        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                    };

                    if held.is_null() {
                        Ok(())
                    } else {
                        continue;
                    }
                }
            },
            // `long` and `double` occupy two words, and nothing says which
            // order this layout puts them in; leave the JVM's own value alone
            // rather than write a guess over it.
            _ => {
                tracing::debug!(
                    "Leaving imported field {}.{} ({}) as the JVM has it",
                    binding.class_name,
                    binding.name,
                    binding.descriptor
                );
                Ok(())
            }
        };

        if let Err(error) = result {
            return Err(JvmSupport::to_wie_err(jvm, error).await);
        }
    }

    Ok(())
}

/// Copies imported instance fields modified by JVM code back to the native
/// guest word slots consumed directly by AOT ARM code.
async fn sync_jvm_fields_to_guest(jvm: &Jvm, handles: &JavaHandles, handle: u32) -> Result<()> {
    let Some(instance) = handles.get(handle) else {
        return Ok(());
    };

    for binding in handles.applied_field_bindings(jvm, &*instance).iter() {
        // Each arm reads the field back as the type it was written with; the
        // word the guest reads is that value in the one word its slot has.
        macro_rules! word {
            ($ty:ty, $convert:expr) => {{
                let value: $ty = match jvm.get_field(&instance, &binding.name, &binding.descriptor).await {
                    Ok(value) => value,
                    Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                };

                #[allow(clippy::redundant_closure_call)]
                $convert(value)
            }};
        }

        let word = match binding.descriptor.as_bytes()[0] {
            b'Z' => word!(bool, u32::from),
            b'B' => word!(i8, |value| value as u32),
            b'C' => word!(u16, u32::from),
            b'S' => word!(i16, |value| value as u32),
            b'I' => word!(i32, |value| value as u32),
            b'F' => word!(f32, f32::to_bits),
            b'L' | b'[' => {
                let value: ClassInstanceRef<()> = match jvm.get_field(&instance, &binding.name, &binding.descriptor).await {
                    Ok(value) => value,
                    Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                };

                match value.instance {
                    Some(instance) => handles.address_of(instance)?,
                    None => 0,
                }
            }
            // Two-word fields, as above: nothing to write back.
            _ => continue,
        };

        handles.write_field_word(handle, binding.slot, word)?;
    }

    Ok(())
}

/// Converts raw argument words into JVM values using the parameter
/// descriptors.
/// `first_word` is where the declared parameters start, which is one past
/// `this` for anything called on an object.
async fn marshal_arguments(
    core: &ArmCore,
    jvm: &Jvm,
    handles: &JavaHandles,
    parameters: &[String],
    first_word: usize,
    writebacks: &mut GuestArrayWritebacks,
) -> Result<Vec<JavaValue>> {
    let slots: usize = parameters.iter().map(|x| if is_wide(x) { 2 } else { 1 }).sum();
    let words = read_arguments(core, slots + first_word)?;

    let mut values = Vec::with_capacity(parameters.len());
    let mut word = first_word;

    for parameter in parameters {
        let value = match parameter.as_bytes()[0] {
            b'Z' => JavaValue::Boolean(words[word] != 0),
            b'B' => JavaValue::Byte(words[word] as i8),
            b'C' => JavaValue::Char(words[word] as u16),
            b'S' => JavaValue::Short(words[word] as i16),
            b'I' => JavaValue::Int(words[word] as i32),
            b'F' => JavaValue::Float(f32::from_bits(words[word])),
            b'J' => JavaValue::Long(((words[word + 1] as u64) << 32 | words[word] as u64) as i64),
            b'D' => JavaValue::Double(f64::from_bits((words[word + 1] as u64) << 32 | words[word] as u64)),
            // A byte array allocated by the compiled application exists only
            // in guest memory. Imported JVM methods need a real JVM `[B`, so
            // copy the guest bytes into a temporary JVM array.
            b'[' if parameter == "[B" => {
                let handle = words[word];

                match handles.get(handle) {
                    Some(instance) => JavaValue::Object(Some(instance)),
                    None if handle == 0 => JavaValue::Object(None),
                    None => {
                        let bytes = handles.read_byte_array(handle)?;
                        let length = bytes.len();

                        let mut array = match jvm.instantiate_array("B", length).await {
                            Ok(array) => array,
                            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                        };

                        if let Err(error) = jvm.store_array(&mut array, 0, bytes).await {
                            return Err(JvmSupport::to_wie_err(jvm, error).await);
                        }

                        writebacks.bytes.push(ByteArrayWriteback {
                            guest_handle: handle,
                            array: array.clone().into(),
                            length,
                        });

                        JavaValue::Object(Some(array))
                    }
                }
            }

            // A guest `char[]` (e.g. `String.<init>([C)`). Its elements are
            // 16-bit units, so reading them as bytes would keep only their low
            // halves and hand the JVM a `char[]` half the intended length whose
            // slots then fail the char type-check. Copy them as chars into a
            // real JVM `[C`.
            b'[' if parameter == "[C" => {
                let handle = words[word];

                match handles.get(handle) {
                    Some(instance) => JavaValue::Object(Some(instance)),
                    None if handle == 0 => JavaValue::Object(None),
                    None => {
                        let chars = handles.read_char_array(handle)?;
                        let length = chars.len();

                        let mut array = match jvm.instantiate_array("C", length).await {
                            Ok(array) => array,
                            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                        };

                        if let Err(error) = jvm.store_array(&mut array, 0, chars).await {
                            return Err(JvmSupport::to_wie_err(jvm, error).await);
                        }

                        writebacks.chars.push(CharArrayWriteback {
                            guest_handle: handle,
                            array: array.clone().into(),
                            length,
                        });

                        JavaValue::Object(Some(array))
                    }
                }
            }

            // A zero word is a null reference, which is a value. A non-zero
            // one this runtime never handed out is not: passing it on as null
            // reaches a platform method that dereferences it without checking,
            // and the failure then reads as a bug in that method rather than
            // as the missing object it is.
            _ => match handles.get(words[word]) {
                Some(instance) => JavaValue::Object(Some(instance)),
                None if words[word] == 0 => JavaValue::Object(None),
                None => {
                    let handle = words[word];

                    // A compiled array passed where the method declares Object
                    // (System.arraycopy's src/dst, an I/O read buffer) never
                    // reaches the `[B` path above, so wrap it here from the
                    // element type its allocation recorded. Guard the length
                    // first so a garbage handle cannot allocate wildly; anything
                    // implausible keeps the original diagnostic.
                    let data = read_generic::<u32, _>(core, handle + 8).unwrap_or(0);
                    let length = if data != 0 {
                        read_generic::<u32, _>(core, data).unwrap_or(u32::MAX)
                    } else {
                        u32::MAX
                    };

                    if data == 0 || length > MAX_WRAPPED_ARRAY_BYTES {
                        let vtable = read_generic::<u32, _>(core, handle).unwrap_or(0);
                        let root = if vtable != 0 {
                            read_generic::<u32, _>(core, vtable).unwrap_or(0)
                        } else {
                            0
                        };

                        return Err(WieError::FatalError(format!(
                            "Argument {word} of {} is {handle:#x}, which names no object this runtime handed out; vtable={vtable:#x}, class_root={root:#x}",
                            parameters.join("")
                        )));
                    }

                    // The header counts elements, not bytes, so an array whose
                    // elements are wider than a byte has to be wrapped at its
                    // own width: read as bytes it would be a fraction of its
                    // length, and a copy would move a fraction of its contents
                    // and land the low byte of each element in a whole slot.
                    // `System.arraycopy` between two compiled arrays is where
                    // this shows up.
                    let element = handles.array_element_type(handle).unwrap_or(b'B');

                    // Every arm builds a JVM array of the right element type,
                    // fills it from guest memory, and registers the copy back.
                    macro_rules! wrap {
                        ($descriptor:literal, $ty:ty, $width:literal) => {{
                            let bytes = handles.read_array_bytes(handle, $width)?;
                            let values = bytes
                                .chunks_exact($width)
                                .map(|chunk| <$ty>::from_le_bytes(chunk.try_into().unwrap()))
                                .collect::<Vec<$ty>>();
                            let count = values.len();

                            let mut array = match jvm.instantiate_array($descriptor, count).await {
                                Ok(array) => array,
                                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                            };

                            if let Err(error) = jvm.store_array(&mut array, 0, values).await {
                                return Err(JvmSupport::to_wie_err(jvm, error).await);
                            }

                            writebacks.primitives.push(PrimitiveArrayWriteback {
                                guest_handle: handle,
                                array: array.clone().into(),
                                element,
                                count,
                            });

                            JavaValue::Object(Some(array))
                        }};
                    }

                    match element {
                        // A compiled `char[]` (`String.<init>([C)`, or a copy
                        // buffer). Its 16-bit units go to a real JVM `[C`.
                        b'C' => {
                            let chars = handles.read_char_array(handle)?;
                            let length = chars.len();

                            let mut array = match jvm.instantiate_array("C", length).await {
                                Ok(array) => array,
                                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                            };

                            if let Err(error) = jvm.store_array(&mut array, 0, chars).await {
                                return Err(JvmSupport::to_wie_err(jvm, error).await);
                            }

                            writebacks.chars.push(CharArrayWriteback {
                                guest_handle: handle,
                                array: array.clone().into(),
                                length,
                            });

                            JavaValue::Object(Some(array))
                        }
                        // A compiled object array (`System.arraycopy` on the
                        // `String[]` a word-wrap builds). Its elements are
                        // handles, which name JVM objects rather than values.
                        b'L' => {
                            let references = handles.read_reference_array(handle)?;
                            let elements = references
                                .iter()
                                .map(|&reference| ClassInstanceRef::new(handles.get(reference)))
                                .collect::<Vec<ClassInstanceRef<()>>>();

                            let mut array = match jvm.instantiate_array("Ljava/lang/Object;", elements.len()).await {
                                Ok(array) => array,
                                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                            };

                            if let Err(error) = jvm.store_array(&mut array, 0, elements).await {
                                return Err(JvmSupport::to_wie_err(jvm, error).await);
                            }

                            writebacks.references.push(ReferenceArrayWriteback {
                                guest_handle: handle,
                                array: array.clone().into(),
                                original: references,
                            });

                            JavaValue::Object(Some(array))
                        }
                        b'S' => wrap!("S", i16, 2),
                        b'I' => wrap!("I", i32, 4),
                        b'F' => wrap!("F", f32, 4),
                        b'J' => wrap!("J", i64, 8),
                        b'D' => wrap!("D", f64, 8),
                        // `byte`, `boolean`, and anything whose allocation this
                        // runtime never saw. One byte an element either way.
                        _ => {
                            if handles.array_element_type(handle).is_none() {
                                tracing::debug!("guest array {handle:#x} names no element type; assuming bytes for {parameter}");
                            }

                            let bytes = handles.read_byte_array(handle)?;
                            let byte_length = bytes.len();

                            let mut array = match jvm.instantiate_array("B", byte_length).await {
                                Ok(array) => array,
                                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                            };

                            if let Err(error) = jvm.store_array(&mut array, 0, bytes).await {
                                return Err(JvmSupport::to_wie_err(jvm, error).await);
                            }

                            writebacks.bytes.push(ByteArrayWriteback {
                                guest_handle: handle,
                                array: array.clone().into(),
                                length: byte_length,
                            });

                            JavaValue::Object(Some(array))
                        }
                    }
                }
            },
        };

        word += if is_wide(parameter) { 2 } else { 1 };
        values.push(value);
    }

    Ok(values)
}

/// A bridged method's return, in the registers the compiled code reads it back
/// from: `r0`, and for a `long` or `double` the high half in `r1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JavaReturn {
    pub low: u32,
    pub high: Option<u32>,
}

impl From<u32> for JavaReturn {
    fn from(low: u32) -> Self {
        Self { low, high: None }
    }
}

impl JavaReturn {
    fn wide(value: u64) -> Self {
        Self {
            low: value as u32,
            high: Some((value >> 32) as u32),
        }
    }
}

/// Converts a JVM return value into the registers the compiled code expects.
///
/// Objects are handed back as handles rather than pointers, because the
/// instance lives on the Rust side. A `long` or `double` occupies two
/// registers, low half first - truncating it to `r0` left `r1` holding
/// whatever the call had put there, which is how Fantasy Knight's
/// `System.currentTimeMillis()` came back with a garbage high word and its
/// authentication card asked to sleep for eighty-five billion milliseconds.
fn marshal_return(handles: &JavaHandles, value: JavaValue) -> Result<JavaReturn> {
    Ok(match value {
        JavaValue::Void => 0.into(),
        JavaValue::Boolean(x) => u32::from(x).into(),
        JavaValue::Byte(x) => (x as i32 as u32).into(),
        JavaValue::Char(x) => u32::from(x).into(),
        JavaValue::Short(x) => (x as i32 as u32).into(),
        JavaValue::Int(x) => (x as u32).into(),
        JavaValue::Float(x) => x.to_bits().into(),
        JavaValue::Long(x) => JavaReturn::wide(x as u64),
        JavaValue::Double(x) => JavaReturn::wide(x.to_bits()),
        JavaValue::Object(x) => match x {
            // An object returned from a JVM method may already have a guest
            // handle - e.g. a compiled `d` item stored in a JVM Vector and read
            // back with elementAt. `address_of` returns that existing handle (and
            // its populated field block); only a genuinely new object gets a fresh
            // one. Using `insert` here instead allocated a fresh, zeroed instance
            // every time, so a round-tripped item came back empty - its arrays
            // null - which showed as a shell item and made the save serializer
            // throw a NullPointerException on the missing data.
            Some(instance) => handles.address_of(instance)?.into(),
            None => 0.into(),
        },
    })
}

/// Mirrors a primitive array a platform method returned into guest memory.
///
/// The compiled code usually hands platform methods arrays it allocated
/// itself, so their `[length][elements]` block already lives in guest memory.
/// An array a platform method *returns* is a fresh JVM object whose guest
/// handle carries only the generic field block `insert` leaves behind, so the
/// compiled code reading its elements directly (as the LGT text word-wrapper
/// does with `String.toCharArray()`) would see an empty array. Copy the JVM
/// contents into a proper guest block so a direct read matches the JVM side.
pub(super) async fn materialize_primitive_array_result(jvm: &Jvm, handles: &JavaHandles, handle: u32) -> Result<Option<MirroredArray>> {
    if handle == 0 {
        return Ok(None);
    }

    let Some(instance) = handles.get(handle) else {
        return Ok(None);
    };

    let name = instance.class_definition().name();
    let Some(element) = name.strip_prefix('[') else {
        return Ok(None);
    };

    let length = match jvm.array_length(&instance).await {
        Ok(length) => length as u32,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    macro_rules! load_le {
        ($ty:ty, $to_bytes:expr) => {{
            let values: Vec<$ty> = match jvm.load_array(&instance, 0, length as usize).await {
                Ok(values) => values,
                Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
            };
            values.iter().flat_map($to_bytes).collect::<Vec<u8>>()
        }};
    }

    // Only primitive arrays are mirrored: their elements are self-contained
    // bytes. A reference array's elements are handles that would each need
    // their own materialisation, and the compiled code reaches those through
    // platform calls rather than by reading the block directly.
    let bytes = match element.as_bytes()[0] {
        b'C' => load_le!(u16, |value: &u16| value.to_le_bytes()),
        b'B' | b'Z' => load_le!(i8, |value: &i8| [*value as u8]),
        b'S' => load_le!(i16, |value: &i16| value.to_le_bytes()),
        b'I' => load_le!(i32, |value: &i32| value.to_le_bytes()),
        b'F' => load_le!(f32, |value: &f32| value.to_le_bytes()),
        b'J' => load_le!(i64, |value: &i64| value.to_le_bytes()),
        b'D' => load_le!(f64, |value: &f64| value.to_le_bytes()),
        _ => return Ok(None),
    };

    handles.materialize_array_block(handle, length, &bytes)?;

    Ok(Some(MirroredArray {
        handle,
        element: element.as_bytes()[0],
        count: length as usize,
        mirrored: bytes,
    }))
}

/// A JVM array whose elements were copied into a guest block, and the copy that
/// was written there.
pub(super) struct MirroredArray {
    pub handle: u32,
    /// The element's JVM descriptor byte.
    pub element: u8,
    pub count: usize,
    /// The bytes [`materialize_primitive_array_result`] laid down. What the
    /// guest block holds afterwards is the guest's own doing.
    mirrored: Vec<u8>,
}

/// The reverse: copies a mirrored array's guest elements back into the JVM
/// array they were mirrored from, if the guest wrote them.
///
/// A compiled method handed an array does not return it - it fills it, the way
/// `InputStream.read(byte[], int, int)` does - so what the call wrote lands in
/// the guest block and nowhere else, and without this the JVM array the caller
/// holds still reads as it did before the call.
///
/// The guard matters as much as the copy. A compiled method often fills its
/// array by calling back into the platform - `System.arraycopy` into it is how
/// 놈3's stream reads - and that call reaches the JVM array itself, leaving the
/// guest block untouched. Writing an unchanged block back over it would undo
/// exactly the work the call did, so a block the guest did not touch is left
/// alone and the JVM array keeps what the platform put in it.
pub(super) async fn store_primitive_array_from_guest(jvm: &Jvm, handles: &JavaHandles, array: &MirroredArray) -> Result<()> {
    let MirroredArray {
        handle,
        element,
        count,
        mirrored,
    } = array;

    let Some(mut instance) = handles.get(*handle) else {
        return Ok(());
    };

    macro_rules! store_le {
        ($ty:ty, $width:literal) => {{
            let bytes = handles.read_array_bytes(*handle, $width)?;
            if bytes == *mirrored {
                return Ok(());
            }

            let values: Vec<$ty> = bytes
                .chunks_exact($width)
                .take(*count)
                .map(|chunk| <$ty>::from_le_bytes(chunk.try_into().unwrap()))
                .collect();

            if let Err(error) = jvm.store_array(&mut instance, 0, values).await {
                return Err(JvmSupport::to_wie_err(jvm, error).await);
            }
        }};
    }

    match element {
        b'C' => store_le!(u16, 2),
        b'B' | b'Z' => store_le!(i8, 1),
        b'S' => store_le!(i16, 2),
        b'I' => store_le!(i32, 4),
        b'F' => store_le!(f32, 4),
        b'J' => store_le!(i64, 8),
        b'D' => store_le!(f64, 8),
        _ => return Ok(()),
    }

    Ok(())
}

/// Invokes an imported method and returns the word to put in `r0`.
///
/// `receiver` is `None` for a static method. A `<init>` row is a constructor:
/// the compiled code expects a new instance back, so it is handled as a
/// construction rather than an invocation.
/// Instance identities of the object arguments in `arguments`, so a bridge
/// crossing can pin them against the collector for the call's duration.
///
/// While a bridged JVM method runs, these objects live only as Rust-stack
/// values here - reachable from neither the guest roots nor a JVM thread frame
/// the collector scans - so a sweep triggered by an allocation inside the call
/// would otherwise reclaim one and hand the guest a freed address.
fn argument_identities(arguments: &[JavaValue]) -> Vec<usize> {
    arguments
        .iter()
        .filter_map(|value| match value {
            JavaValue::Object(Some(instance)) => Some(instance.identity()),
            _ => None,
        })
        .collect()
}

pub async fn invoke(core: &mut ArmCore, jvm: &Jvm, handles: &JavaHandles, member: &ResolvedMember, receiver: Option<u32>) -> Result<JavaReturn> {
    let ResolvedMember {
        class_name,
        name,
        descriptor,
    } = member;

    let Some((parameters, _)) = split_descriptor(descriptor) else {
        return Err(WieError::FatalError(format!("Malformed descriptor on {class_name}.{name}{descriptor}")));
    };

    // A constructor row is not a factory. The compiled code allocates the
    // object, prepares it through the class's first reserved row, then calls
    // the constructor on it - so `this` arrives in the first word and the
    // object it names is what the caller goes on to use.
    if name == "<init>" {
        let this = core.read_param(0)?;
        let mut writebacks = GuestArrayWritebacks::default();
        let arguments = marshal_arguments(core, jvm, handles, &parameters, 1, &mut writebacks).await?;

        // Pin the constructor's object arguments for the call's duration: an
        // allocation inside the constructor can trigger a sweep, and these live
        // only on this Rust stack (observed freeing a live String argument to
        // StringBuffer.<init>, later reused as a byte array).
        let _pin = handles.pin_identities(argument_identities(&arguments));

        // An object already bound to an instance is being initialized, not
        // created: this is a subclass running its superclass constructor, and
        // constructing a second object would discard the one in play. The
        // superclass is frequently abstract, so it could not be constructed
        // anyway.
        if let Some(instance) = handles.get(this) {
            tracing::debug!("LGT {class_name}.<init>{descriptor} on existing {this:#x}");

            sync_guest_fields_to_jvm(jvm, handles, this).await?;

            let result: JvmResult<()> = jvm.invoke_special(&instance, class_name, "<init>", descriptor, arguments).await;
            if let Err(error) = result {
                return Err(thrown_or_fatal(jvm, handles, error).await);
            }

            writebacks.write_back(jvm, handles).await?;
            sync_jvm_fields_to_guest(jvm, handles, this).await?;

            return Ok(this.into());
        }

        tracing::debug!("LGT new {class_name}{descriptor} on {this:#x}");

        let instance = match jvm.new_class(class_name, descriptor, arguments).await {
            Ok(instance) => instance,
            Err(error) => return Err(thrown_or_fatal(jvm, handles, error).await),
        };

        writebacks.write_back(jvm, handles).await?;
        handles.bind(this, instance);
        sync_jvm_fields_to_guest(jvm, handles, this).await?;

        return Ok(this.into());
    }

    // A static row naming an instance method is how the compiled code encodes
    // `invokespecial` - a `super.m(...)` call - with `this` in the first word.
    // Invoking it as static hands `this` to the first declared parameter, and
    // the JVM rejects the call outright: 서든어택 포켓's loader thread died on
    // `TextFieldComponent.focusNotify` before it could load anything.
    let super_call = receiver.is_none() && platform_declares_instance_method(class_name, name, descriptor);
    let receiver = match receiver {
        Some(handle) => Some(handle),
        None if super_call => Some(core.read_param(0)?),
        None => None,
    };

    let receiver = match receiver {
        Some(handle) => match handles.get(handle) {
            Some(instance) => Some(instance),
            None => {
                return Err(WieError::FatalError(format!(
                    "{class_name}.{name}{descriptor} called on unknown instance {handle:#x}"
                )));
            }
        },
        None => None,
    };

    let first_word = usize::from(receiver.is_some());

    let mut writebacks = GuestArrayWritebacks::default();
    let arguments = marshal_arguments(core, jvm, handles, &parameters, first_word, &mut writebacks).await?;

    // Pin the receiver and object arguments for the call's duration: an
    // allocation inside the callee can trigger a sweep, and these live only on
    // this Rust stack, invisible to the guest and JVM roots the collector scans.
    let mut pinned = argument_identities(&arguments);
    if let Some(instance) = &receiver {
        pinned.push(instance.identity());
    }
    let _pin = handles.pin_identities(pinned);

    let receiver_handle = receiver.as_ref().map(|_| core.read_param(0)).transpose()?;

    if let Some(handle) = receiver_handle {
        sync_guest_fields_to_jvm(jvm, handles, handle).await?;
    }

    note_java_call(class_name, name);

    let result = if let Some(instance) = receiver {
        // Held at warn by default (via the `wie_lgt::hot` target): one line per
        // guest method call is the single biggest capture flood, and it buries
        // the rare semantic events (file I/O, item grants) a save/load trace
        // needs. Raise `wie_lgt::hot=trace` to see the per-call sequence.
        tracing::debug!(target: "wie_lgt::hot", "LGT invoke virtual {class_name}.{name}{descriptor}");

        if super_call {
            jvm.invoke_special::<_, JavaValue>(&instance, class_name, name, descriptor, arguments)
                .await
        } else {
            jvm.invoke_virtual::<_, JavaValue>(&instance, name, descriptor, arguments).await
        }
    } else {
        tracing::debug!(target: "wie_lgt::hot", "LGT invoke static {class_name}.{name}{descriptor}");

        jvm.invoke_static::<_, JavaValue>(class_name, name, descriptor, arguments).await
    };

    match result {
        Ok(value) => {
            writebacks.write_back(jvm, handles).await?;

            if let Some(handle) = receiver_handle {
                sync_jvm_fields_to_guest(jvm, handles, handle).await?;
            }

            let result = marshal_return(handles, value)?;
            let _ = materialize_primitive_array_result(jvm, handles, result.low).await?;

            Ok(result)
        }
        // The bridged Java method threw. Register the exception so a guest
        // handle names it, and surface it as `WieError::JavaException` so the
        // dispatcher can route it through the compiled save-point chain (a
        // try/catch in the caller) or, failing that, hand it to a Java catch
        // higher up - rather than the fatal a stringified trace would become,
        // which ends the whole title (e.g. System.arraycopy on a null array
        // thrown from a compiled paint callback).
        Err(error) => Err(thrown_or_fatal(jvm, handles, error).await),
    }
}

/// Turns what a bridged Java call threw into something the compiled caller can
/// still catch.
///
/// A guest handle is registered for the exception and it comes back as
/// [`WieError::JavaException`], which the dispatcher routes through the
/// compiled save-point chain - a `try`/`catch` in the compiled code - or hands
/// to a Java catch further up. The stringified trace [`JvmSupport::to_wie_err`]
/// produces is a fatal, and a fatal ends the title: 놈3 reads its save file by
/// handing what it found, null and all, to `new ByteArrayInputStream(...)` and
/// catching the `NullPointerException` that follows, and the constructor path
/// here was the one place that still turned a throw into one of those.
///
/// The fatal remains for an exception no handle can be made for, because
/// nothing in the compiled code could name it to catch it either.
async fn thrown_or_fatal(jvm: &Jvm, handles: &JavaHandles, error: JavaError) -> WieError {
    let JavaError::JavaException(exception) = error;

    match handles.address_of(exception.clone()) {
        Ok(address) => WieError::JavaException(address),
        Err(_) => JvmSupport::to_wie_err(jvm, JavaError::JavaException(exception)).await,
    }
}
