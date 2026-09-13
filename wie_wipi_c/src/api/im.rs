//! `MC_imHandleInput` and friends - typing on a keypad.
//!
//! A handset turns keypresses into text through an input method: the same key
//! pressed twice is a different letter, and a Korean syllable is built from
//! several presses before it is finished. These five calls are how a WIPI-C
//! title reaches that, and they sit on the same
//! [`wie_backend::System::handle_input_method`] the WIPI-Java side and the UIC
//! text component already use, so a title gets the same typing through whichever
//! door it comes in.
//!
//! Their shapes come from LGT native's DIME implementation, which implements the
//! standard calls rather than anything of its own, and the mode set is confirmed
//! by the reference emulator, whose string table carries exactly
//! `EN/S\0EN/L\0N123\0KO\0` beside its `setCurrentMode(I)Z`.

use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use wie_util::{Result, write_generic, write_null_terminated_string_bytes};

use wipi_types::wipic::WIPICWord;

use crate::context::WIPICContext;

/// The modes a title can be typing in, in the order they are reported.
///
/// EN/S and EN/L are English in each case, N123 is digits, KO is Korean. The
/// same four, in the same order, are what the reference emulator carries.
const SUPPORTED_MODES: [&[u8]; 4] = [b"EN/S", b"EN/L", b"N123", b"KO"];

#[derive(Default)]
pub struct ImState {
    /// The mode table once it has been built, so a title that asks twice gets
    /// the same pointer - native hands back a table that outlives the call.
    supported_modes: Option<WIPICWord>,
}

pub type SharedImState = Arc<Mutex<ImState>>;

pub fn new_state() -> SharedImState {
    Arc::new(Mutex::new(ImState::default()))
}

/// `MC_imGetSupportModeCount` - how many modes there are to cycle through.
pub async fn get_support_mode_count(_context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    tracing::debug!("MC_imGetSupportModeCount() -> {}", SUPPORTED_MODES.len());

    Ok(SUPPORTED_MODES.len() as WIPICWord)
}

/// `MC_imGetSupportedModes` - their names, as a `char **` the title can keep.
///
/// Built once and handed back on every later call, because native's table is a
/// fixed one: a title that asks during a redraw must not be given a new
/// allocation each frame.
pub async fn get_supported_modes(context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    if let Some(existing) = context.im_state().lock().supported_modes {
        return Ok(existing);
    }

    let table_size = (SUPPORTED_MODES.len() * size_of::<WIPICWord>()) as WIPICWord;
    let names_size: WIPICWord = SUPPORTED_MODES.iter().map(|name| name.len() as WIPICWord + 1).sum();

    let memory = context.alloc(table_size + names_size)?;
    let table = context.data_ptr(memory)?;

    let mut name_address = table + table_size;
    let mut addresses = Vec::with_capacity(SUPPORTED_MODES.len());
    for name in SUPPORTED_MODES {
        write_null_terminated_string_bytes(context, name_address, name)?;
        addresses.push(name_address);
        name_address += name.len() as WIPICWord + 1;
    }

    for (index, address) in addresses.into_iter().enumerate() {
        write_generic(context, table + (index * size_of::<WIPICWord>()) as WIPICWord, address)?;
    }

    context.im_state().lock().supported_modes = Some(table);

    tracing::debug!("MC_imGetSupportedModes() -> {table:#x}");

    Ok(table)
}

/// `MC_imGetCurrentMode` - which of them is selected, as an index.
pub async fn get_current_mode(context: &mut dyn WIPICContext) -> Result<WIPICWord> {
    let mode = context.system().current_input_mode();
    tracing::debug!("MC_imGetCurrentMode() -> {mode}");

    Ok(mode)
}

