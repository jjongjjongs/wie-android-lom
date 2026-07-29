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
//! - **fields** get a byte offset into the instance.
//!
//! Arguments arrive under the ARM procedure call standard: the first four
//! words in `r0`-`r3`, the rest on the stack, with `long` and `double` taking
//! two slots each. They are converted using the method descriptor, since the
//! raw words carry no type information.

use alloc::{boxed::Box, format, string::String, vec, vec::Vec};

use jvm::{ClassInstance, JavaValue, Jvm, Result as JvmResult};

use wie_core_arm::ArmCore;
use wie_jvm_support::JvmSupport;
use wie_util::{ByteRead, ByteWrite, Result, WieError};

use super::{
    class_table::{ClassTable, is_wide, split_descriptor},
    handles::JavaHandles,
};

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
}

/// Reads the `count` argument words a call was made with.
fn read_arguments(core: &ArmCore, count: usize) -> Result<Vec<u32>> {
    (0..count).map(|index| core.read_param(index)).collect()
}

/// Converts raw argument words into JVM values using the parameter
/// descriptors.
/// `first_word` is where the declared parameters start, which is one past
/// `this` for anything called on an object.
/// An array argument, with the guest allocation it was built from so what the
/// callee writes can be copied back.
struct BorrowedArray {
    guest: u32,
    element_size: u32,
    length: u32,
    instance: Box<dyn ClassInstance>,
}

/// Builds a JVM array holding what a guest array holds.
///
/// The compiled code builds arrays itself, in guest memory, with no instance
/// on the JVM side - which is fine until one is passed to a platform method.
/// `String.<init>([BII)` takes one, and so does every `read` an application
/// loads a resource with.
async fn borrow_array(core: &ArmCore, jvm: &Jvm, handles: &JavaHandles, element: u8, guest: u32) -> Result<Option<BorrowedArray>> {
    let Some((length, element_size)) = handles.array_at(guest) else {
        return Ok(None);
    };

    let data = handles.array_data(guest)?;
    let mut bytes = vec![0u8; (length * element_size) as usize];
    core.read_bytes(data, &mut bytes)?;

    let element_type = alloc::string::String::from_utf8_lossy(&[element]).into_owned();

    let mut instance = match jvm.instantiate_array(&element_type, length as _).await {
        Ok(instance) => instance,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    let stored = match element {
        b'B' | b'Z' => {
            jvm.store_array(&mut instance, 0, bytes.iter().map(|x| *x as i8).collect::<Vec<_>>())
                .await
        }
        b'C' => {
            jvm.store_array(
                &mut instance,
                0,
                bytes.chunks(2).map(|x| u16::from_le_bytes([x[0], x[1]])).collect::<Vec<_>>(),
            )
            .await
        }
        b'S' => {
            jvm.store_array(
                &mut instance,
                0,
                bytes.chunks(2).map(|x| i16::from_le_bytes([x[0], x[1]])).collect::<Vec<_>>(),
            )
            .await
        }
        b'I' => {
            jvm.store_array(
                &mut instance,
                0,
                bytes.chunks(4).map(|x| i32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect::<Vec<_>>(),
            )
            .await
        }
        // Anything wider or an array of objects: the array exists and is the
        // right length, and its contents do not cross.
        _ => Ok(()),
    };

    if let Err(error) = stored {
        return Err(JvmSupport::to_wie_err(jvm, error).await);
    }

    Ok(Some(BorrowedArray {
        guest,
        element_size,
        length,
        instance,
    }))
}

/// Writes back what the callee put in a borrowed array.
async fn return_array(core: &mut ArmCore, jvm: &Jvm, handles: &JavaHandles, borrowed: &BorrowedArray, element: u8) -> Result<()> {
    let bytes = match element {
        b'B' | b'Z' => match jvm.load_array::<i8>(&borrowed.instance, 0, borrowed.length as _).await {
            Ok(values) => values.into_iter().map(|x| x as u8).collect::<Vec<_>>(),
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        },
        b'C' => match jvm.load_array::<u16>(&borrowed.instance, 0, borrowed.length as _).await {
            Ok(values) => values.into_iter().flat_map(u16::to_le_bytes).collect(),
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        },
        b'S' => match jvm.load_array::<i16>(&borrowed.instance, 0, borrowed.length as _).await {
            Ok(values) => values.into_iter().flat_map(i16::to_le_bytes).collect(),
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        },
        b'I' => match jvm.load_array::<i32>(&borrowed.instance, 0, borrowed.length as _).await {
            Ok(values) => values.into_iter().flat_map(i32::to_le_bytes).collect(),
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        },
        _ => return Ok(()),
    };

    let data = handles.array_data(borrowed.guest)?;
    core.write_bytes(data, &bytes[..(borrowed.length * borrowed.element_size) as usize])?;

    Ok(())
}

fn marshal_arguments(core: &ArmCore, handles: &JavaHandles, parameters: &[String], first_word: usize) -> Result<Vec<JavaValue>> {
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
            // A zero word is a null reference, which is a value. A non-zero
            // one this runtime never handed out is not: passing it on as null
            // reaches a platform method that dereferences it without checking,
            // and the failure then reads as a bug in that method rather than
            // as the missing object it is.
            // An array is filled in by the caller, which needs the JVM.
            b'[' => JavaValue::Object(None),
            _ => match handles.get(words[word]) {
                Some(instance) => JavaValue::Object(Some(instance)),
                None if words[word] == 0 => JavaValue::Object(None),
                None => {
                    return Err(WieError::FatalError(format!(
                        "Argument {word} of {} is {:#x}, which names no object this runtime handed out",
                        parameters.join(""),
                        words[word]
                    )));
                }
            },
        };

        word += if is_wide(parameter) { 2 } else { 1 };
        values.push(value);
    }

    Ok(values)
}

