use alloc::{borrow::ToOwned, boxed::Box, str, string::String, vec, vec::Vec};
use core::mem::size_of;

use bytemuck::{Pod, Zeroable};

use wipi_types::wipic::WIPICWord;

use wie_backend::Database;
use wie_util::{Result, read_generic, read_null_terminated_string_bytes, write_generic};

use crate::context::WIPICContext;

/// Per-handle state for KTF's stream-style database API.
///
/// KTF's `stream_read` / `stream_write` slots behave like a record-scoped
/// `fread` / `fwrite` pair rather than the standard WIPI record-by-id API
/// — the same record id 1 is walked sequentially with implicit cursors.
/// The original interface field names (`read_record_single`,
/// `write_record_single`) were a pre-disassembly guess; the impl-side names
/// `stream_read` / `stream_write` reflect the verified semantics.
///
/// The handle, including its read/write cursors and the in-memory mirror
/// of record 1, lives entirely in emulated memory: the `DatabaseHandle`
/// struct sits at the pointer returned from `open_database`, and the
/// mirror itself is a separate guest-heap allocation referenced by
/// `buffer_ptr`. Every op reads the struct, mutates it, writes it back —
/// no host-side global state.
///
/// `select_record` with a non-zero recid is treated as a seek: KTF apps
/// use slot 4 to position the cursor at known byte offsets within the
/// single backing record, e.g. for multi-slot save files.
#[derive(Pod, Zeroable, Copy, Clone)]
#[repr(C)]
struct DatabaseHandle {
    magic: u32,
    name: [u8; 32], // TODO hardcoded max size
    read_cursor: u32,
    write_cursor: u32,
    buffer_ptr: u32,
    buffer_len: u32,
    buffer_capacity: u32,
}

const MIN_BUFFER_CAPACITY: u32 = 64;
// "MCDB" — sentinel at the start of the handle struct so we can distinguish
// a real DB handle pointer from an unrelated guest pointer (e.g. a C-string
// name pointer that KTF's slot 6 passes through the same SVC argument slot).
const DATABASE_HANDLE_MAGIC: u32 = 0x4D434442;
const MAX_NAME_LEN: usize = 31; // leave a byte for null terminator inside the 32-byte field

// LGT's native database keeps fixed-record metadata in the companion `.idx` file.
// WIE's repository only exposes numbered records, so reserve backend record 0 for
// the LGT-only metadata that must survive close/reopen. Backend allocation starts at
// record 1, and the generic/KTF paths never use record 0 as a normal data record.
const LGT_METADATA_RECORD_ID: u32 = 0;
const LGT_METADATA_MAGIC: u32 = 0x4C475444; // "LGTD"
const LGT_METADATA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LgtDatabaseMetadata {
    record_size: u32,
    next_record_id: u32,
    active_count: u32,
    free_ids: Vec<u32>,
}

impl LgtDatabaseMetadata {
    fn new(record_size: u32) -> Self {
        Self {
            record_size,
            next_record_id: 1,
            active_count: 0,
            free_ids: Vec::new(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(20 + self.free_ids.len() * 4);
        data.extend_from_slice(&LGT_METADATA_MAGIC.to_le_bytes());
        data.extend_from_slice(&LGT_METADATA_VERSION.to_le_bytes());
        data.extend_from_slice(&self.record_size.to_le_bytes());
        data.extend_from_slice(&self.next_record_id.to_le_bytes());
        data.extend_from_slice(&self.active_count.to_le_bytes());
        data.extend_from_slice(&(self.free_ids.len() as u32).to_le_bytes());
        for id in &self.free_ids {
            data.extend_from_slice(&id.to_le_bytes());
        }
        data
    }

    fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }

        let word = |offset: usize| -> u32 {
            u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
        };

        if word(0) != LGT_METADATA_MAGIC || word(4) != LGT_METADATA_VERSION {
            return None;
        }

        let free_count = word(20) as usize;
        let expected_len = 24usize.checked_add(free_count.checked_mul(4)?)?;
        if data.len() != expected_len {
            return None;
        }

        let mut free_ids = Vec::with_capacity(free_count);
        for offset in (24..expected_len).step_by(4) {
            free_ids.push(word(offset));
        }

        Some(Self {
            record_size: word(8),
            next_record_id: word(12),
            active_count: word(16),
            free_ids,
        })
    }
}

async fn load_lgt_metadata(db: &mut dyn Database) -> Option<LgtDatabaseMetadata> {
    let data = db.get(LGT_METADATA_RECORD_ID).await?;
    LgtDatabaseMetadata::decode(&data)
}

async fn store_lgt_metadata(db: &mut dyn Database, metadata: &LgtDatabaseMetadata) -> bool {
    db.set(LGT_METADATA_RECORD_ID, &metadata.encode()).await
}

pub async fn open_database(context: &mut dyn WIPICContext, ptr_name: WIPICWord, mode: i32, r#type: i32) -> Result<i32> {
    tracing::debug!("MC_dbOpenDataBase({ptr_name:#x}, {mode}, {type})");

    // Guest-provided C string — invalid UTF-8 must not bring down the
    // emulator. Treat it as a bad parameter and return -22, matching the
    // fail-soft behaviour of the other name-keyed entry points in this
    // file (`stat_by_name_ktf`, `exists_database_ktf`).
    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        tracing::warn!("MC_dbOpenDataBase: invalid utf8 name @ {ptr_name:#x}");
        return Ok(-22);
    };

    // Validate before any repository side effects. Mode 4 deletes record 1
    // up front, so a too-long name reaching that path would wipe data we
    // can't open a handle for anyway.
    if name.len() > MAX_NAME_LEN {
        tracing::warn!("MC_dbOpenDataBase: name {name:?} too long ({} > {MAX_NAME_LEN})", name.len());
        return Ok(-22); // M_E_BADRECID — closest WIPI parameter-error idiom in this file
    }

    let packaged = read_packaged_database(context, &name).await?;

    let system = context.system();
    let pid = system.pid().to_owned();
    let exists = system.platform().database_repository().exists(&name, &pid).await;

    if !exists && packaged.is_none() && mode == 1 {
        return Ok(-12); // M_E_NOENT
    }

    // Mode 4 (`MC_DB_CREATE`) wipes any prior contents up front unless the
    // DB is backed by a packaged resource. Other modes seed the per-handle
    // buffer with the existing record or packaged data so seek+overlay writes
    // preserve unrelated bytes (multi-slot saves at fixed byte offsets).
    let initial: Vec<u8> = if exists {
        let mut db = system.platform().database_repository().open(&name, &pid).await;
        if mode == 4 && packaged.is_none() {
            db.delete(1).await;
            Vec::new()
        } else if let Some(data) = db.get(1).await {
            data
        } else if let Some(data) = packaged {
            db.set(1, &data).await;
            data
        } else {
            Vec::new()
        }
    } else if let Some(data) = packaged {
        let mut db = system.platform().database_repository().open(&name, &pid).await;
        db.set(1, &data).await;
        data
    } else if mode == 4 {
        system.platform().database_repository().open(&name, &pid).await;
        Vec::new()
    } else {
        Vec::new()
    };

    let name_bytes = name.as_bytes();

    let mut handle = DatabaseHandle {
        magic: DATABASE_HANDLE_MAGIC,
        name: [0; 32],
        read_cursor: 0,
        write_cursor: 0,
        buffer_ptr: 0,
        buffer_len: 0,
        buffer_capacity: 0,
    };
    handle.name[..name_bytes.len()].copy_from_slice(name_bytes);

    if !initial.is_empty() {
        let cap = (initial.len() as u32).max(MIN_BUFFER_CAPACITY);
        let buf_ptr = context.alloc_raw(cap)?;
        context.write_bytes(buf_ptr, &initial)?;
        handle.buffer_ptr = buf_ptr;
        handle.buffer_len = initial.len() as u32;
        handle.buffer_capacity = cap;
    }

    let ptr_handle = context.alloc_raw(size_of::<DatabaseHandle>() as _)?;
    write_generic(context, ptr_handle, handle)?;

    tracing::debug!("Created database handle {ptr_handle:#x} for {name}");

    Ok(ptr_handle as _)
}

