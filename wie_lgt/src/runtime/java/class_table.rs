//! The class table an ahead-of-time compiled LGT application hands to the VM.
//!
//! LGT's toolchain compiles an application's Java classes to ARM code and
//! stores them in `binary.mod`; there is no bytecode left. What survives is a
//! description of everything the application needs *from the platform*, passed
//! to import `0x14` (`java_load_classes`) as eleven pointers.
//!
//! Six of them are read-only tables in the image:
//!
//! | table             | contents                                          |
//! |-------------------|---------------------------------------------------|
//! | `classes`         | `u32` count, then one 24 byte entry per class      |
//! | `fields`          | `(name, descriptor)` string pointer pairs          |
//! | `static_fields`   | same                                               |
//! | `virtual_methods` | same                                               |
//! | `interface_methods` | same                                            |
//! | `static_methods`  | same                                               |
//!
//! A class entry is a name pointer followed by five `(start, count)` `u16`
//! pairs slicing those tables:
//!
//! ```text
//! +0x00 u32 name
//! +0x04 u16 field_start,            +0x06 u16 field_count
//! +0x08 u16 static_field_start,     +0x0a u16 static_field_count
//! +0x0c u16 virtual_method_start,   +0x0e u16 virtual_method_count
//! +0x10 u16 interface_method_start, +0x12 u16 interface_method_count
//! +0x14 u16 static_method_start,    +0x16 u16 static_method_count
//! ```
//!
//! The remaining five pointers are zeroed arrays in `.bss` - they are *output*
//! parameters, one entry per row of the corresponding table, which the VM
//! fills in so the compiled code can reach the platform. See
//! [`super::method_bridge`] for what goes into them.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use wie_util::{ByteRead, Result, WieError, read_generic, read_null_terminated_string_bytes};

/// One platform class the application imports.
pub struct JavaClass {
    pub name: String,
    pub field_start: u32,
    pub field_count: u32,
    pub virtual_method_start: u32,
    pub virtual_method_count: u32,
    pub static_method_start: u32,
    pub static_method_count: u32,
}

/// One row of a method or field table.
///
/// Rows are addressed by a single flat index across all classes, which is also
/// the index into the matching output array.
pub struct JavaMember {
    pub class_index: u32,
    pub name: String,
    pub descriptor: String,
}

/// Addresses of the five output arrays, in the order they are passed.
pub struct OutputArrays {
    pub field_offsets: u32,
    pub static_field_offsets: u32,
    pub virtual_method_offsets: u32,
    pub interface_method_offsets: u32,
    pub static_method_offsets: u32,
}

pub struct ClassTable {
    pub classes: Vec<JavaClass>,
    /// `None` where the application left a row blank, which it does for the
    /// slots reserved at the head of each class's static method block.
    pub static_methods: Vec<Option<JavaMember>>,
    pub virtual_methods: Vec<Option<JavaMember>>,
    pub fields: Vec<Option<JavaMember>>,
    pub outputs: OutputArrays,
}

const CLASS_ENTRY_SIZE: u32 = 24;
const MEMBER_ENTRY_SIZE: u32 = 8;

/// Applications import a few dozen classes. A larger count means the pointer
/// is not a class table, and parsing on would read arbitrary memory.
const MAX_CLASSES: u32 = 1024;

fn read_string<R>(reader: &R, address: u32) -> Result<String>
where
    R: ?Sized + ByteRead,
{
    let bytes = read_null_terminated_string_bytes(reader, address)?;

    Ok(String::from_utf8_lossy(&bytes).into())
}

/// Reads `count` rows starting at `start`, attributing each to `class_index`.
fn read_members<R>(reader: &R, table: u32, start: u32, count: u32, class_index: u32, rows: &mut Vec<Option<JavaMember>>) -> Result<()>
where
    R: ?Sized + ByteRead,
{
    for index in start..start + count {
        let entry = table + index * MEMBER_ENTRY_SIZE;
        let name_address: u32 = read_generic(reader, entry)?;
        let descriptor_address: u32 = read_generic(reader, entry + 4)?;

        // Blank rows are expected, so they are recorded rather than skipped:
        // the index has to keep lining up with the output array.
        let member = if name_address == 0 || descriptor_address == 0 {
            None
        } else {
            Some(JavaMember {
                class_index,
                name: read_string(reader, name_address)?,
                descriptor: read_string(reader, descriptor_address)?,
            })
        };

        if rows.len() <= index as usize {
            rows.resize_with(index as usize + 1, || None);
        }
        rows[index as usize] = member;
    }

    Ok(())
}

