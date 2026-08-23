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
//!   +0x18 u16 instance field words
//!   +0x28 u32 interface table, zero for none
//!   +0x38 u32 method table, zero for none
//!
//! root (immediately after the metadata):
//!   +0x00 runtime slots, zero in the image
//!   +0x08 u32 metadata
//!   +0x0c u32 flags
//!   +0x10 field table
//! ```
//!
//! A class keeps its fields and its methods in two separate tables. The fields
//! start at `root+0x10` and run, 20 bytes each, right up to the method table.
//! The method table opens with a `u32` count and continues with rows of 28
//! bytes. Both row kinds begin with the owning class's root, which is what
//! makes the boundary checkable.
//!
//! ```text
//! field:  { u32 owner, u32 name, u32 descriptor, u32 flags, u32 slot }
//! method: { u32 owner, u32 name, u32 descriptor, u32 flags, u32, u32 entry, u32 }
//! ```
//!
//! The high half of a method's flags is the number of argument words it
//! expects, `this` included.
//!
//! The metadata's own member count is **not** the size of these tables -
//! Legend of Master's `f` declares 425 and has 409 fields and 372 methods - so
//! it is recorded for the trace and nothing else. Getting this wrong is
//! expensive rather than merely incomplete: reading only the first 425 rows
//! stops in the middle of the method table, which hides every method a class
//! overrides and leaves an earlier row of the same descriptor looking like the
//! override.
//!
//! Not every class carries these tables. Most of an application's classes have
//! no method table at all, because nothing outside the compiled code ever
//! needs to call them by name; those parse as a class with no members, which
//! is correct rather than a failure.

use alloc::{format, string::String, vec::Vec};

use wie_util::{ByteRead, Result, WieError, read_generic, read_null_terminated_string_bytes};

const METADATA_SIZE: u32 = 0x4c;

const METADATA_NAME: u32 = 0x08;
const METADATA_SUPERCLASS: u32 = 0x10;
const METADATA_INSTANCE_WORDS: u32 = 0x18;
const METADATA_INTERFACES: u32 = 0x28;
const METADATA_METHODS: u32 = 0x38;

const FIELD_TABLE_OFFSET: u32 = 0x10;
const FIELD_ROW_SIZE: u32 = 0x14;
const FIELD_SLOT_OFFSET: u32 = 0x10;

const METHOD_ROW_SIZE: u32 = 0x1c;
const METHOD_ENTRY_OFFSET: u32 = 0x14;

/// A ceiling on either table, so a stray pointer cannot make the loader walk
/// the whole image. The largest class seen so far declares 409 fields and 372
/// methods.
const MAX_MEMBERS: u32 = 8192;

/// Applications register a few dozen classes.
const MAX_CLASSES: u32 = 4096;