/// LGT canonical `MC_dbOpenDataBase` (service 0x1f4).
///
/// Native ABI:
/// `MC_dbOpenDataBase(name, record_size, create, access)`.
///
/// Verified native contract:
/// - null `name` -> -9;
/// - `create == 0` requires the database to exist, otherwise -12;
/// - `create == 1` permits creation, but requires `record_size > 0`,
///   otherwise -9;
/// - filesystem access must be 1, 2 or 3, otherwise -9;
/// - the underlying native mode-8 file open preserves an existing database
///   and creates the backing files only when absent.
///
/// WIE's database repository exposes one logical database instead of the
/// native `.db` / `.idx` pair.  For the valid native create/open forms we
/// reproduce the externally visible existence/create behaviour and then use
/// the existing WIE database handle implementation.  Native record-size
/// metadata is not representable by the current repository interface.
pub async fn open_database_lgt(
    context: &mut dyn WIPICContext,
    ptr_name: WIPICWord,
    record_size: i32,
    create: i32,
    access: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_dbOpenDataBase({ptr_name:#x}, record_size={record_size}, create={create}, access={access})"
    );

    if ptr_name == 0 {
        return Ok(-9);
    }

    if !matches!(access, 1..=3) {
        return Ok(-9);
    }

    if create == 1 && record_size <= 0 {
        return Ok(-9);
    }

    // Native builds the `.db` / `.idx` paths with sprintf/strcat and performs
    // no explicit database-name length or encoding validation here. WIE stores
    // the logical database name in a fixed 32-byte guest handle and uses a
    // UTF-8 host repository key, so these two checks are safety adaptations
    // rather than native MC_dbOpenDataBase error semantics.
    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        return Ok(-22);
    };

    if name.len() > MAX_NAME_LEN {
        return Ok(-22);
    }

    let packaged = read_packaged_database(context, &name).await?;
    let system = context.system();
    let pid = system.pid().to_owned();
    let exists = system
        .platform()
        .database_repository()
        .exists(&name, &pid)
        .await;

    if !exists && packaged.is_none() {
        if create == 0 {
            return Ok(-12);
        }

        if create == 1 {
            // Native MC_fsOpen mode 8 first opens read/write and, on ENOENT,
            // retries with O_RDWR | O_CREAT.  Opening the WIE repository is
            // the logical equivalent of materializing that backing store.
            system
                .platform()
                .database_repository()
                .open(&name, &pid)
                .await;
        }
    }

    // Native persists record size, next id, active count and the free-list in
    // the `.idx` file. Preserve the equivalent state in reserved backend record
    // 0. Once present, persisted metadata wins over a later caller-supplied
    // record_size, matching native reopen semantics.
    {
        let mut repository_db = system
            .platform()
            .database_repository()
            .open(&name, &pid)
            .await;

        if load_lgt_metadata(repository_db.as_mut()).await.is_none() {
            let effective_record_size = if record_size > 0 {
                record_size as u32
            } else {
                // A legacy WIE repository may predate LGT metadata. Native
                // would have the size in `.idx`; the closest migration source
                // available here is an existing positive record.
                let ids = repository_db.get_record_ids().await;
                let mut derived = 0u32;
                for id in ids {
                    if id == LGT_METADATA_RECORD_ID {
                        continue;
                    }
                    if let Some(data) = repository_db.get(id).await {
                        derived = derived.max(data.len() as u32);
                    }
                }
                derived
            };

            if effective_record_size > 0 {
                let mut positive_ids: Vec<u32> = repository_db
                    .get_record_ids()
                    .await
                    .into_iter()
                    .filter(|&id| id != LGT_METADATA_RECORD_ID)
                    .collect();
                positive_ids.sort_unstable();

                let next_record_id = positive_ids
                    .last()
                    .copied()
                    .and_then(|id| id.checked_add(1))
                    .unwrap_or(1);

                let mut metadata = LgtDatabaseMetadata::new(effective_record_size);
                metadata.next_record_id = next_record_id;
                metadata.active_count = positive_ids.len() as u32;

                // Exact historic deletion order cannot be reconstructed from a
                // legacy repository that never stored it. Leave the migrated
                // free-list empty rather than inventing an ordering.
                if !store_lgt_metadata(repository_db.as_mut(), &metadata).await {
                    return Ok(-1);
                }
            }
        }
    }

    // Mode 0 in the existing WIE helper preserves existing contents.  The
    // LGT wrapper has already performed the native create/existence checks,
    // so this does not inherit the KTF-oriented mode interpretation.
    open_database(context, ptr_name, 0, access).await
}

/// LGT canonical `MC_dbInsertRecord` (service 0x1f7).
///
/// Native ABI: `MC_dbInsertRecord(handle, data, length)`.
///
/// Verified native contract:
/// - unknown handle -> -2;
/// - null data or non-positive length -> -9;
/// - length larger than the database's fixed record size -> -21;
/// - deleted record ids are reused in LIFO order;
/// - otherwise `next_record_id` is allocated and incremented;
/// - short input is zero-padded to exactly the fixed record size;
/// - success returns the allocated positive record id;
/// - persistence failures collapse to -1.
pub async fn insert_record_lgt(
    context: &mut dyn WIPICContext,
    db_id: i32,
    buf_ptr: WIPICWord,
    buf_len: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_dbInsertRecord({db_id:#x}, {buf_ptr:#x}, {buf_len}) [LGT]"
    );

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-2);
    };

    if buf_ptr == 0 || buf_len <= 0 {
        return Ok(-9);
    }

    let Some(mut repository_db) = open_db_for_handle(context, &handle).await else {
        return Ok(-2);
    };

    let Some(mut metadata) = load_lgt_metadata(repository_db.as_mut()).await else {
        return Ok(-1);
    };

    let buf_len = buf_len as u32;
    if buf_len > metadata.record_size {
        return Ok(-21);
    }

    let record_id = if let Some(id) = metadata.free_ids.pop() {
        id
    } else {
        let id = metadata.next_record_id;
        let Some(next) = id.checked_add(1) else {
            return Ok(-1);
        };
        metadata.next_record_id = next;
        id
    };

    if record_id == 0 {
        return Ok(-1);
    }

    let mut record = vec![0u8; metadata.record_size as usize];
    context.read_bytes(buf_ptr, &mut record[..buf_len as usize])?;

    if !repository_db.set(record_id, &record).await {
        return Ok(-1);
    }

    let Some(active_count) = metadata.active_count.checked_add(1) else {
        return Ok(-1);
    };
    metadata.active_count = active_count;

    if !store_lgt_metadata(repository_db.as_mut(), &metadata).await {
        // Native may already have written the data record before failing while
        // updating `.idx`; preserve that partial-write ordering rather than
        // rolling the record back.
        return Ok(-1);
    }

    Ok(record_id as i32)
}

