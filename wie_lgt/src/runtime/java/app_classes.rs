//! The classes an ahead-of-time compiled LGT application brings with it.
//!
//! Import `0x07` registers them as `{ u32 count, u32 pad, u32 root[count] }`.
//! Each root is followed inline by its member table, and preceded by a 76 byte
//! metadata block:
//!
//! ```text
//! metadata:
//!   +0x00 u32 flags
//!   +0x08 u32 name
//!   +0x10 u32 superclass name, zero for none
//!   +0x18 u16 member count
//!
//! root (immediately after the metadata):
//!   +0x00 runtime slots, zero in the image
//!   +0x08 u32 metadata
//!   +0x0c u32 flags
//!   +0x10 member table
//! ```
//!
//! A described member is `{ owner root, name, descriptor, flags, ... }`, but
//! the entries are **not** a fixed size: fields run 20 bytes and methods 28,
//! with variants. Rather than guess a size, the table is walked by its one
//! invariant - a described entry begins with the owning class's root - which
//! resynchronises after anything unrecognised.
//!
//! Field and method rows are told apart by the descriptor, not by a flag bit:
//! only a method descriptor starts with `(`. A method's ARM entry point sits
//! at `+0x14`, and the high half of its flags is the number of argument words
//! it expects, `this` included.
//!
//! Not every class describes its members. Some carry a bare vtable of entry
//! points with no names at all, so the walk stops at the first row it cannot
//! read and reports how far it got rather than failing: a partially read class
//! is still useful for looking entry points up, and an application should not
//! fail to load over one.

use alloc::{format, string::String, vec::Vec};

use wie_util::{ByteRead, Result, WieError, read_generic, read_null_terminated_string_bytes};

const METADATA_SIZE: u32 = 0x4c;
const MEMBER_TABLE_OFFSET: u32 = 0x10;
const METHOD_ENTRY_OFFSET: u32 = 0x14;
const FIELD_SLOT_OFFSET: u32 = 0x10;

/// How far the walk will look for the next entry before giving up. Member
/// tables are dense; a gap this large means the table was misparsed.
const MAX_ENTRY_GAP: u32 = 0x100;

/// Applications register a few dozen classes.
const MAX_CLASSES: u32 = 4096;

#[derive(Debug)]
pub enum AppMember {
    Field {
        name: String,
        descriptor: String,
        slot: u32,
    },
    Method {
        name: String,
        descriptor: String,
        /// Address of the compiled code, in ARM mode.
        entry: u32,
        /// Argument words the compiled code expects, `this` included.
        argument_words: u32,
    },
}

impl AppMember {
    pub fn name(&self) -> &str {
        match self {
            AppMember::Field { name, .. } | AppMember::Method { name, .. } => name,
        }
    }

    pub fn descriptor(&self) -> &str {
        match self {
            AppMember::Field { descriptor, .. } | AppMember::Method { descriptor, .. } => descriptor,
        }
    }
}

#[derive(Debug)]
pub struct AppClass {
    pub root: u32,
    pub name: String,
    pub superclass: Option<String>,
    pub members: Vec<AppMember>,
    /// Members the metadata declared, which is more than `members` holds when
    /// the class does not describe all of them.
    pub declared_members: u32,
}

impl AppClass {
    pub fn methods(&self) -> impl Iterator<Item = &AppMember> {
        self.members.iter().filter(|x| matches!(x, AppMember::Method { .. }))
    }
}

fn read_string<R>(reader: &R, address: u32) -> Result<Option<String>>
where
    R: ?Sized + ByteRead,
{
    if address == 0 {
        return Ok(None);
    }

    let bytes = read_null_terminated_string_bytes(reader, address)?;
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&bytes).into()))
}