#[derive(Debug, Clone)]
pub enum AppMember {
    Field {
        name: String,
        descriptor: String,
        flags: u32,
        slot: u32,
    },
    Method {
        name: String,
        descriptor: String,
        flags: u32,
        /// Address of this 28-byte method row in guest memory.
        row: u32,
        /// Native dispatch/interface slot from row +0x10.
        slot: u32,
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

    pub fn flags(&self) -> u32 {
        match self {
            AppMember::Field { flags, .. } | AppMember::Method { flags, .. } => *flags,
        }
    }

    pub fn slot(&self) -> u32 {
        match self {
            AppMember::Field { slot, .. } | AppMember::Method { slot, .. } => *slot,
        }
    }

    /// Address of the native method row, when this member is a method.
    pub fn method_row(&self) -> Option<u32> {
        match self {
            AppMember::Method { row, .. } => Some(*row),
            AppMember::Field { .. } => None,
        }
    }

    /// Replaces the linked native dispatch/interface slot of a method.
    pub fn set_method_slot(&mut self, linked_slot: u32) -> bool {
        match self {
            AppMember::Method { slot, .. } => {
                *slot = linked_slot;
                true
            }
            AppMember::Field { .. } => false,
        }
    }

    pub fn entry(&self) -> Option<u32> {
        match self {
            AppMember::Method { entry, .. } => Some(*entry),
            AppMember::Field { .. } => None,
        }
    }

    pub fn is_field(&self) -> bool {
        matches!(self, AppMember::Field { .. })
    }

    pub fn is_method(&self) -> bool {
        matches!(self, AppMember::Method { .. })
    }
}

#[derive(Debug, Clone)]
pub struct AppClass {
    pub root: u32,
    pub get_class: u32,
    pub get_raw_class: u32,
    pub name: String,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub members: Vec<AppMember>,
    /// Number of four-byte instance-field words native allocation reserves.
    pub instance_words: u32,
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

/// Reads the superclass a class's metadata points at.
///
/// The pointer is a name when the superclass is a platform class, and another
/// class's root when it is one of the application's own - Battle Monster's
/// `Game` extends `a`, which extends `org/kwis/msp/lcdui/Jlet`. A root is
/// recognisable by the metadata block sitting directly before it.
fn read_superclass<R>(reader: &R, pointer: u32) -> Result<Option<String>>
where
    R: ?Sized + ByteRead,
{
    if pointer == 0 {
        return Ok(None);
    }

    if let Ok(metadata) = read_generic::<u32, _>(reader, pointer + 8)
        && metadata != 0
        && metadata.checked_add(METADATA_SIZE) == Some(pointer)
        && let Ok(name) = read_generic::<u32, _>(reader, metadata + 8)
    {
        return read_string(reader, name);
    }

    read_string(reader, pointer)
}

/// Reads the name and descriptor a member row opens with, once the row is
/// known to belong to `root`. `None` for anything that does not read as a
/// described member, which ends the table it is in.
fn read_member_head<R>(reader: &R, root: u32, row: u32) -> Option<(String, String)>
where
    R: ?Sized + ByteRead,
{
    if read_generic::<u32, _>(reader, row).ok()? != root {
        return None;
    }

    let name = read_string(reader, read_generic(reader, row + 4).ok()?).ok()??;
    let descriptor = read_string(reader, read_generic(reader, row + 8).ok()?).ok()??;

    Some((name, descriptor))
}

/// Reads the fields between the root and the method table.
fn parse_fields<R>(reader: &R, root: u32, end: u32) -> Result<Vec<AppMember>>
where
    R: ?Sized + ByteRead,
{
    let mut fields = Vec::new();
    let mut row = root + FIELD_TABLE_OFFSET;

    while row + FIELD_ROW_SIZE <= end && fields.len() < MAX_MEMBERS as usize {
        let Some((name, descriptor)) = read_member_head(reader, root, row) else {
            break;
        };

        fields.push(AppMember::Field {
            name,
            descriptor,
            flags: read_generic(reader, row + 0xc)?,
            slot: read_generic(reader, row + FIELD_SLOT_OFFSET)?,
        });

        row += FIELD_ROW_SIZE;
    }

    Ok(fields)
}

/// Reads the counted method table.
fn parse_methods<R>(reader: &R, root: u32, table: u32) -> Result<Vec<AppMember>>
where
    R: ?Sized + ByteRead,
{
    let count: u32 = read_generic(reader, table)?;
    if count > MAX_MEMBERS {
        return Err(WieError::FatalError(format!(
            "LGT class root {root:#x} declares {count} methods at {table:#x}"
        )));
    }

    let mut methods = Vec::with_capacity(count as usize);

    for index in 0..count {
        let row = table + 4 + index * METHOD_ROW_SIZE;

        let Some((name, descriptor)) = read_member_head(reader, root, row) else {
            break;
        };

        let flags: u32 = read_generic(reader, row + 0xc)?;

        methods.push(AppMember::Method {
            name,
            descriptor,
            flags,
            row,
            slot: read_generic(reader, row + 0x10)?,
            entry: read_generic(reader, row + METHOD_ENTRY_OFFSET)?,
            argument_words: flags >> 16,
        });
    }

    Ok(methods)
}

/// Reads the counted table of interface names a class implements.
fn parse_interfaces<R>(reader: &R, table: u32) -> Result<Vec<String>>
where
    R: ?Sized + ByteRead,
{
    if table == 0 {
        return Ok(Vec::new());
    }

    let count: u32 = read_generic(reader, table)?;
    if count > MAX_MEMBERS {
        return Ok(Vec::new());
    }

    let mut interfaces = Vec::with_capacity(count as usize);

    for index in 0..count {
        match read_string(reader, read_generic(reader, table + 4 + index * 4)?)? {
            Some(name) => interfaces.push(name),
            None => break,
        }
    }

    Ok(interfaces)
}

/// Reads one class and its member tables.
fn parse_class<R>(reader: &R, root: u32) -> Result<AppClass>
where
    R: ?Sized + ByteRead,
{
    let metadata: u32 = read_generic(reader, root + 8)?;
    if metadata == 0 || metadata.checked_add(METADATA_SIZE) != Some(root) {
        return Err(WieError::FatalError(format!(
            "LGT class root {root:#x} has metadata at {metadata:#x}, which is not the block before it"
        )));
    }

    let name = read_string(reader, read_generic(reader, metadata + METADATA_NAME)?)?
        .ok_or_else(|| WieError::FatalError(format!("LGT class root {root:#x} has no name")))?;
    let superclass = read_superclass(reader, read_generic(reader, metadata + METADATA_SUPERCLASS)?)?;
    let instance_words: u16 = read_generic(reader, metadata + METADATA_INSTANCE_WORDS)?;

    let methods_table: u32 = read_generic(reader, metadata + METADATA_METHODS)?;
    let interfaces = parse_interfaces(reader, read_generic(reader, metadata + METADATA_INTERFACES)?)?;

    // Without a method table there is nothing to bound the fields with, and a
    // class with neither describes no members at all.
    let mut members = if methods_table > root + FIELD_TABLE_OFFSET {
        parse_fields(reader, root, methods_table)?
    } else {
        Vec::new()
    };

    if methods_table != 0 {
        members.extend(parse_methods(reader, root, methods_table)?);
    }

    Ok(AppClass {
        root,
        get_class: read_generic(reader, metadata + 0x30)?,
        get_raw_class: read_generic(reader, metadata + 0x34)?,
        name,
        superclass,
        interfaces,
        members,
        instance_words: instance_words.into(),
    })
}

/// Parses one application class from the native `class_shared` root supplied by
/// `vm_resolve_one` (Java import 0x13).
pub fn parse_class_root<R>(reader: &R, root: u32) -> Result<AppClass>
where
    R: ?Sized + ByteRead,
{
    parse_class(reader, root)
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
    if metadata == 0 || metadata.checked_add(METADATA_SIZE) != Some(root) {
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
        "{} extends {}{}{} at {:#x} ({} fields, {} methods, {} instance words)",
        class.name,
        class.superclass.as_deref().unwrap_or("-"),
        if class.interfaces.is_empty() { "" } else { " implements " },
        class.interfaces.join(", "),
        class.root,
        class.members.len() - methods,
        methods,
        class.instance_words
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
    use alloc::{string::ToString, vec, vec::Vec};

    use wie_util::{ByteRead, Result};

    use super::{
        AppMember, FIELD_ROW_SIZE, FIELD_SLOT_OFFSET, FIELD_TABLE_OFFSET, METADATA_INSTANCE_WORDS, METADATA_METHODS, METADATA_NAME, METADATA_SIZE,
        METADATA_SUPERCLASS, METHOD_ENTRY_OFFSET, METHOD_ROW_SIZE, descriptor_argument_words, parse_class,
    };

    /// A flat image laid out from a fixed base, so a class can be built by
    /// writing words and strings at known addresses.
    struct Image {
        base: u32,
        data: Vec<u8>,
    }

    impl Image {
        fn new(base: u32, size: usize) -> Self {
            Self { base, data: vec![0; size] }
        }

        fn word(&mut self, address: u32, value: u32) {
            let offset = (address - self.base) as usize;
            self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn string(&mut self, address: u32, value: &str) {
            let offset = (address - self.base) as usize;
            self.data[offset..offset + value.len()].copy_from_slice(value.as_bytes());
        }
    }

    impl ByteRead for Image {
        fn read_bytes(&self, address: u32, result: &mut [u8]) -> Result<usize> {
            let offset = (address - self.base) as usize;
            let available = self.data.len() - offset;
            let length = result.len().min(available);

            result[..length].copy_from_slice(&self.data[offset..offset + length]);

            Ok(length)
        }
    }

    /// The shape of Legend of Master's `f`, cut down to two fields and two
    /// methods: a member count that is neither table's length, a field table
    /// that runs to the method table, and a method table with its own count.
    fn legend_of_master_shaped_class() -> (Image, u32) {
        let base = 0x1000;
        let mut image = Image::new(base, 0x400);

        let metadata = 0x1100;
        let root = metadata + METADATA_SIZE;
        let fields = root + FIELD_TABLE_OFFSET;
        let methods = fields + 2 * FIELD_ROW_SIZE;

        image.string(0x1000, "f");
        image.string(0x1010, "org/kwis/msp/lcdui/Card");
        image.string(0x1040, "paint");
        image.string(0x1050, "(Lorg/kwis/msp/lcdui/Graphics;)V");
        image.string(0x1080, "run");
        image.string(0x1090, "()V");
        image.string(0x10a0, "hp");
        image.string(0x10b0, "I");
        image.string(0x10c0, "name");
        image.string(0x10d0, "Ljava/lang/String;");

        image.word(metadata + METADATA_NAME, 0x1000);
        image.word(metadata + METADATA_SUPERCLASS, 0x1010);
        // Deliberately not two, and not four: the count in the image describes
        // neither table.
        image.word(metadata + METADATA_INSTANCE_WORDS, 3);
        image.word(metadata + METADATA_METHODS, methods);
        image.word(root + 8, metadata);

        for (index, (name, descriptor)) in [(0x10a0, 0x10b0), (0x10c0, 0x10d0)].into_iter().enumerate() {
            let row = fields + index as u32 * FIELD_ROW_SIZE;
            image.word(row, root);
            image.word(row + 4, name);
            image.word(row + 8, descriptor);
            image.word(row + FIELD_SLOT_OFFSET, index as u32);
        }

        image.word(methods, 2);
        for (index, (name, descriptor, entry, argument_words)) in [(0x1040, 0x1050, 0x91ebc, 2), (0x1080, 0x1090, 0x939cc, 1)].into_iter().enumerate()
        {
            let row = methods + 4 + index as u32 * METHOD_ROW_SIZE;
            image.word(row, root);
            image.word(row + 4, name);
            image.word(row + 8, descriptor);
            image.word(row + 0xc, argument_words << 16);
            image.word(row + METHOD_ENTRY_OFFSET, entry);
        }

        (image, root)
    }

    #[test]
    fn reads_both_member_tables() {
        let (image, root) = legend_of_master_shaped_class();
        let class = parse_class(&image, root).unwrap();

        assert_eq!(class.name, "f");
        assert_eq!(class.superclass.as_deref(), Some("org/kwis/msp/lcdui/Card"));

        let methods = class.methods().collect::<Vec<_>>();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].name(), "paint");
        assert_eq!(methods[1].name(), "run");

        assert_eq!(class.members.len() - methods.len(), 2);
    }

    /// Instance allocation size must not bound either member-table walk.
    #[test]
    fn ignores_the_instance_word_count_when_parsing_members() {
        let (image, root) = legend_of_master_shaped_class();
        let class = parse_class(&image, root).unwrap();

        assert_eq!(class.instance_words, 3);
        assert_eq!(class.members.len(), 4);
        assert!(class.methods().any(|x| x.name() == "paint"));
    }

    /// Most application classes carry no tables at all, which is a class with
    /// no members rather than a parse failure.
    #[test]
    fn accepts_a_class_without_tables() {
        let base = 0x1000;
        let mut image = Image::new(base, 0x400);

        let metadata = 0x1100;
        let root = metadata + METADATA_SIZE;

        image.string(0x1000, "b");
        image.word(metadata + METADATA_NAME, 0x1000);
        image.word(metadata + METADATA_INSTANCE_WORDS, 111);
        image.word(root + 8, metadata);

        let class = parse_class(&image, root).unwrap();

        assert_eq!(class.name, "b");
        assert!(class.members.is_empty());
    }

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
            flags: 0,
            row: 0,
            slot: 0,
            entry: 0x1118,
            argument_words: 2,
        };
        assert!(start_app.is_instance_method());

        let pause_app = AppMember::Method {
            name: "pauseApp".to_string(),
            descriptor: "()V".to_string(),
            flags: 0,
            row: 0,
            slot: 0,
            entry: 0x1248,
            argument_words: 1,
        };
        assert!(pause_app.is_instance_method());

        let static_like = AppMember::Method {
            name: "main".to_string(),
            descriptor: "()V".to_string(),
            flags: 0,
            row: 0,
            slot: 0,
            entry: 0x2000,
            argument_words: 0,
        };
        assert!(!static_like.is_instance_method());
    }
}