/// LGT canonical `MC_dbSelectRecord` (service 0x1f8).
///
/// Native ABI: `MC_dbSelectRecord(handle, record_id, buffer, length)`.
///
/// Verified native contract:
/// - unknown handle -> -2;
/// - null buffer or non-positive length -> -9;
/// - length smaller than the database fixed record size -> -18;
/// - record id at/after `next_record_id` -> -22;
/// - a record id present in the persisted free-list -> -22;
/// - seek/read failure -> -1;
/// - successful reads return 0, including a short read at physical EOF.
///
/// The native implementation seeks to `(record_id - 1) * record_size` in one
/// contiguous `.db` file and then reads the caller's full requested length.
/// WIE stores fixed records as separate backend entries, so reproduce that raw
/// byte-stream view by concatenating successive record slots. Only the starting
/// id is checked against the free-list, matching the native pre-read validation.
/// LGT canonical `MC_dbDeleteRecord` (service 0x1fa).
///
/// Native ABI: `MC_dbDeleteRecord(handle, record_id)`.
///
/// Verified native contract:
/// - unknown handle -> -2;
/// - record id at/after `next_record_id` -> -22;
/// - negative record id -> -1;
/// - record id 0 is not rejected and is handled like any other non-negative id;
/// - an id already present in the free-list -> -22;
/// - deletion appends the id to the free-list without touching the `.db` payload;
/// - active count is decremented with native 32-bit wrapping arithmetic;
/// - index persistence/allocation failures collapse to -1;
/// - success returns 0.
///
/// The native implementation also refreshes its internal modification timestamp.
/// As with `MC_dbUpdateRecord`, that header field is not exposed by the audited DB
/// exports, so the repository adaptation keeps the existing metadata format.
pub async fn delete_record_lgt(
    context: &mut dyn WIPICContext,
    db_id: i32,
    rec_id: i32,
) -> Result<i32> {
    tracing::debug!("MC_dbDeleteRecord({db_id:#x}, {rec_id}) [LGT]");

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-2);
    };

    let Some(mut repository_db) = open_db_for_handle(context, &handle).await else {
        return Ok(-2);
    };

    let Some(mut metadata) = load_lgt_metadata(repository_db.as_mut()).await else {
        return Ok(-1);
    };

    // Native tests the upper bound before testing for a negative id.
    if (metadata.next_record_id as i32) <= rec_id {
        return Ok(-22);
    }

    // Native deliberately permits record id zero here.
    if rec_id < 0 {
        return Ok(-1);
    }

    let record_id = rec_id as u32;
    if metadata.free_ids.iter().any(|&id| id == record_id) {
        return Ok(-22);
    }

    // Native deletion is index-only. The physical `.db` bytes remain intact.
    // Preserving the backend slot is required both for SelectRecord over-read
    // semantics and for later LIFO reuse by InsertRecord.
    metadata.free_ids.push(record_id);
    metadata.active_count = metadata.active_count.wrapping_sub(1);

    if !store_lgt_metadata(repository_db.as_mut(), &metadata).await {
        return Ok(-1);
    }

    Ok(0)
}

/// LGT canonical `MC_dbUpdateRecord` (service 0x1f9).
///
/// Native ABI: `MC_dbUpdateRecord(handle, record_id, buffer, length)`.
///
/// Verified native contract:
/// - unknown handle -> -2;
/// - null buffer or non-positive length -> -9;
/// - length larger than the fixed record size -> -21;
/// - record id <= 0 or at/after `next_record_id` -> -22;
/// - a record id present in the persisted free-list -> -9;
/// - short updates overwrite only the supplied prefix and preserve the tail;
/// - seek/write or metadata/header persistence failures collapse to -1;
/// - success returns 0.
///
/// Native also refreshes an internal 64-bit modification timestamp in the
/// `.idx` header. No exported DB operation inspected so far exposes that field,
/// so the WIE repository adaptation intentionally does not widen the existing
/// persistent metadata format solely for this inert native bookkeeping field.
pub async fn update_record_lgt(
    context: &mut dyn WIPICContext,
    db_id: i32,
    rec_id: i32,
    buf_ptr: WIPICWord,
    buf_len: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_dbUpdateRecord({db_id:#x}, {rec_id}, {buf_ptr:#x}, {buf_len}) [LGT]"
    );

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-2);
    };

    if buf_ptr == 0 || buf_len <= 0 {
        return Ok(-9);
    }

    let Some(mut repository_db) = open_db_for_handle(context, &handle).await else {
        return Ok(-2);
    };

    let Some(metadata) = load_lgt_metadata(repository_db.as_mut()).await else {
        return Ok(-1);
    };

    let buf_len_u32 = buf_len as u32;
    if buf_len_u32 > metadata.record_size {
        return Ok(-21);
    }

    // Native performs both bounds checks before consulting the free-list.
    if rec_id <= 0 || (metadata.next_record_id as i32) <= rec_id {
        return Ok(-22);
    }

    if metadata.free_ids.iter().any(|&id| id == rec_id as u32) {
        return Ok(-9);
    }

    let record_id = rec_id as u32;
    let Some(mut record) = repository_db.get(record_id).await else {
        return Ok(-1);
    };

    // Canonical LGT inserts materialize exactly fixed-size slots. A malformed
    // shorter backend record cannot reproduce the native fixed-file seek/write
    // safely, so treat it as the same generic persistence failure.
    if record.len() < metadata.record_size as usize {
        return Ok(-1);
    }

    let mut update = vec![0u8; buf_len as usize];
    context.read_bytes(buf_ptr, &mut update)?;
    record[..update.len()].copy_from_slice(&update);

    if !repository_db.set(record_id, &record).await {
        return Ok(-1);
    }

    Ok(0)
}

pub async fn select_record_lgt(
    context: &mut dyn WIPICContext,
    db_id: i32,
    rec_id: i32,
    buf_ptr: WIPICWord,
    buf_len: i32,
) -> Result<i32> {
    tracing::debug!(
        "MC_dbSelectRecord({db_id:#x}, {rec_id}, {buf_ptr:#x}, {buf_len}) [LGT]"
    );

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-2);
    };

    if buf_ptr == 0 || buf_len <= 0 {
        return Ok(-9);
    }

    let Some(mut repository_db) = open_db_for_handle(context, &handle).await else {
        return Ok(-2);
    };

    let Some(metadata) = load_lgt_metadata(repository_db.as_mut()).await else {
        return Ok(-1);
    };

    let buf_len_u32 = buf_len as u32;
    if buf_len_u32 < metadata.record_size {
        return Ok(-18);
    }

    // Native performs a signed comparison: next_record_id <= record_id.
    if (metadata.next_record_id as i32) <= rec_id {
        return Ok(-22);
    }

    if metadata.free_ids.iter().any(|&id| id == rec_id as u32) {
        return Ok(-22);
    }

    // Native has no explicit lower-bound test. For the ordinary zero/negative
    // ids, `(record_id - 1) * record_size` produces a negative SEEK_SET offset
    // and MC_fsSeek fails; preserve that externally visible result.
    let byte_offset = rec_id
        .wrapping_sub(1)
        .wrapping_mul(metadata.record_size as i32);
    if byte_offset < 0 {
        return Ok(-1);
    }

    if metadata.record_size == 0 {
        return Ok(-1);
    }

    let mut remaining = buf_len as usize;
    let mut current_id = rec_id as u32;
    let mut output = Vec::with_capacity(remaining);

    while remaining > 0 {
        if current_id >= metadata.next_record_id {
            break;
        }

        let Some(record) = repository_db.get(current_id).await else {
            // The backend entry boundary is the closest equivalent to native
            // physical EOF/read failure. With canonical LGT writes, allocated
            // slots are fixed-size and deleted slots retain their payload.
            break;
        };

        let take = remaining.min(record.len());
        output.extend_from_slice(&record[..take]);
        remaining -= take;

        if take < metadata.record_size as usize {
            break;
        }

        let Some(next_id) = current_id.checked_add(1) else {
            break;
        };
        current_id = next_id;
    }

    if !output.is_empty() {
        context.write_bytes(buf_ptr, &output)?;
    }

    Ok(0)
}