/// `MC_imSetCurrentMode` - select one.
///
/// Answers 1 for a mode that exists and 0 for one that does not, and an index
/// that does not exist changes nothing.
pub async fn set_current_mode(context: &mut dyn WIPICContext, mode: WIPICWord) -> Result<WIPICWord> {
    if mode >= SUPPORTED_MODES.len() as WIPICWord {
        tracing::debug!("MC_imSetCurrentMode({mode}) -> 0, there is no such mode");

        return Ok(0);
    }

    tracing::debug!("MC_imSetCurrentMode({mode}) -> 1");
    context.system().set_current_input_mode(mode);

    Ok(1)
}

/// Normalize the public `MC_imHandleInput` key to the signed byte the input
/// method takes.
///
/// The native 35..57 jump table is identity. `ime_handle` then:
/// - maps signed -3 to -16,
/// - preserves `*`, `#`, digits, and values in its accepted unsigned range,
/// - maps other out-of-range values to -99,
/// - finally forwards only the low byte to the provider.
fn provider_key(key: WIPICWord) -> i8 {
    let signed = key as i32;

    let normalized = if signed == -3 {
        -16i32
    } else if signed == 42 || signed == 35 || (48..=57).contains(&signed) {
        signed
    } else {
        let range_value = key.wrapping_sub(32);
        if range_value > 65_499 { -99 } else { signed }
    };

    normalized as u8 as i8
}