/// Reads one class and its member table.
fn parse_class<R>(reader: &R, root: u32) -> Result<AppClass>
where
    R: ?Sized + ByteRead,
{
    let metadata: u32 = read_generic(reader, root + 8)?;
    if metadata == 0 || metadata + METADATA_SIZE != root {
        return Err(WieError::FatalError(format!(
            "LGT class root {root:#x} has metadata at {metadata:#x}, which is not the block before it"
        )));
    }

    let name = read_string(reader, read_generic(reader, metadata + 8)?)?
        .ok_or_else(|| WieError::FatalError(format!("LGT class root {root:#x} has no name")))?;
    let superclass = read_string(reader, read_generic(reader, metadata + 0x10)?)?;
    let count: u16 = read_generic(reader, metadata + 0x18)?;

    let mut members = Vec::with_capacity(count.into());
    let mut cursor = root + MEMBER_TABLE_OFFSET;

    for _ in 0..count {
        // Resynchronise on the owner word, which starts every described entry.
        let mut owner: u32 = read_generic(reader, cursor)?;
        let limit = cursor + MAX_ENTRY_GAP;
        while owner != root && cursor < limit {
            cursor += 4;
            owner = read_generic(reader, cursor)?;
        }
        if owner != root {
            break;
        }

        let member_name = read_string(reader, read_generic(reader, cursor + 4)?)?;
        let descriptor = read_string(reader, read_generic(reader, cursor + 8)?)?;
        let flags: u32 = read_generic(reader, cursor + 0xc)?;

        // A row without both strings is a placeholder, or a table that does
        // not describe its members at all.
        let (Some(member_name), Some(descriptor)) = (member_name, descriptor) else {
            break;
        };

        members.push(if descriptor.starts_with('(') {
            AppMember::Method {
                name: member_name,
                descriptor,
                entry: read_generic(reader, cursor + METHOD_ENTRY_OFFSET)?,
                argument_words: flags >> 16,
            }
        } else {
            AppMember::Field {
                name: member_name,
                descriptor,
                slot: read_generic(reader, cursor + FIELD_SLOT_OFFSET)?,
            }
        });

        cursor += 4;
    }

    Ok(AppClass {
        root,
        name,
        superclass,
        members,
        declared_members: count.into(),
    })
}

/// Whether `root` looks like a class root: the metadata block sits directly
/// before it, and names a class.
fn is_class_root<R>(reader: &R, root: u32) -> bool
where
    R: ?Sized + ByteRead,
{
    let Ok(metadata) = read_generic::<u32, _>(reader, root + 8) else {
        return false;
    };
    if metadata == 0 || metadata + METADATA_SIZE != root {
        return false;
    }

    read_generic::<u32, _>(reader, metadata + 8).is_ok_and(|name| matches!(read_string(reader, name), Ok(Some(_))))
}

/// Finds a class by name anywhere in the loaded image.
///
/// The class an application starts from is not in the table it registers -
/// Legend of Master registers 18 classes and its `Lm` is not one of them - so
/// it has to be found by shape. A root is recognisable enough that scanning
/// finds every class and nothing else.
pub fn find_class<R>(reader: &R, ranges: &[(u32, u32)], name: &str) -> Option<AppClass>
where
    R: ?Sized + ByteRead,
{
    for (base, size) in ranges {
        // A root always follows its metadata, so nothing below that can be one.
        let mut address = base + METADATA_SIZE;

        while address < base + size {
            if is_class_root(reader, address)
                && let Ok(class) = parse_class(reader, address)
                && class.name == name
            {
                return Some(class);
            }

            address += 4;
        }
    }

    None
}

/// Reads every class registered through import `0x07`.
///
/// A class that fails to parse is dropped with a warning rather than failing
/// the load: the rest of the application is still worth running, and the
/// classes are only used to look entry points up.
pub fn parse_registered_classes<R>(reader: &R, classes: u32) -> Result<Vec<AppClass>>
where
    R: ?Sized + ByteRead,
{
    let count: u32 = read_generic(reader, classes)?;
    if count > MAX_CLASSES {
        return Err(WieError::FatalError(format!(
            "Implausible LGT application class count {count} at {classes:#x}"
        )));
    }

    let mut parsed = Vec::with_capacity(count as usize);

    for index in 0..count {
        let root: u32 = read_generic(reader, classes + 8 + index * 4)?;

        match parse_class(reader, root) {
            Ok(class) => parsed.push(class),
            Err(error) => tracing::warn!("Skipping LGT application class {index} at {root:#x}: {error}"),
        }
    }

    Ok(parsed)
}