/// LGT canonical `MC_dbCloseDataBase` (service 0x1f5).
///
/// Native first searches the global database-handle list. A handle not found
/// there returns -2. On success the native implementation flushes its header
/// and index state, closes both backing files, removes the handle from the
/// global list, frees it, and returns 0.
///
/// WIE keeps its logical database buffer write-through, so there is no pending
/// native-style `.db` / `.idx` metadata to flush here.
pub async fn close_database_lgt(context: &mut dyn WIPICContext, db_id: i32) -> Result<i32> {
    tracing::debug!("MC_dbCloseDataBase({db_id:#x}) [LGT]");

    let Some(mut handle) = load_handle(context, db_id)? else {
        return Ok(-2);
    };

    if handle.buffer_ptr != 0 && handle.buffer_capacity > 0 {
        context.free_raw(handle.buffer_ptr, handle.buffer_capacity)?;
    }

    // Native removes the handle from its global database list before freeing
    // it. WIE has no equivalent host-side list, so invalidate the guest
    // sentinel first; otherwise freed memory can still look like a live
    // DatabaseHandle on a repeated close.
    handle.magic = 0;
    write_generic(context, db_id as _, handle)?;
    context.free_raw(db_id as _, size_of::<DatabaseHandle>() as _)?;

    Ok(0)
}

pub async fn close_database(context: &mut dyn WIPICContext, db_id: i32) -> Result<i32> {
    tracing::debug!("MC_dbCloseDataBase({db_id:#x})");

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };

    // The buffer was kept in sync with disk via write-through on every
    // `stream_write`, so close just frees the guest-heap allocations.
    if handle.buffer_ptr != 0 && handle.buffer_capacity > 0 {
        context.free_raw(handle.buffer_ptr, handle.buffer_capacity)?;
    }
    context.free_raw(db_id as _, size_of::<DatabaseHandle>() as _)?;

    Ok(0) // success
}

pub async fn list_record(context: &mut dyn WIPICContext, db_id: i32, buf_ptr: WIPICWord, buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_dbListRecords({db_id:#x}, {buf_ptr:#x}, {buf_len})");

    let Some(db) = get_database_from_db_id(context, db_id).await? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };
    let ids = db.get_record_ids().await;

    let mut cursor = 0;
    for &id in &ids {
        write_generic(context, buf_ptr + cursor, id)?;
        cursor += size_of::<WIPICWord>() as u32;
    }

    Ok(ids.len() as _)
}

pub async fn seek_record_single(context: &mut dyn WIPICContext, db_id: i32, offset: i32, origin: i32) -> Result<i32> {
    tracing::debug!("MC_dbSeekRecordSingle({db_id:#x}, {offset}, {origin})");

    let Some(mut handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };

    let base = match origin {
        0 => 0,
        1 => handle.read_cursor as i64,
        2 => handle.buffer_len as i64,
        _ => return Ok(-1),
    };
    let position = (base + offset as i64).clamp(0, handle.buffer_len as i64) as u32;
    handle.read_cursor = position;
    handle.write_cursor = position;
    write_generic(context, db_id as _, handle)?;

    Ok(position as i32)
}

pub async fn list_record_info(context: &mut dyn WIPICContext, ptr_name: WIPICWord, buf_ptr: WIPICWord, capacity: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_dbListRecordInfo({ptr_name:#x}, {buf_ptr:#x}, {capacity})");

    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        return Ok(-22);
    };
    let system = context.system();
    let pid = system.pid().to_owned();

    if !system.platform().database_repository().exists(&name, &pid).await {
        if let Some(data) = read_packaged_database(context, &name).await? {
            if capacity > 0 {
                write_generic(context, buf_ptr, 1u32)?;
                write_generic(context, buf_ptr + 4, 0u32)?;
                write_generic(context, buf_ptr + 8, data.len() as u32)?;
            }
            return Ok(0);
        }
        return Ok(-12); // M_E_NOENT
    }

    let db = system.platform().database_repository().open(&name, &pid).await;
    let ids = db.get_record_ids().await;

    let mut written = 0;
    for id in ids {
        if written >= capacity {
            break;
        }

        let Some(data) = db.get(id).await else {
            continue;
        };

        let entry_ptr = buf_ptr + written * 12;
        write_generic(context, entry_ptr, id)?;
        write_generic(context, entry_ptr + 4, 0u32)?;
        write_generic(context, entry_ptr + 8, data.len() as u32)?;
        written += 1;
    }

    Ok(0)
}

pub async fn exists_database(context: &mut dyn WIPICContext, ptr_name: WIPICWord, r#type: i32) -> Result<i32> {
    tracing::debug!("MC_dbExistsDataBase({ptr_name:#x}, {type})");

    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        return Ok(-22);
    };
    if read_packaged_database(context, &name).await?.is_some() {
        return Ok(0);
    }

    let system = context.system();
    let pid = system.pid().to_owned();
    if system.platform().database_repository().exists(&name, &pid).await {
        Ok(0)
    } else {
        Ok(-12) // M_E_NOENT
    }
}

pub async fn stream_write(context: &mut dyn WIPICContext, db_id: i32, buf_ptr: WIPICWord, buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("db.stream_write({db_id:#x}, {buf_ptr:#x}, {buf_len})");

    let Some(mut handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };

    // Cursor + len is guest-controlled, so guard the arithmetic. An
    // overflowed `new_end` would silently bypass the capacity check below
    // and let a write spill into unrelated guest memory.
    let Some(new_end) = handle.write_cursor.checked_add(buf_len) else {
        return Ok(-22); // M_E_BADRECID — closest "bad parameter" code
    };

    let old_len = handle.buffer_len;

    // Grow the guest-heap buffer if the next write would land past its
    // end. Doubling-on-demand starting from MIN_BUFFER_CAPACITY keeps the
    // realloc count amortized; alloc/free is a guest-side `WIPICContext`
    // primitive so we copy old bytes via host-side scratch.
    if new_end > handle.buffer_capacity {
        let Some(rounded) = new_end.checked_next_power_of_two() else {
            return Ok(-22);
        };
        let new_cap = rounded.max(MIN_BUFFER_CAPACITY);
        let new_ptr = context.alloc_raw(new_cap)?;
        if handle.buffer_len > 0 && handle.buffer_ptr != 0 {
            let mut old_data = vec![0u8; handle.buffer_len as usize];
            context.read_bytes(handle.buffer_ptr, &mut old_data)?;
            context.write_bytes(new_ptr, &old_data)?;
        }
        if handle.buffer_ptr != 0 && handle.buffer_capacity > 0 {
            context.free_raw(handle.buffer_ptr, handle.buffer_capacity)?;
        }
        handle.buffer_ptr = new_ptr;
        handle.buffer_capacity = new_cap;
    }

    // If the write_cursor was seeked past the prior end (e.g. via a slot 4
    // multi-slot save), the bytes between the old end and the cursor were
    // never initialised. `alloc_raw` doesn't guarantee zeroed memory and
    // the snapshot below is flushed straight to disk, so explicitly zero
    // the gap to avoid leaking heap residue into the save file. This must
    // run for `buf_len == 0` too: `new_end == write_cursor` still extends
    // `buffer_len`, so the gap would otherwise be snapshotted uninitialised.
    if handle.write_cursor > old_len {
        let gap_size = (handle.write_cursor - old_len) as usize;
        let zeros = vec![0u8; gap_size];
        context.write_bytes(handle.buffer_ptr + old_len, &zeros)?;
    }

    if buf_len > 0 {
        let mut buf = vec![0u8; buf_len as usize];
        context.read_bytes(buf_ptr, &mut buf)?;
        context.write_bytes(handle.buffer_ptr + handle.write_cursor, &buf)?;
    }

    handle.write_cursor = new_end;
    if new_end > handle.buffer_len {
        handle.buffer_len = new_end;
    }
    write_generic(context, db_id as _, handle)?;

    // Write-through to disk on every stream_write. Some titles tear down
    // the game without making a final `close_database` call after their
    // save sequence — relying on close as the only flush point loses all
    // the writes that landed since the session opened. Flushing eagerly
    // costs an extra small file write per call but keeps the on-disk state
    // consistent if the process exits or the title forgets to close.
    let mut snapshot = vec![0u8; handle.buffer_len as usize];
    if handle.buffer_ptr != 0 && handle.buffer_len > 0 {
        context.read_bytes(handle.buffer_ptr, &mut snapshot)?;
    }
    if let Some(mut db) = open_db_for_handle(context, &handle).await {
        db.set(1, &snapshot).await;
    }

    Ok(buf_len as _)
}

/// Standard WIPI `MC_dbDeleteRecord(handle, rec_id)` — delete a single
/// record by id from an open DB handle.
pub async fn delete_record(context: &mut dyn WIPICContext, db_id: i32, rec_id: i32) -> Result<i32> {
    tracing::debug!("MC_dbDeleteRecord({db_id:#x}, {rec_id})");

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };
    let Some(mut db) = open_db_for_handle(context, &handle).await else {
        return Ok(-25);
    };
    let ok = db.delete(rec_id as u32).await;
    Ok(if ok { 0 } else { -22 })
}