/// `MC_imHandleInput` - give the input method a keypress and take back text.
///
/// Two buffers come back: `output0` is text that is finished and `output1` is
/// the syllable still being composed, each with its length written through the
/// pointer beside it. Both are cleared before the input method runs, so a title
/// that only checks the lengths and a title that reads the buffers agree.
///
/// Native maps WIPI events 502/503/504 to DIME 1/2/3; `ime_handle` maps those to
/// provider events 2/3/4, and the provider accepts only 2 and 4. So a press and
/// a release are processed and a repeat is ignored without the caller's buffers
/// being touched at all.
pub async fn handle_input(
    context: &mut dyn WIPICContext,
    key: WIPICWord,
    event: WIPICWord,
    output0: WIPICWord,
    output0_len: WIPICWord,
    output1: WIPICWord,
    output1_len: WIPICWord,
) -> Result<WIPICWord> {
    tracing::debug!("MC_imHandleInput({key:#x}, {event}, {output0:#x}, {output0_len:#x}, {output1:#x}, {output1_len:#x})");

    let provider_event = match event {
        502 => 2,
        504 => 4,
        _ => return Ok(0),
    };

    context.write_bytes(output0, &[0])?;
    write_generic(context, output0_len, 0u32)?;
    context.write_bytes(output1, &[0])?;
    write_generic(context, output1_len, 0u32)?;

    let output = context.system().handle_input_method(provider_key(key), provider_event);

    if output.output0_len != 0 {
        context.write_bytes(output0, &output.output0[..output.output0_len])?;
    }
    write_generic(context, output0_len, output.output0_len as u32)?;

    if output.output1_len != 0 {
        context.write_bytes(output1, &output.output1[..output.output1_len])?;
    }
    write_generic(context, output1_len, output.output1_len as u32)?;

    Ok(if output.handled { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, vec::Vec};

    use test_utils::TestPlatform;
    use wie_backend::{DefaultTaskRunner, System};
    use wie_util::{ByteRead, ByteWrite, read_generic, read_null_terminated_string_bytes};

    use wipi_types::wipic::WIPICWord;

    use crate::context::test::TestContext;

    use super::{get_current_mode, get_support_mode_count, get_supported_modes, handle_input, provider_key, set_current_mode};

    fn im_context() -> TestContext {
        TestContext::with_system(System::new(Box::new(TestPlatform::new()), "test-pid", "test-aid", DefaultTaskRunner))
    }

    #[test]
    fn provider_key_matches_native() {
        // Native MC_imHandleInput 35..57 jump table is identity.
        for key in 35u32..=57 {
            assert_eq!(provider_key(key), key as u8 as i8);
        }

        // ime_handle special signed key.
        assert_eq!(provider_key((-3i32) as u32), -16);

        // Other directly supplied signed negative WIPI keys are rejected to
        // provider flush key -99.
        for key in [-16i32, -7, -4, -2, -1, -99] {
            assert_eq!(provider_key(key as u32), -99);
        }

        // UIC masks special keys before calling the public API. These values
        // survive ime_handle and become signed again at the provider boundary.
        assert_eq!(provider_key(157), -99);
        assert_eq!(provider_key(240), -16);
        assert_eq!(provider_key(249), -7);
        assert_eq!(provider_key(252), -4);
        assert_eq!(provider_key(253), -3);
        assert_eq!(provider_key(254), -2);
        assert_eq!(provider_key(255), -1);

        // Accepted printable/range values remain their low byte.
        for key in [32u32, 34, 35, 36, 42, 47, 48, 57, 58, 240, 255] {
            assert_eq!(provider_key(key), key as u8 as i8);
        }
    }

    /// The four modes are named and counted the same way, so an index a title
    /// gets from one call means something in the other.
    #[futures_test::test]
    async fn the_modes_are_named_and_counted_together() {
        let mut context = im_context();

        let count = get_support_mode_count(&mut context).await.unwrap();
        assert_eq!(count, 4);

        let table = get_supported_modes(&mut context).await.unwrap();
        let mut names = Vec::new();
        for index in 0..count {
            let address: WIPICWord = read_generic(&context, table + index * 4).unwrap();
            names.push(read_null_terminated_string_bytes(&context, address).unwrap());
        }

        assert_eq!(names, [b"EN/S".to_vec(), b"EN/L".to_vec(), b"N123".to_vec(), b"KO".to_vec()]);
    }

    /// Asking twice gives the same table: a title that asks while painting must
    /// not be handed a new allocation each frame.
    #[futures_test::test]
    async fn the_mode_table_is_built_once() {
        let mut context = im_context();

        let first = get_supported_modes(&mut context).await.unwrap();
        assert_eq!(get_supported_modes(&mut context).await.unwrap(), first);
    }

    /// A mode that exists is selected and reads back; one that does not is
    /// refused and changes nothing.
    #[futures_test::test]
    async fn only_a_mode_that_exists_can_be_selected() {
        let mut context = im_context();

        assert_eq!(set_current_mode(&mut context, 3).await.unwrap(), 1);
        assert_eq!(get_current_mode(&mut context).await.unwrap(), 3);

        assert_eq!(set_current_mode(&mut context, 4).await.unwrap(), 0);
        assert_eq!(get_current_mode(&mut context).await.unwrap(), 3);
    }

    /// A digit typed in N123 comes back committed, with the length written
    /// beside it.
    #[futures_test::test]
    async fn a_typed_digit_comes_back_committed() {
        let mut context = im_context();
        set_current_mode(&mut context, 2).await.unwrap();

        let handled = handle_input(&mut context, b'7' as WIPICWord, 502, 0x1000, 0x1100, 0x1200, 0x1300)
            .await
            .unwrap();
        assert_eq!(handled, 1);

        let committed_len: u32 = read_generic(&context, 0x1100).unwrap();
        assert_eq!(committed_len, 1);

        let mut committed = [0u8; 1];
        context.read_bytes(0x1000, &mut committed).unwrap();
        assert_eq!(&committed, b"7");
    }

    /// An event that is not a press or a release leaves the caller's buffers
    /// exactly as they were - native never reaches them.
    #[futures_test::test]
    async fn a_repeat_event_touches_nothing() {
        let mut context = im_context();
        context.write_bytes(0x1100, &0xffff_ffffu32.to_le_bytes()).unwrap();

        assert_eq!(
            handle_input(&mut context, b'7' as WIPICWord, 503, 0x1000, 0x1100, 0x1200, 0x1300)
                .await
                .unwrap(),
            0
        );

        let untouched: u32 = read_generic(&context, 0x1100).unwrap();
        assert_eq!(untouched, 0xffff_ffff);
    }
}
