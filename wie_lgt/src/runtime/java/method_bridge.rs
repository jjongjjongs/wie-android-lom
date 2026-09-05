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

async fn write_back_byte_arrays(jvm: &Jvm, handles: &JavaHandles, writebacks: &[ByteArrayWriteback]) -> Result<()> {
    for writeback in writebacks {
        let bytes: Vec<i8> = match jvm.load_array(&writeback.array, 0, writeback.length).await {
            Ok(bytes) => bytes,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        handles.write_byte_array(writeback.guest_handle, &bytes)?;
    }

    Ok(())
}

struct CharArrayWriteback {
    guest_handle: u32,
    array: ClassInstanceRef<Array<u16>>,
    length: usize,
}

async fn write_back_char_arrays(jvm: &Jvm, handles: &JavaHandles, writebacks: &[CharArrayWriteback]) -> Result<()> {
    for writeback in writebacks {
        let chars: Vec<u16> = match jvm.load_array(&writeback.array, 0, writeback.length).await {
            Ok(chars) => chars,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        handles.write_char_array(writeback.guest_handle, &chars)?;
    }

    Ok(())
}

/// Copies imported native-ABI instance fields from the guest object's word
/// block into the JVM object before an imported Java method observes them.
async fn sync_guest_fields_to_jvm(jvm: &Jvm, handles: &JavaHandles, handle: u32) -> Result<()> {
    let Some(mut instance) = handles.get(handle) else {
        return Ok(());
    };

    for binding in handles.applied_field_bindings(jvm, &*instance).iter() {
        let word = handles.read_field_word(handle, binding.slot)?;

        match binding.descriptor.as_str() {
            "I" => {
                if let Err(error) = jvm.put_field(&mut instance, &binding.name, &binding.descriptor, word as i32).await {
                    return Err(JvmSupport::to_wie_err(jvm, error).await);
                }
            }
            descriptor => {
                return Err(WieError::FatalError(format!(
                    "Unsupported LGT imported field descriptor {descriptor} for {}.{}",
                    binding.class_name, binding.name
                )));
            }
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
        let word = match binding.descriptor.as_str() {
            "I" => {
                let value: i32 = match jvm.get_field(&instance, &binding.name, &binding.descriptor).await {
                    Ok(value) => value,
                    Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                };

                value as u32
            }
            descriptor => {
                return Err(WieError::FatalError(format!(
                    "Unsupported LGT imported field descriptor {descriptor} for {}.{}",
                    binding.class_name, binding.name
                )));
            }
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
    writebacks: &mut Vec<ByteArrayWriteback>,
    char_writebacks: &mut Vec<CharArrayWriteback>,
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

                        writebacks.push(ByteArrayWriteback {
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

                        char_writebacks.push(CharArrayWriteback {
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
                    // reaches the `[B` path above. Wrap it as a byte array with
                    // writeback - the buffers these calls carry are byte arrays -
                    // rather than ending the run. Guard the length first so a
                    // garbage handle cannot allocate wildly; anything implausible
                    // keeps the original diagnostic.
                    let data = read_generic::<u32, _>(core, handle + 8).unwrap_or(0);
                    let length = if data != 0 {
                        read_generic::<u32, _>(core, data).unwrap_or(u32::MAX)
                    } else {
                        u32::MAX
                    };

                    // A compiled `char[]` handed to a method that takes it as
                    // `Object` (`System.arraycopy`'s buffers) must be copied out
                    // as chars: reading its 16-bit units as bytes would halve its
                    // length and fail the JVM's char type-check on store.
                    if data != 0 && length <= MAX_WRAPPED_ARRAY_BYTES && handles.array_element_type(handle) == Some(b'C') {
                        let chars = handles.read_char_array(handle)?;
                        let length = chars.len();

                        let mut array = match jvm.instantiate_array("C", length).await {
                            Ok(array) => array,
                            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                        };

                        if let Err(error) = jvm.store_array(&mut array, 0, chars).await {
                            return Err(JvmSupport::to_wie_err(jvm, error).await);
                        }

                        char_writebacks.push(CharArrayWriteback {
                            guest_handle: handle,
                            array: array.clone().into(),
                            length,
                        });

                        JavaValue::Object(Some(array))
                    } else if data != 0 && length <= MAX_WRAPPED_ARRAY_BYTES {
                        let bytes = handles.read_byte_array(handle)?;
                        let byte_length = bytes.len();

                        let mut array = match jvm.instantiate_array("B", byte_length).await {
                            Ok(array) => array,
                            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
                        };

                        if let Err(error) = jvm.store_array(&mut array, 0, bytes).await {
                            return Err(JvmSupport::to_wie_err(jvm, error).await);
                        }

                        writebacks.push(ByteArrayWriteback {
                            guest_handle: handle,
                            array: array.clone().into(),
                            length: byte_length,
                        });

                        tracing::warn!("marshalled guest array {handle:#x} as a byte array for parameter {parameter}");
                        JavaValue::Object(Some(array))
                    } else {
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
async fn materialize_primitive_array_result(jvm: &Jvm, handles: &JavaHandles, handle: u32) -> Result<()> {
    if handle == 0 {
        return Ok(());
    }

    let Some(instance) = handles.get(handle) else {
        return Ok(());
    };

    let name = instance.class_definition().name();
    let Some(element) = name.strip_prefix('[') else {
        return Ok(());
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
        _ => return Ok(()),
    };

    handles.materialize_array_block(handle, length, &bytes)
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
        let mut writebacks = Vec::new();
        let mut char_writebacks = Vec::new();
        let arguments = marshal_arguments(core, jvm, handles, &parameters, 1, &mut writebacks, &mut char_writebacks).await?;

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
                return Err(JvmSupport::to_wie_err(jvm, error).await);
            }

            write_back_byte_arrays(jvm, handles, &writebacks).await?;
            write_back_char_arrays(jvm, handles, &char_writebacks).await?;
            sync_jvm_fields_to_guest(jvm, handles, this).await?;

            return Ok(this.into());
        }

        tracing::debug!("LGT new {class_name}{descriptor} on {this:#x}");

        let instance = match jvm.new_class(class_name, descriptor, arguments).await {
            Ok(instance) => instance,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        write_back_byte_arrays(jvm, handles, &writebacks).await?;
        write_back_char_arrays(jvm, handles, &char_writebacks).await?;
        handles.bind(this, instance);
        sync_jvm_fields_to_guest(jvm, handles, this).await?;

        return Ok(this.into());
    }

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

    let mut writebacks = Vec::new();
    let mut char_writebacks = Vec::new();
    let arguments = marshal_arguments(core, jvm, handles, &parameters, first_word, &mut writebacks, &mut char_writebacks).await?;

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

        jvm.invoke_virtual::<_, JavaValue>(&instance, name, descriptor, arguments).await
    } else {
        tracing::debug!(target: "wie_lgt::hot", "LGT invoke static {class_name}.{name}{descriptor}");

        jvm.invoke_static::<_, JavaValue>(class_name, name, descriptor, arguments).await
    };

    match result {
        Ok(value) => {
            write_back_byte_arrays(jvm, handles, &writebacks).await?;
            write_back_char_arrays(jvm, handles, &char_writebacks).await?;

            if let Some(handle) = receiver_handle {
                sync_jvm_fields_to_guest(jvm, handles, handle).await?;
            }

            let result = marshal_return(handles, value)?;
            materialize_primitive_array_result(jvm, handles, result.low).await?;

            Ok(result)
        }
        // The bridged Java method threw. Register the exception so a guest
        // handle names it, and surface it as `WieError::JavaException` so the
        // dispatcher can route it through the compiled save-point chain (a
        // try/catch in the caller) or, failing that, hand it to a Java catch
        // higher up - rather than the fatal a stringified trace would become,
        // which ends the whole title (e.g. System.arraycopy on a null array
        // thrown from a compiled paint callback).
        Err(JavaError::JavaException(exception)) => Err(WieError::JavaException(handles.address_of(exception)?)),
    }
}