/// KTF reuses slot 6 with two call shapes that share the same SVC signature:
///
///  - standard WIPI: `delete_record(handle, rec_id)`
///  - KTF custom:    `(name_ptr, type)` — used as a name-keyed cleanup
///
/// Both pass two ints, so we disambiguate by reading the magic field at
/// `a0`. A real handle starts with `DATABASE_HANDLE_MAGIC`; a name pointer
/// (or anything else) does not, and we fall back to a no-op.
pub async fn delete_record_ktf(context: &mut dyn WIPICContext, a0: i32, a1: i32) -> Result<i32> {
    if load_handle(context, a0)?.is_some() {
        return delete_record(context, a0, a1).await;
    }

    // Not a real handle — KTF name-keyed form. No-op preserves saves; the
    // bytes of a name string would otherwise round-trip into the standard
    // path and silently delete record 1 of the just-saved DB.
    tracing::debug!("MC_dbDeleteRecord(name-keyed @ {a0:#x}, {a1}) -> 0 (no-op)");
    Ok(0)
}

/// LGT canonical `MC_dbDeleteDataBase` (service 0x1f6).
///
/// Native ABI: `MC_dbDeleteDataBase(name, access)`.
///
/// Verified native contract:
/// - null `name` -> -9;
/// - `access` must be 1, 2 or 3, otherwise -9;
/// - deletion is name-based and does not search the open-database handle list;
/// - the native `.db` file is removed first and `.idx` second;
/// - a missing database ultimately maps to -1;
/// - successful removal returns 0.
///
/// WIE stores a database as one logical repository entry rather than separate
/// `.db` / `.idx` files, so repository deletion is the corresponding atomic
/// operation. Open handles are deliberately not invalidated here: the native
/// function performs no database-handle lookup before unlinking its files.
pub async fn delete_database_lgt(
    context: &mut dyn WIPICContext,
    ptr_name: WIPICWord,
    access: i32,
) -> Result<i32> {
    tracing::debug!("MC_dbDeleteDataBase({ptr_name:#x}, access={access}) [LGT]");

    if ptr_name == 0 {
        return Ok(-9);
    }

    if !matches!(access, 1..=3) {
        return Ok(-9);
    }

    // Native treats the name as raw C bytes. WIE repository keys are UTF-8,
    // so invalid encoding is a host-side safety adaptation.
    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        return Ok(-22);
    };

    let system = context.system();
    let pid = system.pid().to_owned();

    if !system
        .platform()
        .database_repository()
        .exists(&name, &pid)
        .await
    {
        return Ok(-1);
    }

    if system
        .platform()
        .database_repository()
        .delete(&name, &pid)
        .await
    {
        Ok(0)
    } else {
        Ok(-1)
    }
}

pub async fn delete_database(context: &mut dyn WIPICContext, ptr_name: WIPICWord, flags: i32) -> Result<i32> {
    tracing::debug!("MC_dbDeleteDataBase({ptr_name:#x}, {flags})");

    let Ok(name) = String::from_utf8(read_null_terminated_string_bytes(context, ptr_name)?) else {
        return Ok(-22);
    };
    let system = context.system();
    let pid = system.pid().to_owned();

    let deleted = system.platform().database_repository().delete(&name, &pid).await;
    if deleted || !system.platform().database_repository().exists(&name, &pid).await {
        Ok(0)
    } else {
        Ok(-12) // M_E_NOENT
    }
}

pub async fn update_record(context: &mut dyn WIPICContext, db_id: i32, rec_id: i32, buf_ptr: WIPICWord, buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_dbUpdateRecord({db_id:#x}, {rec_id}, {buf_ptr:#x}, {buf_len})");

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };
    let Some(mut db) = open_db_for_handle(context, &handle).await else {
        return Ok(-25);
    };
    if rec_id < 0 {
        return Ok(-22);
    }
    let rec_id = rec_id as u32;
    if db.get(rec_id).await.is_none() {
        return Ok(-22);
    }

    let mut buf = vec![0; buf_len as usize];
    context.read_bytes(buf_ptr, &mut buf)?;

    if db.set(rec_id, &buf).await { Ok(0) } else { Ok(-22) }
}

pub async fn select_record(context: &mut dyn WIPICContext, db_id: i32, rec_id: i32, buf_ptr: WIPICWord, buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_dbSelectRecord({db_id:#x}, {rec_id}, {buf_ptr:#x}, {buf_len})");

    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };
    let Some(db) = open_db_for_handle(context, &handle).await else {
        return Ok(-25);
    };
    if rec_id < 0 {
        return Ok(-22);
    }

    if let Some(data) = db.get(rec_id as u32).await {
        if buf_len < data.len() as u32 {
            return Ok(-18); // M_E_SHORTBUF
        }
        context.write_bytes(buf_ptr, &data)?;
        Ok(0)
    } else {
        Ok(-22)
    }
}

pub async fn stream_read(context: &mut dyn WIPICContext, db_id: i32, buf_ptr: WIPICWord, buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("db.stream_read({db_id:#x}, {buf_ptr:#x}, {buf_len})");

    let Some(mut handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };

    if handle.read_cursor >= handle.buffer_len {
        // Don't touch buf — caller may have passed a sentinel (NULL) that
        // we shouldn't write to. Some titles do this past EOF.
        return Ok(-23); // M_E_EOF
    }

    let take = core::cmp::min(buf_len, handle.buffer_len - handle.read_cursor);
    if take == 0 {
        return Ok(0);
    }

    // Copy from the guest-heap buffer into the caller's destination via
    // host-side scratch; `WIPICContext` doesn't expose an in-guest memmove.
    let mut data = vec![0u8; take as usize];
    context.read_bytes(handle.buffer_ptr + handle.read_cursor, &mut data)?;
    context.write_bytes(buf_ptr, &data)?;

    handle.read_cursor += take;
    write_generic(context, db_id as _, handle)?;

    Ok(take as _)
}

/// KTF custom slot 4 — repurposed from standard `MC_dbSelectRecord` into a
/// stream-control op `(handle, offset, mode)` that seeks both read/write
/// cursors. The standard WIPI signature `(db_id, rec_id, buf_ptr, buf_len)`
/// is not implemented; LGT routes do not use this slot.
pub async fn select_record_ktf(context: &mut dyn WIPICContext, db_id: i32, rec_id: i32, mode: WIPICWord, _buf_len: WIPICWord) -> Result<i32> {
    tracing::debug!("MC_dbSelectRecord({db_id:#x}, {rec_id}, mode={mode:#x}, {_buf_len})");

    let Some(mut handle) = load_handle(context, db_id)? else {
        return Ok(-25); // M_E_INVALIDHANDLE
    };

    // KTF reuses slot 4 as a stream-control op `(handle, offset, mode)`. The
    // shapes observed across games:
    //
    //   - `(handle, slot_offset, 0)` — multi-slot save files store each
    //     slot at a known byte offset within record 1; this seeks both
    //     cursors so the next read/write hits the right slot while
    //     preserving the bytes belonging to the other slots.
    //   - `(handle, 0, 0)` and `(handle, 0, 2)` — rewinds both cursors.
    //     mode=0 vs 2 isn't a length and isn't truncate (truncating on
    //     mode=2 on the read path destroys a prefetched buffer during a
    //     subsequent re-open and wipes the saved record). Both are treated
    //     as plain seek-and-rewind.
    if rec_id >= 0 {
        let offset = rec_id as u32;
        handle.read_cursor = offset;
        handle.write_cursor = offset;
        write_generic(context, db_id as _, handle)?;
        return Ok(0);
    }

    Ok(-22) // M_E_BADRECID
}

/// Slot 5 — KTF custom `db_stat_by_name`. From observed call shape:
///
/// ```text
/// int32 v2[3];
/// ret = slot5(name_ptr, &v2, mode, fn_self_ptr);
/// if (ret == 0 && v2[2] > 0xC7) "valid save";
/// ```
///
/// Takes a name plus a 12-byte (3-int) output struct, and returns 0 when
/// the DB exists with a non-trivial payload. The third int is treated as a
/// size threshold (must exceed 199 bytes). We fill the struct with
/// `{0, 0, record_size}` and return 0 on hit, -22 on miss.
pub async fn stat_by_name_ktf(context: &mut dyn WIPICContext, name_ptr: WIPICWord, out_buf: WIPICWord, mode: i32, _arg3: i32) -> Result<i32> {
    let name = match read_null_terminated_string_bytes(context, name_ptr) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return Ok(-22),
        },
        Err(_) => return Ok(-22),
    };

    let system = context.system();
    let pid = system.pid().to_owned();
    let exists = system.platform().database_repository().exists(&name, &pid).await;
    if !exists {
        tracing::debug!("db.stat_by_name({name:?}, mode={mode}) -> -22 (not found)");
        return Ok(-22);
    }

    // Pull record 1's size as the "valid save" indicator the game checks
    // against 0xC7 in v2[2].
    let db = system.platform().database_repository().open(&name, &pid).await;
    let record_size = db.get(1).await.map(|x| x.len() as u32).unwrap_or(0);

    if out_buf != 0 {
        write_generic(context, out_buf, 0u32)?;
        write_generic(context, out_buf + 4, 0u32)?;
        write_generic(context, out_buf + 8, record_size)?;
    }

    tracing::debug!("db.stat_by_name({name:?}, mode={mode}) -> 0 (size={record_size})");
    Ok(0)
}