/// One line per class, for the trace.
pub fn describe(class: &AppClass) -> String {
    let methods = class.methods().count();

    format!(
        "{} extends {} at {:#x} ({} methods, {}/{} members described)",
        class.name,
        class.superclass.as_deref().unwrap_or("-"),
        class.root,
        methods,
        class.members.len(),
        class.declared_members
    )
}

/// `Class.method:descriptor` for a member, for the trace.
pub fn describe_member(class: &AppClass, member: &AppMember) -> String {
    match member {
        AppMember::Field { slot, .. } => format!("{}.{}:{} slot {slot}", class.name, member.name(), member.descriptor()),
        AppMember::Method { entry, argument_words, .. } => format!(
            "{} {}.{}{} at {entry:#x}, {argument_words} argument words",
            if member.is_instance_method() { "instance" } else { "static" },
            class.name,
            member.name(),
            member.descriptor()
        ),
    }
}

impl AppMember {
    /// Whether this looks like an instance method, judged by comparing the
    /// declared argument words with the descriptor's own arity.
    pub fn is_instance_method(&self) -> bool {
        let AppMember::Method {
            descriptor, argument_words, ..
        } = self
        else {
            return false;
        };

        descriptor_argument_words(descriptor).is_some_and(|words| *argument_words == words + 1)
    }
}

/// Argument words a descriptor's parameters occupy, `long` and `double`
/// counting twice. `None` if the descriptor is malformed.
pub fn descriptor_argument_words(descriptor: &str) -> Option<u32> {
    let body = descriptor.strip_prefix('(')?;
    let (parameters, _) = body.split_once(')')?;

    let mut words = 0;
    let mut rest = parameters;

    while !rest.is_empty() {
        let arity = rest.bytes().take_while(|x| *x == b'[').count();

        let length = match rest.as_bytes().get(arity)? {
            b'L' => rest[arity..].find(';')? + arity + 1,
            b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => arity + 1,
            _ => return None,
        };

        words += if arity == 0 && matches!(rest.as_bytes()[0], b'J' | b'D') { 2 } else { 1 };
        rest = &rest[length..];
    }

    Some(words)
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{AppMember, descriptor_argument_words};

    #[test]
    fn counts_argument_words() {
        assert_eq!(descriptor_argument_words("()V"), Some(0));
        assert_eq!(descriptor_argument_words("(Z)V"), Some(1));
        assert_eq!(descriptor_argument_words("([Ljava/lang/String;)V"), Some(1));
        assert_eq!(descriptor_argument_words("(II)V"), Some(2));
        assert_eq!(descriptor_argument_words("(JD)V"), Some(4));
        assert_eq!(descriptor_argument_words("([J)V"), Some(1));
        assert_eq!(descriptor_argument_words("bad"), None);
    }

    /// The flags of Legend of Master's `Lm` methods, which are the reference
    /// for how argument words are counted.
    #[test]
    fn recognises_instance_methods() {
        let start_app = AppMember::Method {
            name: "startApp".to_string(),
            descriptor: "([Ljava/lang/String;)V".to_string(),
            entry: 0x1118,
            argument_words: 2,
        };
        assert!(start_app.is_instance_method());

        let pause_app = AppMember::Method {
            name: "pauseApp".to_string(),
            descriptor: "()V".to_string(),
            entry: 0x1248,
            argument_words: 1,
        };
        assert!(pause_app.is_instance_method());

        let static_like = AppMember::Method {
            name: "main".to_string(),
            descriptor: "()V".to_string(),
            entry: 0x2000,
            argument_words: 0,
        };
        assert!(!static_like.is_instance_method());
    }
}
