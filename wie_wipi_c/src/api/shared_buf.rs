//! `MC_knlCreateSharedBuf` and friends - the platform's named scratch memory.
//!
//! A WIPI title suite is more than one program, and a shared buffer is how they
//! pass each other data: one creates a buffer under a name, another asks for
//! the same name and gets the same memory. Within one emulated run there is
//! only ever one program, so "shared" here means what it means to that
//! program - a named allocation it can find again, resize and destroy.
//!
//! The memory is an ordinary guest allocation, so a buffer's address is
//! something the title can hand to any other WIPI call.

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec};

use spin::Mutex;

use wie_util::{Result, read_null_terminated_string_bytes};

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// A buffer's guest allocation and the size the title asked for.
///
/// The allocation can be larger than `size` after a shrink, which costs a
/// little memory and saves moving the contents.
#[derive(Clone, Copy)]
struct SharedBuf {
    address: WIPICWord,
    allocated: WIPICWord,
    size: WIPICWord,
}

#[derive(Default)]
pub struct SharedBufState {
    buffers: BTreeMap<String, SharedBuf>,
}

pub type SharedSharedBufState = Arc<Mutex<SharedBufState>>;

pub fn new_state() -> SharedSharedBufState {
    Arc::new(Mutex::new(SharedBufState::default()))
}