/// KTF custom slot 16 — `MC_dbExists(name)`. Observed call shape across
/// multiple titles is `(name_ptr, 1, size_hint_or_zero, callback_garbage)`.
/// Titles call it before deciding whether to take the load or fresh-init
/// path. Returning 1 unconditionally makes them try to load nonexistent
/// state on first run and trip later, so we read the C string at `a0` and
/// answer based on the real persisted state.
pub async fn exists_database_ktf(context: &mut dyn WIPICContext, name_ptr: WIPICWord, _arg1: i32, _arg2: i32) -> Result<i32> {
    let name = match read_null_terminated_string_bytes(context, name_ptr) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!("MC_dbExists invalid utf8 name @ {name_ptr:#x}, defaulting to 0");
                return Ok(0);
            }
        },
        Err(_) => {
            tracing::warn!("MC_dbExists unreadable name @ {name_ptr:#x}, defaulting to 0");
            return Ok(0);
        }
    };

    let system = context.system();
    let pid = system.pid().to_owned();
    let exists = system.platform().database_repository().exists(&name, &pid).await;

    let result = if exists { 1 } else { 0 };
    tracing::debug!("MC_dbExists({name:?}) -> {result}");
    Ok(result)
}

/// Read a `DatabaseHandle` from guest memory if `db_id` looks like one.
///
/// Returns `Ok(None)` for any pointer that's obviously not a handle —
/// out-of-range, missing the magic sentinel — so callers can return
/// `M_E_INVALIDHANDLE` instead of panicking on garbage input.
fn load_handle(context: &mut dyn WIPICContext, db_id: i32) -> Result<Option<DatabaseHandle>> {
    if db_id < 0x10000 {
        return Ok(None);
    }
    let handle: DatabaseHandle = read_generic(context, db_id as _)?;
    if handle.magic != DATABASE_HANDLE_MAGIC {
        return Ok(None);
    }
    Ok(Some(handle))
}

async fn open_db_for_handle(context: &mut dyn WIPICContext, handle: &DatabaseHandle) -> Option<Box<dyn Database>> {
    let name_length = handle.name.iter().position(|&c| c == 0).unwrap_or(handle.name.len());
    let db_name = str::from_utf8(&handle.name[..name_length]).ok()?;

    let system = context.system();
    let pid = system.pid().to_owned();

    Some(system.platform().database_repository().open(db_name, &pid).await)
}

async fn get_database_from_db_id(context: &mut dyn WIPICContext, db_id: i32) -> Result<Option<Box<dyn Database>>> {
    let Some(handle) = load_handle(context, db_id)? else {
        return Ok(None);
    };
    Ok(open_db_for_handle(context, &handle).await)
}

