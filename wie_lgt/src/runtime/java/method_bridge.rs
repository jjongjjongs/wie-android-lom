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

use alloc::{format, string::String, vec::Vec};

use jvm::{JavaValue, Jvm};

use wie_core_arm::ArmCore;
use wie_jvm_support::JvmSupport;
use wie_util::{Result, WieError};

use super::{
    class_table::{ClassTable, JavaMember, is_wide, split_descriptor},
    handles::JavaHandles,
};

/// Reads the `count` argument words a call was made with.
fn read_arguments(core: &ArmCore, count: usize) -> Result<Vec<u32>> {
    (0..count).map(|index| core.read_param(index)).collect()
}

/// Converts raw argument words into JVM values using the parameter
/// descriptors.
fn marshal_arguments(core: &ArmCore, handles: &JavaHandles, parameters: &[String], receiver: Option<JavaValue>) -> Result<Vec<JavaValue>> {
    let slots: usize = parameters.iter().map(|x| if is_wide(x) { 2 } else { 1 }).sum();
    let words = read_arguments(core, slots + usize::from(receiver.is_some()))?;

    let mut values = Vec::with_capacity(parameters.len() + 1);
    let mut word = 0;

    if let Some(receiver) = receiver {
        values.push(receiver);
        word += 1;
    }

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
            _ => JavaValue::Object(handles.get(words[word])),
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

/// Invokes an imported method and returns the word to put in `r0`.
///
/// `receiver` is `None` for a static method. A `<init>` row is a constructor:
/// the compiled code expects a new instance back, so it is handled as a
/// construction rather than an invocation.
pub async fn invoke(
    core: &mut ArmCore,
    jvm: &Jvm,
    handles: &JavaHandles,
    table: &ClassTable,
    member: &JavaMember,
    receiver: Option<u32>,
) -> Result<u32> {
    let class_name = table.class_name(member.class_index);

    let Some((parameters, _)) = split_descriptor(&member.descriptor) else {
        return Err(WieError::FatalError(format!("Malformed descriptor on {}", table.describe(member))));
    };

    if member.name == "<init>" {
        let arguments = marshal_arguments(core, handles, &parameters, None)?;

        tracing::debug!("LGT new {class_name}{}", member.descriptor);

        let instance = match jvm.new_class(class_name, &member.descriptor, arguments).await {
            Ok(instance) => instance,
            Err(error) => return Err(JvmSupport::to_wie_err(jvm, error).await),
        };

        return handles.insert(instance);
    }

    let receiver = match receiver {
        Some(handle) => match handles.get(handle) {
            Some(instance) => Some(instance),
            None => {
                return Err(WieError::FatalError(format!(
                    "{} called on unknown instance {handle:#x}",
                    table.describe(member)
                )));
            }
        },
        None => None,
    };

    let arguments = marshal_arguments(core, handles, &parameters, receiver.clone().map(|x| JavaValue::Object(Some(x))))?;

    let result = if let Some(instance) = receiver {
        tracing::debug!("LGT invoke virtual {class_name}.{}{}", member.name, member.descriptor);

        // The receiver was pushed as the first value; the JVM takes it
        // separately.
        let arguments = arguments[1..].to_vec();

        jvm.invoke_virtual::<_, JavaValue>(&instance, &member.name, &member.descriptor, arguments)
            .await
    } else {
        tracing::debug!("LGT invoke static {class_name}.{}{}", member.name, member.descriptor);

        jvm.invoke_static::<_, JavaValue>(class_name, &member.name, &member.descriptor, arguments)
            .await
    };

    match result {
        Ok(value) => marshal_return(handles, value),
        Err(error) => Err(JvmSupport::to_wie_err(jvm, error).await),
    }
}