/// Reads a buffer name, refusing the ones the platform cannot key on.
fn read_name(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<Option<String>> {
    if ptr_name == 0 {
        return Ok(None);
    }

    let bytes = read_null_terminated_string_bytes(context, ptr_name)?;
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(String::from_utf8(bytes).ok())
}

/// `MC_knlCreateSharedBuf` - make a named buffer, or find the one that is
/// already there.
///
/// Answers its address, or zero. A name that already exists is answered with
/// the existing buffer rather than a second one, which is what makes the two
/// programs meet: the second caller is the one that finds it.
pub async fn create_shared_buf(context: &mut dyn WIPICContext, ptr_name: WIPICWord, size: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_knlCreateSharedBuf({ptr_name:#x}, {size:#x})");

    let Some(name) = read_name(context, ptr_name)? else {
        return Ok(0);
    };

    if size == 0 {
        return Ok(0);
    }

    if let Some(existing) = context.shared_buf_state().lock().buffers.get(&name) {
        return Ok(existing.address);
    }

    let memory = context.alloc(size)?;
    let address = context.data_ptr(memory)?;

    // A buffer a title has just made and not yet written to should read as
    // zeroes, not as whatever the last owner of the block left there.
    context.write_bytes(address, &vec![0u8; size as usize])?;

    context.shared_buf_state().lock().buffers.insert(
        name,
        SharedBuf {
            address,
            allocated: size,
            size,
        },
    );

    Ok(address)
}

/// `MC_knlGetSharedBuf` - the address of a buffer someone already created.
pub async fn get_shared_buf(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_knlGetSharedBuf({ptr_name:#x})");

    let Some(name) = read_name(context, ptr_name)? else {
        return Ok(0);
    };

    Ok(context.shared_buf_state().lock().buffers.get(&name).map(|x| x.address).unwrap_or(0))
}

/// `MC_knlGetSharedBufSize` - how big it is, or zero if there is no such
/// buffer.
pub async fn get_shared_buf_size(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_knlGetSharedBufSize({ptr_name:#x})");

    let Some(name) = read_name(context, ptr_name)? else {
        return Ok(0);
    };

    Ok(context.shared_buf_state().lock().buffers.get(&name).map(|x| x.size).unwrap_or(0))
}

/// `MC_knlResizeSharedBuf` - change the size, keeping what is already there.
///
/// Answers the address, which can move when the buffer grows. Shrinking keeps
/// the allocation and only lowers the size, so an address a title is already
/// holding stays good.
pub async fn resize_shared_buf(context: &mut dyn WIPICContext, ptr_name: WIPICWord, size: WIPICWord) -> Result<WIPICWord> {
    tracing::debug!("MC_knlResizeSharedBuf({ptr_name:#x}, {size:#x})");

    let Some(name) = read_name(context, ptr_name)? else {
        return Ok(0);
    };

    if size == 0 {
        return Ok(0);
    }

    let Some(existing) = context.shared_buf_state().lock().buffers.get(&name).copied() else {
        return Ok(0);
    };

    if size <= existing.allocated {
        context.shared_buf_state().lock().buffers.insert(name, SharedBuf { size, ..existing });

        return Ok(existing.address);
    }

    // Growing needs a new block; carry the old contents over and let the old
    // one go.
    let memory = context.alloc(size)?;
    let address = context.data_ptr(memory)?;
    context.write_bytes(address, &vec![0u8; size as usize])?;

    let mut carried = vec![0u8; existing.size as usize];
    context.read_bytes(existing.address, &mut carried)?;
    context.write_bytes(address, &carried)?;

    context.shared_buf_state().lock().buffers.insert(
        name,
        SharedBuf {
            address,
            allocated: size,
            size,
        },
    );

    Ok(address)
}

/// `MC_knlDestroySharedBuf` - forget the name and give the memory back.
pub async fn destroy_shared_buf(context: &mut dyn WIPICContext, ptr_name: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_knlDestroySharedBuf({ptr_name:#x})");

    let Some(name) = read_name(context, ptr_name)? else {
        return Ok(-1);
    };

    let removed = context.shared_buf_state().lock().buffers.remove(&name);

    match removed {
        Some(_) => Ok(0),
        None => Ok(-1),
    }
}

#[cfg(test)]
mod tests {
    use wie_util::{ByteRead, ByteWrite};

    use crate::context::test::TestContext;

    use super::{create_shared_buf, destroy_shared_buf, get_shared_buf, get_shared_buf_size, resize_shared_buf};

    async fn named(context: &mut TestContext, address: u32, name: &[u8]) -> u32 {
        let mut bytes = name.to_vec();
        bytes.push(0);
        context.write_bytes(address, &bytes).unwrap();

        address
    }

    /// The point of the API: the second caller finds what the first made.
    #[futures_test::test]
    async fn a_second_caller_finds_the_same_buffer() {
        let mut context = TestContext::new();
        let name = named(&mut context, 0x1000, b"save").await;

        let first = create_shared_buf(&mut context, name, 64).await.unwrap();
        assert_ne!(first, 0);

        assert_eq!(create_shared_buf(&mut context, name, 64).await.unwrap(), first);
        assert_eq!(get_shared_buf(&mut context, name).await.unwrap(), first);
        assert_eq!(get_shared_buf_size(&mut context, name).await.unwrap(), 64);
    }

    /// A different name is a different buffer.
    #[futures_test::test]
    async fn different_names_do_not_meet() {
        let mut context = TestContext::new();
        let one = named(&mut context, 0x1000, b"one").await;
        let two = named(&mut context, 0x1100, b"two").await;

        let first = create_shared_buf(&mut context, one, 32).await.unwrap();
        let second = create_shared_buf(&mut context, two, 32).await.unwrap();

        assert_ne!(first, second);
    }

    /// A fresh buffer reads as zeroes rather than as whatever was in the block.
    #[futures_test::test]
    async fn a_new_buffer_starts_empty() {
        let mut context = TestContext::new();
        let name = named(&mut context, 0x1000, b"fresh").await;

        let address = create_shared_buf(&mut context, name, 16).await.unwrap();

        let mut read = [0xffu8; 16];
        context.read_bytes(address, &mut read).unwrap();
        assert_eq!(read, [0u8; 16]);
    }

    /// Growing keeps what was written, and shrinking leaves the address alone.
    #[futures_test::test]
    async fn resizing_keeps_the_contents() {
        let mut context = TestContext::new();
        let name = named(&mut context, 0x1000, b"grow").await;

        let address = create_shared_buf(&mut context, name, 8).await.unwrap();
        context.write_bytes(address, b"12345678").unwrap();

        let grown = resize_shared_buf(&mut context, name, 32).await.unwrap();
        assert_ne!(grown, 0);
        assert_eq!(get_shared_buf_size(&mut context, name).await.unwrap(), 32);

        let mut read = [0u8; 8];
        context.read_bytes(grown, &mut read).unwrap();
        assert_eq!(&read, b"12345678");

        // Shrinking within what is already allocated keeps the address, so one
        // the title is holding stays good.
        assert_eq!(resize_shared_buf(&mut context, name, 4).await.unwrap(), grown);
        assert_eq!(get_shared_buf_size(&mut context, name).await.unwrap(), 4);
    }

    /// Destroying forgets the name; asking again finds nothing.
    #[futures_test::test]
    async fn destroying_forgets_the_name() {
        let mut context = TestContext::new();
        let name = named(&mut context, 0x1000, b"gone").await;

        create_shared_buf(&mut context, name, 16).await.unwrap();

        assert_eq!(destroy_shared_buf(&mut context, name).await.unwrap(), 0);
        assert_eq!(get_shared_buf(&mut context, name).await.unwrap(), 0);
        assert_eq!(get_shared_buf_size(&mut context, name).await.unwrap(), 0);

        // And a second destroy says there was nothing to destroy.
        assert_eq!(destroy_shared_buf(&mut context, name).await.unwrap(), -1);
    }

    /// A title that passes no name, or asks for nothing, is answered rather
    /// than aborted - which is the whole point of these existing at all.
    #[futures_test::test]
    async fn a_missing_name_or_size_is_answered() {
        let mut context = TestContext::new();
        let name = named(&mut context, 0x1000, b"zero").await;

        assert_eq!(create_shared_buf(&mut context, 0, 16).await.unwrap(), 0);
        assert_eq!(create_shared_buf(&mut context, name, 0).await.unwrap(), 0);
        assert_eq!(get_shared_buf(&mut context, 0).await.unwrap(), 0);
        assert_eq!(resize_shared_buf(&mut context, name, 16).await.unwrap(), 0);
        assert_eq!(destroy_shared_buf(&mut context, 0).await.unwrap(), -1);
    }
}