impl ClassTable {
    #[allow(clippy::too_many_arguments)]
    pub fn parse<R>(
        reader: &R,
        classes: u32,
        fields: u32,
        _static_fields: u32,
        virtual_methods: u32,
        _interface_methods: u32,
        static_methods: u32,
        outputs: OutputArrays,
    ) -> Result<Self>
    where
        R: ?Sized + ByteRead,
    {
        let count: u32 = read_generic(reader, classes)?;
        if count > MAX_CLASSES {
            return Err(WieError::FatalError(format!(
                "Implausible LGT class count {count} at {classes:#x}; not a class table"
            )));
        }

        let mut table = Self {
            classes: Vec::with_capacity(count as usize),
            static_methods: Vec::new(),
            virtual_methods: Vec::new(),
            fields: Vec::new(),
            outputs,
        };

        for index in 0..count {
            let entry = classes + 4 + index * CLASS_ENTRY_SIZE;

            let name_address: u32 = read_generic(reader, entry)?;
            let field_start: u16 = read_generic(reader, entry + 4)?;
            let field_count: u16 = read_generic(reader, entry + 6)?;
            let virtual_method_start: u16 = read_generic(reader, entry + 12)?;
            let virtual_method_count: u16 = read_generic(reader, entry + 14)?;
            let static_method_start: u16 = read_generic(reader, entry + 20)?;
            let static_method_count: u16 = read_generic(reader, entry + 22)?;

            let class = JavaClass {
                name: read_string(reader, name_address)?,
                field_start: field_start.into(),
                field_count: field_count.into(),
                virtual_method_start: virtual_method_start.into(),
                virtual_method_count: virtual_method_count.into(),
                static_method_start: static_method_start.into(),
                static_method_count: static_method_count.into(),
            };

            read_members(reader, fields, class.field_start, class.field_count, index, &mut table.fields)?;
            read_members(
                reader,
                virtual_methods,
                class.virtual_method_start,
                class.virtual_method_count,
                index,
                &mut table.virtual_methods,
            )?;
            read_members(
                reader,
                static_methods,
                class.static_method_start,
                class.static_method_count,
                index,
                &mut table.static_methods,
            )?;

            table.classes.push(class);
        }

        Ok(table)
    }

    pub fn class_name(&self, index: u32) -> &str {
        self.classes.get(index as usize).map(|x| x.name.as_str()).unwrap_or("<unknown>")
    }

    /// `Class.method:descriptor`, for logs and error messages.
    pub fn describe(&self, member: &JavaMember) -> String {
        format!("{}.{}{}", self.class_name(member.class_index), member.name, member.descriptor)
    }

    /// Position of a virtual method within its own class, which is the slot
    /// number the compiled code will index the receiver's vtable with.
    pub fn virtual_slot(&self, index: u32) -> Option<u32> {
        let member = self.virtual_methods.get(index as usize)?.as_ref()?;
        let class = self.classes.get(member.class_index as usize)?;

        Some(index - class.virtual_method_start)
    }
}

/// Splits a JVM method descriptor into its parameter descriptors and return
/// descriptor. `None` if the descriptor is malformed.
pub fn split_descriptor(descriptor: &str) -> Option<(Vec<String>, String)> {
    let body = descriptor.strip_prefix('(')?;
    let (parameters, return_type) = body.split_once(')')?;

    let mut result = Vec::new();
    let mut rest = parameters;

    while !rest.is_empty() {
        let length = descriptor_length(rest)?;
        result.push(rest[..length].to_string());
        rest = &rest[length..];
    }

    Some((result, return_type.to_string()))
}

/// Length of the single field descriptor at the head of `descriptor`.
fn descriptor_length(descriptor: &str) -> Option<usize> {
    let arity = descriptor.bytes().take_while(|x| *x == b'[').count();

    match descriptor.as_bytes().get(arity)? {
        b'L' => Some(descriptor[arity..].find(';')? + arity + 1),
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b'V' => Some(arity + 1),
        _ => None,
    }
}

/// Whether a value of this type occupies two argument slots, as `long` and
/// `double` do.
pub fn is_wide(descriptor: &str) -> bool {
    descriptor == "J" || descriptor == "D"
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::{is_wide, split_descriptor};

    #[test]
    fn splits_descriptors() {
        assert_eq!(split_descriptor("()V"), Some((vec![], "V".to_string())));
        assert_eq!(split_descriptor("(II)V"), Some((vec!["I".to_string(), "I".to_string()], "V".to_string())));
        assert_eq!(
            split_descriptor("(Ljava/lang/String;Ljava/lang/String;)V"),
            Some((vec!["Ljava/lang/String;".to_string(), "Ljava/lang/String;".to_string()], "V".to_string()))
        );
        assert_eq!(
            split_descriptor("([BII)I"),
            Some((vec!["[B".to_string(), "I".to_string(), "I".to_string()], "I".to_string()))
        );
        assert_eq!(
            split_descriptor("(Lorg/kwis/msp/media/Clip;Z)Z"),
            Some((vec!["Lorg/kwis/msp/media/Clip;".to_string(), "Z".to_string()], "Z".to_string()))
        );
        assert_eq!(
            split_descriptor("([[Ljava/lang/String;)[I"),
            Some((vec!["[[Ljava/lang/String;".to_string()], "[I".to_string()))
        );
    }

    #[test]
    fn rejects_malformed_descriptors() {
        assert_eq!(split_descriptor("II)V"), None);
        assert_eq!(split_descriptor("(Ljava/lang/String)V"), None);
        assert_eq!(split_descriptor("(Q)V"), None);
    }

    #[test]
    fn wide_types_take_two_slots() {
        assert!(is_wide("J"));
        assert!(is_wide("D"));
        assert!(!is_wide("I"));
        assert!(!is_wide("Ljava/lang/String;"));
    }
}