async fn read_packaged_database(context: &mut dyn WIPICContext, name: &str) -> Result<Option<Vec<u8>>> {
    if context.get_resource_size(name).await?.is_none() {
        return Ok(None);
    }

    Ok(Some(context.read_resource(name).await?))
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, vec};

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::{ByteRead, ByteWrite};

    use crate::context::test::TestContext;

    use super::{
        LgtDatabaseMetadata, close_database_lgt, delete_database, delete_database_lgt, delete_record_lgt,
        exists_database, insert_record_lgt, list_record_info, load_handle,
        load_lgt_metadata, open_database, open_database_lgt, open_db_for_handle,
        select_record, select_record_lgt, store_lgt_metadata, stream_read, stream_write,
        update_record, update_record_lgt,
    };

    #[futures_test::test]
    async fn lgt_native_insert_record_validates_handle_and_arguments() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, 0x1234, 0x2000, 4)
                .await
                .unwrap(),
            -2
        );

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();
        assert!(db_id > 0);

        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0, 4)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2000, 0)
                .await
                .unwrap(),
            -9
        );

        context.write_bytes(0x2000, &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2000, 5)
                .await
                .unwrap(),
            -21
        );
    }

    #[futures_test::test]
    async fn lgt_native_insert_record_returns_ids_and_zero_pads_fixed_records() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 8, 1, 1)
            .await
            .unwrap();
        context.write_bytes(0x2000, &[1, 2, 3]).unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2000, 3)
                .await
                .unwrap(),
            1
        );

        context.write_bytes(0x2010, &[4, 5, 6, 7, 8, 9, 10, 11]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2010, 8)
                .await
                .unwrap(),
            2
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        let mut db = open_db_for_handle(&mut context, &handle).await.unwrap();

        assert_eq!(
            db.get(1).await.unwrap(),
            vec![1, 2, 3, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            db.get(2).await.unwrap(),
            vec![4, 5, 6, 7, 8, 9, 10, 11]
        );

        let metadata = load_lgt_metadata(db.as_mut()).await.unwrap();
        assert_eq!(metadata.record_size, 8);
        assert_eq!(metadata.next_record_id, 3);
        assert_eq!(metadata.active_count, 2);
        assert!(metadata.free_ids.is_empty());
    }

    #[futures_test::test]
    async fn lgt_native_insert_record_reuses_persisted_free_ids_lifo() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        {
            let mut db = open_db_for_handle(&mut context, &handle).await.unwrap();
            let metadata = LgtDatabaseMetadata {
                record_size: 4,
                next_record_id: 6,
                active_count: 2,
                free_ids: vec![2, 4, 5],
            };
            assert!(store_lgt_metadata(db.as_mut(), &metadata).await);
        }

        context.write_bytes(0x2000, &[9, 8, 7, 6]).unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2000, 4)
                .await
                .unwrap(),
            5
        );
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2000, 4)
                .await
                .unwrap(),
            4
        );

        assert_eq!(close_database_lgt(&mut context, db_id).await.unwrap(), 0);

        let reopened = open_database_lgt(&mut context, 0x1000, 99, 1, 1)
            .await
            .unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, reopened, 0x2000, 4)
                .await
                .unwrap(),
            2
        );

        let handle = load_handle(&mut context, reopened).unwrap().unwrap();
        let mut db = open_db_for_handle(&mut context, &handle).await.unwrap();
        let metadata = load_lgt_metadata(db.as_mut()).await.unwrap();

        assert_eq!(metadata.record_size, 4);
        assert_eq!(metadata.next_record_id, 6);
        assert_eq!(metadata.active_count, 5);
        assert!(metadata.free_ids.is_empty());
    }

    #[futures_test::test]
    async fn lgt_native_delete_record_matches_native_id_validation() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            delete_record_lgt(&mut context, 0x1234, 1)
                .await
                .unwrap(),
            -2
        );

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );

        assert_eq!(
            delete_record_lgt(&mut context, db_id, 2)
                .await
                .unwrap(),
            -22
        );
        assert_eq!(
            delete_record_lgt(&mut context, db_id, -1)
                .await
                .unwrap(),
            -1
        );
    }

    #[futures_test::test]
    async fn lgt_native_delete_record_preserves_payload_and_reuses_id_lifo() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        context.write_bytes(0x2110, &[5, 6, 7, 8]).unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2110, 4)
                .await
                .unwrap(),
            2
        );

        assert_eq!(
            delete_record_lgt(&mut context, db_id, 2)
                .await
                .unwrap(),
            0
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        {
            let mut repository_db =
                open_db_for_handle(&mut context, &handle).await.unwrap();

            assert_eq!(
                repository_db.get(2).await.unwrap(),
                vec![5, 6, 7, 8]
            );

            let metadata =
                load_lgt_metadata(repository_db.as_mut()).await.unwrap();

            assert_eq!(metadata.free_ids, vec![2]);
            assert_eq!(metadata.active_count, 1);
            assert_eq!(metadata.next_record_id, 3);
        }

        assert_eq!(
            delete_record_lgt(&mut context, db_id, 2)
                .await
                .unwrap(),
            -22
        );

        context.write_bytes(0x2120, &[9, 9, 9, 9]).unwrap();

        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2120, 4)
                .await
                .unwrap(),
            2
        );

        let repository_db =
            open_db_for_handle(&mut context, &handle).await.unwrap();

        assert_eq!(
            repository_db.get(2).await.unwrap(),
            vec![9, 9, 9, 9]
        );
    }

    #[futures_test::test]
    async fn lgt_native_delete_record_accepts_zero_and_wraps_active_count() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        assert_eq!(
            delete_record_lgt(&mut context, db_id, 0)
                .await
                .unwrap(),
            0
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        let mut repository_db =
            open_db_for_handle(&mut context, &handle).await.unwrap();

        let metadata =
            load_lgt_metadata(repository_db.as_mut()).await.unwrap();

        assert_eq!(metadata.free_ids, vec![0]);
        assert_eq!(metadata.active_count, u32::MAX);
        assert_eq!(metadata.next_record_id, 1);

        assert_eq!(
            delete_record_lgt(&mut context, db_id, 0)
                .await
                .unwrap(),
            -22
        );
    }

    #[futures_test::test]
    async fn lgt_native_update_record_validates_native_arguments_and_ids() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            update_record_lgt(&mut context, 0x1234, 1, 0x2000, 4)
                .await
                .unwrap(),
            -2
        );

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        assert_eq!(
            update_record_lgt(&mut context, db_id, 1, 0, 4)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            update_record_lgt(&mut context, db_id, 1, 0x2000, 0)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            update_record_lgt(&mut context, db_id, 1, 0x2000, 5)
                .await
                .unwrap(),
            -21
        );

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );

        assert_eq!(
            update_record_lgt(&mut context, db_id, 0, 0x2000, 4)
                .await
                .unwrap(),
            -22
        );
        assert_eq!(
            update_record_lgt(&mut context, db_id, -1, 0x2000, 4)
                .await
                .unwrap(),
            -22
        );
        assert_eq!(
            update_record_lgt(&mut context, db_id, 2, 0x2000, 4)
                .await
                .unwrap(),
            -22
        );
    }

    #[futures_test::test]
    async fn lgt_native_update_record_preserves_tail_on_short_write() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 6, 1, 1)
            .await
            .unwrap();

        context
            .write_bytes(0x2100, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 6)
                .await
                .unwrap(),
            1
        );

        context.write_bytes(0x2200, &[9, 8, 7]).unwrap();
        assert_eq!(
            update_record_lgt(&mut context, db_id, 1, 0x2200, 3)
                .await
                .unwrap(),
            0
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        let db = open_db_for_handle(&mut context, &handle).await.unwrap();
        assert_eq!(db.get(1).await.unwrap(), vec![9, 8, 7, 4, 5, 6]);
    }

    #[futures_test::test]
    async fn lgt_native_update_record_rejects_free_record_with_minus_nine() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        {
            let mut db = open_db_for_handle(&mut context, &handle).await.unwrap();
            let mut metadata = load_lgt_metadata(db.as_mut()).await.unwrap();
            metadata.free_ids.push(1);
            metadata.active_count = 0;
            assert!(store_lgt_metadata(db.as_mut(), &metadata).await);
        }

        context.write_bytes(0x2200, &[9, 9, 9, 9]).unwrap();
        assert_eq!(
            update_record_lgt(&mut context, db_id, 1, 0x2200, 4)
                .await
                .unwrap(),
            -9
        );

        let db = open_db_for_handle(&mut context, &handle).await.unwrap();
        assert_eq!(db.get(1).await.unwrap(), vec![1, 2, 3, 4]);
    }

    #[futures_test::test]
    async fn lgt_native_select_record_validates_native_arguments_and_ids() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            select_record_lgt(&mut context, 0x1234, 1, 0x2000, 4)
                .await
                .unwrap(),
            -2
        );

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0, 4)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0x2000, 0)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0x2000, 3)
                .await
                .unwrap(),
            -18
        );

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );

        assert_eq!(
            select_record_lgt(&mut context, db_id, 2, 0x2000, 4)
                .await
                .unwrap(),
            -22
        );
        assert_eq!(
            select_record_lgt(&mut context, db_id, 0, 0x2000, 4)
                .await
                .unwrap(),
            -1
        );
        assert_eq!(
            select_record_lgt(&mut context, db_id, -1, 0x2000, 4)
                .await
                .unwrap(),
            -1
        );
    }

    #[futures_test::test]
    async fn lgt_native_select_record_reads_fixed_and_contiguous_bytes() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        context.write_bytes(0x2110, &[5, 6, 7, 8]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2110, 4)
                .await
                .unwrap(),
            2
        );

        context.write_bytes(0x2200, &[0xcc; 12]).unwrap();
        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0x2200, 4)
                .await
                .unwrap(),
            0
        );
        let mut fixed = [0u8; 12];
        context.read_bytes(0x2200, &mut fixed).unwrap();
        assert_eq!(&fixed[..4], &[1, 2, 3, 4]);
        assert_eq!(&fixed[4..], &[0xcc; 8]);

        context.write_bytes(0x2200, &[0xcc; 12]).unwrap();
        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0x2200, 10)
                .await
                .unwrap(),
            0
        );
        let mut contiguous = [0u8; 12];
        context.read_bytes(0x2200, &mut contiguous).unwrap();
        assert_eq!(&contiguous[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&contiguous[8..], &[0xcc; 4]);
    }

    #[futures_test::test]
    async fn lgt_native_select_record_rejects_free_start_but_reads_through_free_slot() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 4, 1, 1)
            .await
            .unwrap();

        context.write_bytes(0x2100, &[1, 2, 3, 4]).unwrap();
        context.write_bytes(0x2110, &[9, 8, 7, 6]).unwrap();
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2100, 4)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            insert_record_lgt(&mut context, db_id, 0x2110, 4)
                .await
                .unwrap(),
            2
        );

        let handle = load_handle(&mut context, db_id).unwrap().unwrap();
        {
            let mut db = open_db_for_handle(&mut context, &handle).await.unwrap();
            let mut metadata = load_lgt_metadata(db.as_mut()).await.unwrap();
            metadata.free_ids.push(2);
            metadata.active_count = 1;
            assert!(store_lgt_metadata(db.as_mut(), &metadata).await);
        }

        assert_eq!(
            select_record_lgt(&mut context, db_id, 2, 0x2200, 4)
                .await
                .unwrap(),
            -22
        );

        context.write_bytes(0x2200, &[0xcc; 8]).unwrap();
        assert_eq!(
            select_record_lgt(&mut context, db_id, 1, 0x2200, 8)
                .await
                .unwrap(),
            0
        );

        let mut data = [0u8; 8];
        context.read_bytes(0x2200, &mut data).unwrap();
        assert_eq!(data, [1, 2, 3, 4, 9, 8, 7, 6]);
    }

    #[futures_test::test]
    async fn lgt_native_delete_database_validates_arguments() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            delete_database_lgt(&mut context, 0, 1).await.unwrap(),
            -9
        );
        assert_eq!(
            delete_database_lgt(&mut context, 0x1000, 0).await.unwrap(),
            -9
        );
        assert_eq!(
            delete_database_lgt(&mut context, 0x1000, 4).await.unwrap(),
            -9
        );
    }

    #[futures_test::test]
    async fn lgt_native_delete_database_missing_returns_minus_one() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            delete_database_lgt(&mut context, 0x1000, 1)
                .await
                .unwrap(),
            -1
        );
    }

    #[futures_test::test]
    async fn lgt_native_delete_database_removes_existing_database() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 32, 1, 1)
            .await
            .unwrap();
        assert!(db_id > 0);

        assert_eq!(
            delete_database_lgt(&mut context, 0x1000, 1)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            delete_database_lgt(&mut context, 0x1000, 1)
                .await
                .unwrap(),
            -1
        );
    }

    #[futures_test::test]
    async fn lgt_native_close_database_rejects_unknown_handle_with_minus_two() {
        let mut context = database_test_context();

        assert_eq!(
            close_database_lgt(&mut context, 0).await.unwrap(),
            -2
        );
        assert_eq!(
            close_database_lgt(&mut context, 0x1234).await.unwrap(),
            -2
        );
    }

    #[futures_test::test]
    async fn lgt_native_close_database_invalidates_closed_handle() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 32, 1, 1)
            .await
            .unwrap();
        assert!(db_id > 0);

        assert_eq!(
            close_database_lgt(&mut context, db_id).await.unwrap(),
            0
        );
        assert_eq!(
            close_database_lgt(&mut context, db_id).await.unwrap(),
            -2
        );
    }

    #[futures_test::test]
    async fn lgt_native_open_database_validates_arguments() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            open_database_lgt(&mut context, 0, 32, 1, 1)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            open_database_lgt(&mut context, 0x1000, 0, 1, 1)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            open_database_lgt(&mut context, 0x1000, 32, 1, 0)
                .await
                .unwrap(),
            -9
        );
        assert_eq!(
            open_database_lgt(&mut context, 0x1000, 32, 1, 4)
                .await
                .unwrap(),
            -9
        );
    }

    #[futures_test::test]
    async fn lgt_native_open_database_requires_existing_database_without_create() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(
            open_database_lgt(&mut context, 0x1000, 32, 0, 1)
                .await
                .unwrap(),
            -12
        );
    }

    #[futures_test::test]
    async fn lgt_native_open_database_create_materializes_database() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 32, 1, 1)
            .await
            .unwrap();
        assert!(db_id > 0);
        assert_eq!(
            exists_database(&mut context, 0x1000, 1).await.unwrap(),
            0
        );
    }

    #[futures_test::test]
    async fn lgt_native_open_database_create_preserves_existing_contents() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        let db_id = open_database_lgt(&mut context, 0x1000, 32, 1, 1)
            .await
            .unwrap();
        context.write_bytes(0x2000, &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            stream_write(&mut context, db_id, 0x2000, 4)
                .await
                .unwrap(),
            4
        );

        let reopened = open_database_lgt(&mut context, 0x1000, 32, 1, 1)
            .await
            .unwrap();
        assert!(reopened > 0);
        assert_eq!(
            stream_read(&mut context, reopened, 0x2100, 4)
                .await
                .unwrap(),
            4
        );

        let mut data = [0; 4];
        context.read_bytes(0x2100, &mut data).unwrap();
        assert_eq!(data, [1, 2, 3, 4]);
    }

    #[futures_test::test]
    async fn lgt_exists_database_reports_missing_and_existing_database() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), -12);
        let db_id = open_database(&mut context, 0x1000, 0, 0).await.unwrap();
        context.write_bytes(0x2000, &[1]).unwrap();
        assert_eq!(stream_write(&mut context, db_id, 0x2000, 1).await.unwrap(), 1);
        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_create_mode_materializes_empty_database() {
        let mut context = database_test_context();
        context.write_bytes(0x1000, b"records\0").unwrap();

        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), -12);
        let db_id = open_database(&mut context, 0x1000, 4, 0).await.unwrap();
        assert!(db_id > 0);
        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), 0);
    }

    #[futures_test::test]
    async fn lgt_update_and_select_record_use_standard_record_ids() {
        let mut context = database_test_context();
        let db_id = open_test_database(&mut context).await;
        context.write_bytes(0x2000, &[1, 2, 3]).unwrap();
        assert_eq!(stream_write(&mut context, db_id, 0x2000, 3).await.unwrap(), 3);
        context.write_bytes(0x2010, &[4, 5]).unwrap();

        assert_eq!(update_record(&mut context, db_id, 1, 0x2010, 2).await.unwrap(), 0);
        assert_eq!(select_record(&mut context, db_id, 1, 0x2100, 2).await.unwrap(), 0);

        let mut data = [0; 2];
        context.read_bytes(0x2100, &mut data).unwrap();
        assert_eq!(data, [4, 5]);
    }

    #[futures_test::test]
    async fn lgt_list_record_info_and_delete_database_use_database_name() {
        let mut context = database_test_context();
        let db_id = open_test_database(&mut context).await;
        context.write_bytes(0x2000, &[1, 2, 3, 4]).unwrap();
        assert_eq!(stream_write(&mut context, db_id, 0x2000, 4).await.unwrap(), 4);

        assert_eq!(list_record_info(&mut context, 0x1000, 0x2100, 1).await.unwrap(), 0);
        let mut entry = [0; 12];
        context.read_bytes(0x2100, &mut entry).unwrap();
        assert_eq!(u32::from_le_bytes(entry[0..4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()), 4);

        assert_eq!(delete_database(&mut context, 0x1000, 1).await.unwrap(), 0);
        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), -12);
    }

    #[futures_test::test]
    async fn lgt_open_database_materializes_packaged_database() {
        let mut context = database_test_context().with_resource("kickass", b"seed-data");
        context.write_bytes(0x1000, b"kickass\0").unwrap();

        assert_eq!(exists_database(&mut context, 0x1000, 1).await.unwrap(), 0);
        let db_id = open_database(&mut context, 0x1000, 1, 0).await.unwrap();
        assert!(db_id > 0);
        assert_eq!(stream_read(&mut context, db_id, 0x2000, 9).await.unwrap(), 9);

        let mut data = [0; 9];
        context.read_bytes(0x2000, &mut data).unwrap();
        assert_eq!(&data, b"seed-data");
    }

    fn database_test_context() -> TestContext {
        let system = System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner);
        TestContext::with_system(system)
    }

    async fn open_test_database(context: &mut TestContext) -> i32 {
        context.write_bytes(0x1000, b"records\0").unwrap();
        open_database(context, 0x1000, 0, 0).await.unwrap()
    }
}