/// Converts a JVM return value into the word the compiled code expects.
///
/// Objects are handed back as handles rather than pointers, because the
/// instance lives on the Rust side; `long` and `double` are truncated, which
/// no imported method returns today.
fn marshal_return(handles: &JavaHandles, value: JavaValue) -> Result<u32> {
    Ok(match value {
        JavaValue::Void => 0,
        JavaValue::Boolean(x) => x.into(),
        JavaValue::Byte(x) => x as i32 as u32,
        JavaValue::Char(x) => x.into(),
        JavaValue::Short(x) => x as i32 as u32,
        JavaValue::Int(x) => x as u32,
        JavaValue::Float(x) => x.to_bits(),
        JavaValue::Long(x) => x as u32,
        JavaValue::Double(x) => x.to_bits() as u32,
        JavaValue::Object(x) => match x {
            Some(instance) => handles.insert(instance)?,
            None => 0,
        },
    })
}

/// Marshals the arguments, building a JVM array for every array among them.
///
/// The borrowed arrays come back with the values so the caller can write what
/// the callee put in them back into guest memory.
async fn marshal(
    core: &mut ArmCore,
    jvm: &Jvm,
    handles: &JavaHandles,
    parameters: &[String],
    first_word: usize,
) -> Result<(Vec<JavaValue>, Vec<(BorrowedArray, u8)>)> {
    let mut values = marshal_arguments(core, handles, parameters, first_word)?;
    let mut borrowed = Vec::new();
    let mut word = first_word;

    for (index, parameter) in parameters.iter().enumerate() {
        // One dimension of a primitive, which is what crosses; an array of
        // arrays or of objects holds handles that mean nothing to the JVM.
        let element = parameter
            .strip_prefix('[')
            .filter(|rest| rest.len() == 1)
            .and_then(|rest| rest.bytes().next());

        if let Some(element) = element {
            let handle = core.read_param(word)?;

            if let Some(array) = borrow_array(core, jvm, handles, element, handle).await? {
                values[index] = JavaValue::Object(Some(array.instance.clone()));
                borrowed.push((array, element));
            } else if handle != 0 {
                tracing::warn!("Array argument {index} is {handle:#x}, which names no array this runtime handed out");
            }
        }

        word += if is_wide(parameter) { 2 } else { 1 };
    }

    Ok((values, borrowed))
}

/// Invokes an imported method and returns the word to put in `r0`.
///
/// `receiver` is `None` for a static method. A `<init>` row is a constructor:
/// the compiled code expects a new instance back, so it is handled as a
/// construction rather than an invocation.
pub async fn invoke(core: &mut ArmCore, jvm: &Jvm, handles: &JavaHandles, member: &ResolvedMember, receiver: Option<u32>) -> Result<u32> {
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
        let (arguments, borrowed) = marshal(core, jvm, handles, &parameters, 1).await?;

        // An object already bound to an instance is being initialized, not
        // created: this is a subclass running its superclass constructor, and
        // constructing a second object would discard the one in play. The
        // superclass is frequently abstract, so it could not be constructed
        // anyway.
        if let Some(instance) = handles.get(this) {
            tracing::debug!("LGT {class_name}.<init>{descriptor} on existing {this:#x}");

            let result: JvmResult<()> = jvm.invoke_special(&instance, class_name, "<init>", descriptor, arguments).await;
            if let Err(error) = result {
                return Err(JvmSupport::to_wie_err(jvm, error).await);
            }

            for (array, element) in &borrowed {
                return_array(core, jvm, handles, array, *element).await?;
            }

            return Ok(this);
        }

        tracing::debug!("LGT new {class_name}{descriptor} on {this:#x}");

        let instance = match jvm.new_class(class_name, descriptor, arguments).await {
            Ok(instance) => instance,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        for (array, element) in &borrowed {
            return_array(core, jvm, handles, array, *element).await?;
        }

        handles.bind(this, instance);

        return Ok(this);
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

    let (arguments, borrowed) = marshal(core, jvm, handles, &parameters, usize::from(receiver.is_some())).await?;

    let result = if let Some(instance) = receiver {
        tracing::debug!("LGT invoke virtual {class_name}.{name}{descriptor}");

        jvm.invoke_virtual::<_, JavaValue>(&instance, name, descriptor, arguments).await
    } else {
        tracing::debug!("LGT invoke static {class_name}.{name}{descriptor}");

        jvm.invoke_static::<_, JavaValue>(class_name, name, descriptor, arguments).await
    };

    let value = match result {
        Ok(value) => value,
        Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
    };

    for (array, element) in &borrowed {
        return_array(core, jvm, handles, array, *element).await?;
    }

    marshal_return(handles, value)
}
