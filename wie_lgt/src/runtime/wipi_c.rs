use alloc::{boxed::Box, string::ToString, vec};

mod context;

use jvm::{Jvm, Result as JvmResult, runtime::JavaLangString};
use jvm_rust::ClassDefinitionImpl;
use wipi_types::lgt::CletFunctions;
use wipi_types::wipic::WIPICIndirectPtr;

use wie_backend::System;
use wie_core_arm::{ArmCore, EmulatedFunction, EmulatedFunctionParam, ResultWriter, SvcId};
use wie_jvm_support::JvmSupport;
use wie_util::{Result, read_generic, write_generic, write_null_terminated_string_bytes};
use wie_wipi_c::{
    MethodImpl, WIPICContext, WIPICMethodBody, WIPICResult,
    api::{database, filesystem, graphics, kernel, media, misc, net, phone, serial, system, uic, util},
};

use context::LgtWIPICContext;

use crate::runtime::java::classes::net::wie::{CletWrapper, CletWrapperCard, CletWrapperContext};
use crate::runtime::{SVC_CATEGORY_WIPIC, svc_ids::WIPICSvcId};

const TIME_VALUE_PTR: u32 = 0x7fff1004;
const IME_SUPPORTED_MODES_PTR: u32 = 0x7fff1008;

struct WIPICMethodResult {
    result: WIPICResult,
}

impl ResultWriter<WIPICMethodResult> for WIPICMethodResult {
    fn write(self, core: &mut ArmCore, next_pc: u32) -> Result<()> {
        core.write_return_value(&self.result.results)?;
        core.set_next_pc(next_pc)?;

        Ok(())
    }
}

struct CMethodProxy {
    context: LgtWIPICContext,
    body: WIPICMethodBody,
}

async fn handle_wipic_svc(
    core: &mut ArmCore,
    (system, jvm, network_state, serial_state, filesystem_state): &mut (
        System,
        Jvm,
        net::SharedNetworkState,
        serial::SharedSerialState,
        filesystem::SharedFilesystemState,
    ),
    id: SvcId,
) -> Result<()> {
    let wipic_context = LgtWIPICContext::new(
        core.clone(),
        system.clone(),
        jvm.clone(),
        network_state.clone(),
        serial_state.clone(),
        filesystem_state.clone(),
    );
    let (_, lr) = core.read_pc_lr()?;
    // An unimplemented WIPI-C function is reported and skipped rather than
    // ending the run. Stopping on the first one hides everything a title does
    // afterwards, and the calls that turn up are usually peripheral - Xenogia
    // stops on database id 0x19c before drawing anything.
    let svc_id = match WIPICSvcId::try_from(id) {
        Ok(svc_id) => svc_id,
        Err(error) => {
            tracing::warn!("{error}; returning 0");

            return WIPICMethodResult {
                result: WIPICResult { results: vec![0] },
            }
            .write(core, lr);
        }
    };

    let method = match svc_id {
        WIPICSvcId::CletRegister => {
            return EmulatedFunction::call(&clet_register, core, jvm).await?.write(core, lr);
        }
        WIPICSvcId::GetFramebufferPointer => graphics::get_framebuffer_pointer.into_body(),
        WIPICSvcId::GetFramebufferWidth => graphics::get_framebuffer_width.into_body(),
        WIPICSvcId::GetFramebufferHeight => graphics::get_framebuffer_height.into_body(),
        WIPICSvcId::GetFramebufferBpl => graphics::get_framebuffer_bpl.into_body(),
        WIPICSvcId::GetFramebufferBpp => graphics::get_framebuffer_bpp.into_body(),
        WIPICSvcId::Printk => kernel::printk.into_body(),
        WIPICSvcId::Sprintk => kernel::sprintk.into_body(),
        WIPICSvcId::Unk13 => unk13.into_body(),
        WIPICSvcId::GetCurProgramId => kernel::get_cur_program_id.into_body(),
        WIPICSvcId::GetProgramName => kernel::get_program_name.into_body(),
        WIPICSvcId::Exit => kernel::exit.into_body(),
        WIPICSvcId::Alloc => kernel::alloc.into_body(),
        WIPICSvcId::Calloc => kernel::calloc.into_body(),
        WIPICSvcId::Free => kernel::free.into_body(),
        WIPICSvcId::GetTotalMemory => kernel::get_total_memory.into_body(),
        WIPICSvcId::GetFreeMemory => kernel::get_free_memory.into_body(),
        WIPICSvcId::DefTimer => kernel::def_timer.into_body(),
        WIPICSvcId::SetTimer => kernel::set_timer.into_body(),
        WIPICSvcId::UnsetTimer => kernel::unset_timer.into_body(),
        WIPICSvcId::CurrentTime => kernel::current_time.into_body(),
        WIPICSvcId::GetSystemProperty => kernel::get_system_property.into_body(),
        WIPICSvcId::SetSystemProperty => kernel::set_system_property.into_body(),
        WIPICSvcId::GetResourceId => kernel::get_resource_id.into_body(),
        WIPICSvcId::GetResource => kernel::get_resource.into_body(),
        WIPICSvcId::Unk2 => unk2.into_body(),
        WIPICSvcId::Unk3 => unk3.into_body(),
        WIPICSvcId::GetImageProperty => graphics::get_image_property.into_body(),
        WIPICSvcId::GetImageFramebuffer => graphics::get_image_framebuffer.into_body(),
        WIPICSvcId::GetScreenFramebuffer => graphics::get_screen_framebuffer.into_body(),
        WIPICSvcId::DestroyOffscreenFramebuffer => graphics::destroy_offscreen_framebuffer.into_body(),
        WIPICSvcId::CreateOffscreenFramebuffer => graphics::create_offscreen_framebuffer.into_body(),
        WIPICSvcId::InitContext => graphics::init_context.into_body(),
        WIPICSvcId::SetContext => graphics::set_context.into_body(),
        WIPICSvcId::PutPixel => graphics::put_pixel.into_body(),
        WIPICSvcId::DrawLine => graphics::draw_line.into_body(),
        WIPICSvcId::DrawRect => graphics::draw_rect.into_body(),
        WIPICSvcId::FillRect => graphics::fill_rect.into_body(),
        WIPICSvcId::DrawPolygon => graphics::draw_polygon.into_body(),
        WIPICSvcId::FillPolygon => graphics::fill_polygon.into_body(),
        WIPICSvcId::CopyFrameBuffer => graphics::copy_frame_buffer.into_body(),
        WIPICSvcId::DrawImage => graphics::draw_image.into_body(),
        WIPICSvcId::CopyArea => graphics::copy_area.into_body(),
        WIPICSvcId::DrawArc => graphics::draw_arc.into_body(),
        WIPICSvcId::FillArc => graphics::fill_arc.into_body(),
        WIPICSvcId::DrawString => graphics::draw_string.into_body(),
        WIPICSvcId::GetRgbPixels => graphics::get_rgb_pixels.into_body(),
        WIPICSvcId::SetRgbPixels => graphics::set_rgb_pixels.into_body(),
        WIPICSvcId::FlushLcd => graphics::flush_lcd.into_body(),
        WIPICSvcId::GetPixelFromRgb => graphics::get_pixel_from_rgb.into_body(),
        WIPICSvcId::GetRgbFromPixel => graphics::get_rgb_from_pixel.into_body(),
        WIPICSvcId::GetDisplayInfo => graphics::get_display_info.into_body(),
        WIPICSvcId::Repaint => graphics::repaint.into_body(),
        WIPICSvcId::GetFont => graphics::get_font.into_body(),
        WIPICSvcId::GetFontHeight => graphics::get_font_height.into_body(),
        WIPICSvcId::GetFontAscent => graphics::get_font_ascent.into_body(),
        WIPICSvcId::GetFontDescent => graphics::get_font_descent.into_body(),
        WIPICSvcId::GetStringWidth => graphics::get_string_width.into_body(),
        WIPICSvcId::CreateImage => graphics::create_image.into_body(),
        WIPICSvcId::Unk0 => unk0.into_body(),
        WIPICSvcId::PostEvent => graphics::post_event.into_body(),
        WIPICSvcId::ImGetSupportModeCount => im_get_support_mode_count.into_body(),
        WIPICSvcId::ImGetSupportedModes => im_get_supported_modes.into_body(),
        WIPICSvcId::ImSetCurrentMode => im_set_current_mode.into_body(),
        WIPICSvcId::ImGetCurrentMode => im_get_current_mode.into_body(),
        WIPICSvcId::ImHandleInput => im_handle_input.into_body(),
        WIPICSvcId::UicCreateApplicationContext => uic::create_application_context.into_body(),
        WIPICSvcId::UicGetClass => uic::get_class.into_body(),
        WIPICSvcId::UicCreate => uic::create.into_body(),
        WIPICSvcId::UicDestroy => uic::destroy.into_body(),
        WIPICSvcId::UicRepaint => uic::repaint.into_body(),
        WIPICSvcId::UicPaint => uic::paint.into_body(),
        WIPICSvcId::UicGetClassName => uic::get_class_name.into_body(),
        WIPICSvcId::UicIsInstance => uic::is_instance.into_body(),
        WIPICSvcId::UicHandleEvent => uic::handle_event.into_body(),
        WIPICSvcId::UicConfigure => uic::configure.into_body(),
        WIPICSvcId::UicGetGeometry => uic::get_geometry.into_body(),
        WIPICSvcId::UicSetEnable => uic::set_enable.into_body(),
        WIPICSvcId::UicSetCallback => uic::set_callback.into_body(),
        WIPICSvcId::UicSetEventHandler => uic::set_event_handler.into_body(),
        WIPICSvcId::UicSetFont => uic::set_font.into_body(),
        WIPICSvcId::UicGetFont => uic::get_font.into_body(),
        WIPICSvcId::UicSetFgColor => uic::set_fg_color.into_body(),
        WIPICSvcId::UicSetBgColor => uic::set_bg_color.into_body(),
        WIPICSvcId::UicSetLabel => uic::set_label.into_body(),
        WIPICSvcId::UicGetLabel => uic::get_label.into_body(),
        WIPICSvcId::UicSetLabelAlignment => uic::set_label_alignment.into_body(),
        WIPICSvcId::UicSetTimeMask => uic::set_time_mask.into_body(),
        WIPICSvcId::UicSetTime => uic::set_time.into_body(),
        WIPICSvcId::UicSetTimeLong => uic::set_time_long.into_body(),
        WIPICSvcId::UicInsertText => uic::insert_text.into_body(),
        WIPICSvcId::UicDeleteText => uic::delete_text.into_body(),
        WIPICSvcId::UicSetMaxTextSize => uic::set_max_text_size.into_body(),
        WIPICSvcId::UicGetMaxTextSize => uic::get_max_text_size.into_body(),
        WIPICSvcId::UicGetTextSize => uic::get_text_size.into_body(),
        WIPICSvcId::UicGetText => uic::get_text.into_body(),
        WIPICSvcId::UicAddListItem => uic::add_list_item.into_body(),
        WIPICSvcId::UicGetListItem => uic::get_list_item.into_body(),
        WIPICSvcId::UicRemoveListItem => uic::remove_list_item.into_body(),
        WIPICSvcId::UicSetActiveListItem => uic::set_active_list_item.into_body(),
        WIPICSvcId::UicGetActiveListItem => uic::get_active_list_item.into_body(),
        WIPICSvcId::UicSetCursorPos => uic::set_cursor_pos.into_body(),
        WIPICSvcId::UicGetCursorPos => uic::get_cursor_pos.into_body(),
        WIPICSvcId::UicGetTime => uic::get_time.into_body(),
        WIPICSvcId::UicAddMenuItem => uic::add_menu_item.into_body(),
        WIPICSvcId::UicGetMenuItem => uic::get_menu_item.into_body(),
        WIPICSvcId::UicRemoveMenuItem => uic::remove_menu_item.into_body(),
        WIPICSvcId::UicSetActiveMenuItem => uic::set_active_menu_item.into_body(),
        WIPICSvcId::UicGetActiveMenuItem => uic::get_active_menu_item.into_body(),
        WIPICSvcId::FsOpen => filesystem::open.into_body(),
        WIPICSvcId::OpenDatabase => database::open_database_lgt.into_body(),
        WIPICSvcId::CloseDatabase => database::close_database_lgt.into_body(),
        WIPICSvcId::DeleteDatabase => database::delete_database_lgt.into_body(),
        WIPICSvcId::InsertRecord => database::insert_record_lgt.into_body(),
        WIPICSvcId::SelectRecord => database::select_record_lgt.into_body(),
        WIPICSvcId::UpdateRecord => database::update_record_lgt.into_body(),
        WIPICSvcId::DeleteRecord => database::delete_record_lgt.into_body(),
        WIPICSvcId::ListRecords => database::list_records_lgt.into_body(),
        WIPICSvcId::SortRecords => database::sort_records_lgt.into_body(),
        WIPICSvcId::GetAccessMode => database::get_access_mode_lgt.into_body(),
        WIPICSvcId::GetNumberOfRecords => database::get_number_of_records_lgt.into_body(),
        WIPICSvcId::GetRecordSize => database::get_record_size_lgt.into_body(),
        WIPICSvcId::ListDatabases => database::list_databases_lgt.into_body(),
        WIPICSvcId::FsRead => filesystem::read.into_body(),
        WIPICSvcId::FsWrite => filesystem::write.into_body(),
        WIPICSvcId::FsClose => filesystem::close.into_body(),
        WIPICSvcId::FsSeek => filesystem::seek.into_body(),
        WIPICSvcId::FsFileAttribute => filesystem::file_attribute.into_body(),
        WIPICSvcId::FsRemove => filesystem::remove.into_body(),
        WIPICSvcId::FsRename => filesystem::rename.into_body(),
        WIPICSvcId::FsMkDir => filesystem::mkdir.into_body(),
        WIPICSvcId::FsRmDir => filesystem::rmdir.into_body(),
        WIPICSvcId::FsList => filesystem::list.into_body(),
        WIPICSvcId::FsTotalSpace => fs_total_space.into_body(),
        WIPICSvcId::FsSetMode => filesystem::set_mode.into_body(),
        WIPICSvcId::FsGetCounts => filesystem::get_counts.into_body(),
        WIPICSvcId::FsTell => filesystem::tell.into_body(),
        WIPICSvcId::FsIsExist => filesystem::is_exist.into_body(),
        WIPICSvcId::FsGetMountedNames => filesystem::get_mounted_names.into_body(),
        WIPICSvcId::FsTotalSpaceEx => fs_total_space_ex.into_body(),
        WIPICSvcId::FsAvailableEx => fs_available_ex.into_body(),
        WIPICSvcId::FsAvailable => fs_available.into_body(),
        WIPICSvcId::Connect => net::connect.into_body(),
        WIPICSvcId::Close => net::close.into_body(),
        WIPICSvcId::Socket => net::socket.into_body(),
        WIPICSvcId::SocketConnect => net::socket_connect.into_body(),
        WIPICSvcId::SocketWrite => net::socket_write.into_body(),
        WIPICSvcId::SocketRead => net::socket_read.into_body(),
        WIPICSvcId::SocketClose => net::socket_close.into_body(),
        WIPICSvcId::SocketBind => net::socket_bind.into_body(),
        WIPICSvcId::GetMaxPacketLength => net::get_max_packet_length.into_body(),
        WIPICSvcId::SocketSendTo => net::socket_send_to.into_body(),
        WIPICSvcId::SetReadCallback => net::set_read_callback.into_body(),
        WIPICSvcId::SetWriteCallback => net::set_write_callback.into_body(),
        WIPICSvcId::SerialOpen => serial::open.into_body(),
        WIPICSvcId::SerialWrite => serial::write.into_body(),
        WIPICSvcId::SerialSetWriteCallback => serial::set_write_callback.into_body(),
        WIPICSvcId::SerialClose => serial::close.into_body(),
        WIPICSvcId::BillSocket => net::bill_socket.into_body(),
        WIPICSvcId::Htonl => util::htonl.into_body(),
        WIPICSvcId::Htons => util::htons.into_body(),
        WIPICSvcId::Ntohl => util::ntohl.into_body(),
        WIPICSvcId::Ntohs => util::ntohs.into_body(),
        WIPICSvcId::InetAddrInt => util::inet_addr_int.into_body(),
        WIPICSvcId::ClipCreate => media::clip_create.into_body(),
        WIPICSvcId::ClipFree => media::clip_free.into_body(),
        WIPICSvcId::ClipPutData => media::clip_put_data.into_body(),
        WIPICSvcId::Unk15 => media::clip_control.into_body(),
        WIPICSvcId::ClipGetVolume => media::clip_get_volume.into_body(),
        WIPICSvcId::ClipSetVolume => media::clip_set_volume.into_body(),
        WIPICSvcId::Play => media::play.into_body(),
        WIPICSvcId::Pause => media::pause.into_body(),
        WIPICSvcId::Resume => media::resume.into_body(),
        WIPICSvcId::Stop => media::stop.into_body(),
        WIPICSvcId::Unk5 => unk5.into_body(),
        WIPICSvcId::Vibrator => media::vibrator.into_body(),
        WIPICSvcId::Unk14 => unk14.into_body(),
        WIPICSvcId::ClipAllocPlayer => media::clip_alloc_player.into_body(),
        WIPICSvcId::ClipFreePlayer => media::clip_free_player.into_body(),
        WIPICSvcId::Unk10 => unk10.into_body(),
        WIPICSvcId::SetMuteState => media::set_mute_state.into_body(),
        WIPICSvcId::GetMuteState => media::get_mute_state.into_body(),
        WIPICSvcId::CallPlace => phone::call_place.into_body(),
        WIPICSvcId::SmsSend => phone::sms_send.into_body(),
        WIPICSvcId::SysExecute => system::execute.into_body(),
        WIPICSvcId::BackLight => misc::back_light.into_body(),
    };

    EmulatedFunction::call(
        &CMethodProxy {
            context: wipic_context,
            body: method,
        },
        core,
        &mut (),
    )
    .await?
    .write(core, lr)
}

#[async_trait::async_trait]
impl EmulatedFunction<(), WIPICMethodResult, ()> for CMethodProxy {
    async fn call(&self, core: &mut ArmCore, _: &mut ()) -> Result<WIPICMethodResult> {
        let a0 = u32::get(core, 0);
        let a1 = u32::get(core, 1);
        let a2 = u32::get(core, 2);
        let a3 = u32::get(core, 3);
        let a4 = u32::get(core, 4);
        let a5 = u32::get(core, 5);
        let a6 = u32::get(core, 6);
        let a7 = u32::get(core, 7);
        let a8 = u32::get(core, 8);

        let result = self
            .body
            .call(&mut self.context.clone(), vec![a0, a1, a2, a3, a4, a5, a6, a7, a8].into_boxed_slice())
            .await?;

        Ok(WIPICMethodResult { result })
    }
}

pub fn register_wipic_svc_handler(core: &mut ArmCore, system: &System, jvm: &Jvm) -> Result<()> {
    core.register_svc_handler(
        SVC_CATEGORY_WIPIC,
        handle_wipic_svc,
        &(system.clone(), jvm.clone(), net::new_state(), serial::new_state(), filesystem::new_state()),
    )
}

async fn clet_register(core: &mut ArmCore, jvm: &mut Jvm, function_table: u32, a1: u32) -> Result<()> {
    tracing::debug!("clet_register({function_table:#x}, {a1:#x})");

    let functions: CletFunctions = read_generic(core, function_table)?;

    let context = CletWrapperContext { core: core.clone() };
    let clet_wrapper_class = ClassDefinitionImpl::from_class_proto(CletWrapper::as_proto(), Box::new(context.clone()) as Box<_>);
    let clet_wrapper_card_class = ClassDefinitionImpl::from_class_proto(CletWrapperCard::as_proto(), Box::new(context) as Box<_>);
    jvm.register_class(Box::new(clet_wrapper_class), None).await.unwrap();
    jvm.register_class(Box::new(clet_wrapper_card_class), None).await.unwrap();

    jvm.put_static_field("net/wie/CletWrapper", "startClet", "I", functions.start_clet as i32)
        .await
        .unwrap();
    jvm.put_static_field("net/wie/CletWrapper", "pauseClet", "I", functions.pause_clet as i32)
        .await
        .unwrap();
    jvm.put_static_field("net/wie/CletWrapper", "resumeClet", "I", functions.resume_clet as i32)
        .await
        .unwrap();
    jvm.put_static_field("net/wie/CletWrapper", "destroyClet", "I", functions.destroy_clet as i32)
        .await
        .unwrap();
    jvm.put_static_field("net/wie/CletWrapper", "paintClet", "I", functions.paint_clet as i32)
        .await
        .unwrap();
    jvm.put_static_field("net/wie/CletWrapper", "handleCletEvent", "I", functions.handle_clet_event as i32)
        .await
        .unwrap();

    let main_class_name = JavaLangString::from_rust_string(jvm, "net/wie/CletWrapper").await.unwrap();
    let mut args_array = jvm.instantiate_array("Ljava/lang/String;", 1).await.unwrap();
    jvm.store_array(&mut args_array, 0, vec![main_class_name]).await.unwrap();

    let result: JvmResult<()> = jvm
        .invoke_static("org/kwis/msp/lcdui/Main", "main", "([Ljava/lang/String;)V", (args_array,))
        .await;

    if let Err(x) = result {
        return Err(JvmSupport::to_wie_err(jvm, x).await);
    }

    Ok(())
}

async fn unk0(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("stub unk0({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    // graphics

    Ok(0)
}

async fn unk3(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    // 0xcf sits with the graphics context calls (InitContext/SetContext); it was
    // hitting the unknown-SVC path and spamming a fatal log. Stubbed to 0, which
    // is what the unknown path already returned.
    tracing::debug!("stub unk3/0xcf({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(0)
}

async fn unk2(context: &mut dyn WIPICContext) -> Result<u32> {
    tracing::warn!("stub unk2");

    // OEMC_knlGetProgramInfo? get app id
    let app_id = context.system().aid().to_string();
    let result = context.alloc_raw((app_id.len() + 1) as u32)?;
    write_null_terminated_string_bytes(context, result, app_id.as_bytes())?;

    Ok(result)
}

/// `MC_imGetSurpportModeCount` (vendor export 300).
///
/// The native LGT runtime registers four DIME modes:
/// EN/S, EN/L, N123, KO.
async fn im_get_support_mode_count(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::debug!("MC_imGetSurpportModeCount({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(4)
}

/// `MC_imGetSupportedModes` (vendor export 301).
///
/// Native LGT returns a persistent `char **` table containing:
/// EN/S, EN/L, N123, KO.
async fn im_get_supported_modes(context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::debug!("MC_imGetSupportedModes({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    let existing: u32 = read_generic(context, IME_SUPPORTED_MODES_PTR)?;
    if existing != 0 {
        return Ok(existing);
    }

    // Four 32-bit pointers followed by the four NUL-terminated mode strings.
    const TABLE_SIZE: u32 = 4 * 4;
    const EN_S: &[u8] = b"EN/S";
    const EN_L: &[u8] = b"EN/L";
    const N123: &[u8] = b"N123";
    const KO: &[u8] = b"KO";

    let total_size = TABLE_SIZE
        + (EN_S.len() + 1) as u32
        + (EN_L.len() + 1) as u32
        + (N123.len() + 1) as u32
        + (KO.len() + 1) as u32;

    let memory = context.alloc(total_size)?;
    let table = context.data_ptr(memory)?;

    let en_s = table + TABLE_SIZE;
    let en_l = en_s + (EN_S.len() + 1) as u32;
    let n123 = en_l + (EN_L.len() + 1) as u32;
    let ko = n123 + (N123.len() + 1) as u32;

    write_null_terminated_string_bytes(context, en_s, EN_S)?;
    write_null_terminated_string_bytes(context, en_l, EN_L)?;
    write_null_terminated_string_bytes(context, n123, N123)?;
    write_null_terminated_string_bytes(context, ko, KO)?;

    write_generic(context, table, en_s)?;
    write_generic(context, table + 4, en_l)?;
    write_generic(context, table + 8, n123)?;
    write_generic(context, table + 12, ko)?;

    write_generic(context, IME_SUPPORTED_MODES_PTR, table)?;

    Ok(table)
}

async fn unk5(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("stub unk5({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    // media

    Ok(0)
}

/// `MC_fsTotalSpace` (canonical WIPI-C service 0x19b).
///
/// Native flow:
/// - zero a three-word dfs_df result;
/// - dfs_df() the WIPI filesystem mount;
/// - on success take the total-space word;
/// - pass the value through the common signed-size helper.
///
/// LGTH_fileTotalSpace / AND_fileTotalSpace compute `f_bsize * f_blocks`.
/// The native common helper saturates values above signed 32-bit range to
/// `INT_MAX`; a dfs_df failure becomes `-1`.
async fn fs_total_space(
    context: &mut dyn WIPICContext,
    _a0: u32,
    _a1: u32,
    _a2: u32,
    _a3: u32,
) -> Result<u32> {
    let Some(total) = context.system().filesystem().total_space().await else {
        tracing::debug!("MC_fsTotalSpace() -> -1");
        return Ok(u32::MAX);
    };

    let total = clamp_native_fs_space(total);
    tracing::debug!("MC_fsTotalSpace() -> {total}");
    Ok(total)
}

fn clamp_native_fs_space(total: u64) -> u32 {
    core::cmp::min(total, i32::MAX as u64) as u32
}

fn is_native_fs_space_ex_access(access: i32) -> bool {
    matches!(access, 1 | 2 | 3 | 100)
}

/// `MC_fsTotalSpaceEx` / `LGTC_fsTotalSpaceEx`
/// (canonical WIPI-C service 0x1a2).
///
/// Native flow:
/// - validate the access selector; valid values are 1, 2, 3 and 100;
/// - build the corresponding WIPI filesystem path;
/// - access 1 queries the "and private" device, while 2/3/100 query
///   "wipi root" with control command 5;
/// - the HAL computes `statfs.f_bsize * statfs.f_blocks`;
/// - successful values are saturated to `INT_MAX`.
///
/// WIE exposes one logical backing filesystem rather than the native physical
/// mount/device split, so every valid native access selector maps to that same
/// backing filesystem capacity.
async fn fs_total_space_ex(
    context: &mut dyn WIPICContext,
    access: i32,
) -> Result<u32> {
    if !is_native_fs_space_ex_access(access) {
        tracing::debug!("MC_fsTotalSpaceEx({access}) -> -24");
        return Ok((-24i32) as u32);
    }

    let Some(total) = context.system().filesystem().total_space().await else {
        tracing::debug!("MC_fsTotalSpaceEx({access}) -> -1");
        return Ok(u32::MAX);
    };

    let total = clamp_native_fs_space(total);
    tracing::debug!("MC_fsTotalSpaceEx({access}) -> {total}");
    Ok(total)
}

/// `MC_fsAvailableEx` / `LGTC_fsAvailableEx`
/// (canonical WIPI-C service 0x1a3).
///
/// Native flow is the same as `MC_fsTotalSpaceEx`, except filesystem control
/// command 6 is used and the HAL computes `statfs.f_bsize * statfs.f_bavail`.
/// Valid access selectors are 1, 2, 3 and 100, and successful values are
/// saturated to `INT_MAX`.
///
/// WIE exposes one logical backing filesystem rather than the native physical
/// mount/device split, so every valid native access selector maps to that same
/// backing filesystem's available capacity.
async fn fs_available_ex(
    context: &mut dyn WIPICContext,
    access: i32,
) -> Result<u32> {
    if !is_native_fs_space_ex_access(access) {
        tracing::debug!("MC_fsAvailableEx({access}) -> -24");
        return Ok((-24i32) as u32);
    }

    let Some(available) = context.system().filesystem().available_space().await else {
        tracing::debug!("MC_fsAvailableEx({access}) -> -1");
        return Ok(u32::MAX);
    };

    let available = clamp_native_fs_space(available);
    tracing::debug!("MC_fsAvailableEx({access}) -> {available}");
    Ok(available)
}

/// `MC_fsAvailable` (canonical WIPI-C service 0x19c).
///
/// Native flow:
/// - zero a three-word dfs_df result;
/// - dfs_df() the WIPI filesystem mount;
/// - on success take the available-space word;
/// - pass the value through the common signed-size helper.
///
/// LGTH_fileAvailable / AND_fileAvailable compute `f_bsize * f_bavail`.
/// The native common helper saturates values above signed 32-bit range to
/// `INT_MAX`; a dfs_df failure becomes `-1`.
async fn fs_available(
    context: &mut dyn WIPICContext,
    _a0: u32,
    _a1: u32,
    _a2: u32,
    _a3: u32,
) -> Result<u32> {
    let Some(available) = context.system().filesystem().available_space().await else {
        tracing::debug!("MC_fsAvailable() -> -1");
        return Ok(u32::MAX);
    };

    let available = clamp_native_fs_space(available);
    tracing::debug!("MC_fsAvailable() -> {available}");
    Ok(available)
}

/// `MC_imGetCurrentMode` (vendor export 303).
///
/// Native DIME returns the index of the currently selected supported mode.
async fn im_get_current_mode(context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::debug!("MC_imGetCurrentMode({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(context.system().current_input_mode())
}

/// Normalize the public `MC_imHandleInput` key to the signed byte seen by
/// `MH_IMAhandleInput`.
///
/// The native 35..57 jump table is identity. `ime_handle` then:
/// - maps signed -3 to -16,
/// - preserves '*', '#', digits, and values in its accepted unsigned range,
/// - maps other out-of-range values to -99,
/// - finally forwards only the low byte to the provider.
fn im_provider_key(key: u32) -> i8 {
    let signed = key as i32;

    let normalized = if signed == -3 {
        -16i32
    } else if signed == 42
        || signed == 35
        || (48..=57).contains(&signed)
    {
        signed
    } else {
        let range_value = key.wrapping_sub(32);
        if range_value > 65_499 {
            -99
        } else {
            signed
        }
    };

    normalized as u8 as i8
}

/// `MC_imHandleInput` (vendor export 304 / WIPI-C service 0x130).
///
/// Native maps WIPI events 502/503/504 to DIME 1/2/3. `ime_handle` then maps
/// those to provider events 2/3/4, while `MH_IMAhandleInput` accepts only
/// provider events 2 and 4. Therefore 502 and 504 are processed and 503 is
/// ignored without touching the caller's output buffers.
async fn im_handle_input(
    context: &mut dyn WIPICContext,
    key: u32,
    event: u32,
    output0: u32,
    output0_len: u32,
    output1: u32,
    output1_len: u32,
) -> Result<u32> {
    tracing::debug!(
        "MC_imHandleInput({key:#x}, {event:#x}, {output0:#x}, {output0_len:#x}, {output1:#x}, {output1_len:#x})"
    );

    let provider_event = match event {
        502 => 2,
        504 => 4,
        _ => return Ok(0),
    };

    context.write_bytes(output0, &[0])?;
    write_generic(context, output0_len, 0u32)?;
    context.write_bytes(output1, &[0])?;
    write_generic(context, output1_len, 0u32)?;

    let output = context
        .system()
        .handle_input_method(im_provider_key(key), provider_event);

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

/// `MC_imSetCurrentMode` (vendor export 302).
///
/// Native DIME accepts supported-mode indices 0..3. A valid mode becomes the
/// current mode and returns 1; an unsupported index returns 0.
async fn im_set_current_mode(context: &mut dyn WIPICContext, mode: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::debug!("MC_imSetCurrentMode({mode:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    if mode >= 4 {
        return Ok(0);
    }

    context.system().set_current_input_mode(mode);

    Ok(1)
}

async fn time_now(context: &mut dyn WIPICContext, component_class: u32) -> Result<u32> {
    let epoch_seconds = context.system().platform().now().raw() / 1000;
    tracing::debug!("LGT_timeNow({component_class:#x}) -> {epoch_seconds}");

    write_time_value(context, epoch_seconds as u32)
}

async fn time_component(_context: &mut dyn WIPICContext, name: u32) -> Result<u32> {
    tracing::debug!("LGT_timeComponent({name:#x})");

    Ok(name)
}

async fn time_convert(context: &mut dyn WIPICContext, date_time: u32, component: u32) -> Result<u32> {
    tracing::debug!("LGT_timeConvert({date_time:#x}, {component:#x})");

    let timestamp = read_time_value(context, date_time)?;
    write_time_value(context, timestamp)
}

async fn time_to_tm(context: &mut dyn WIPICContext, time_value: u32, out_ptr: u32) -> Result<i32> {
    tracing::debug!("LGT_timeToTm({time_value:#x}, {out_ptr:#x})");

    let timestamp = read_time_value(context, time_value)?;
    let (year, month, day, hour, minute, second) = unix_seconds_to_utc(timestamp as i64);
    write_generic(context, out_ptr, second)?;
    write_generic(context, out_ptr + 4, minute)?;
    write_generic(context, out_ptr + 8, hour)?;
    write_generic(context, out_ptr + 12, day)?;
    write_generic(context, out_ptr + 16, month - 1)?;
    write_generic(context, out_ptr + 20, year - 1900)?;

    Ok(0)
}

fn write_time_value(context: &mut dyn WIPICContext, timestamp: u32) -> Result<u32> {
    let time_value_ptr: u32 = read_generic(context, TIME_VALUE_PTR)?;
    let memory = if time_value_ptr != 0 {
        WIPICIndirectPtr(time_value_ptr)
    } else {
        let memory = context.alloc(4)?;
        write_generic(context, TIME_VALUE_PTR, memory.0)?;
        memory
    };
    write_generic(context, context.data_ptr(memory)?, timestamp)?;
    Ok(memory.0)
}

fn read_time_value(context: &mut dyn WIPICContext, handle: u32) -> Result<u32> {
    read_generic(context, context.data_ptr(WIPICIndirectPtr(handle))?)
}

fn unix_seconds_to_utc(timestamp: i64) -> (i32, i32, i32, i32, i32, i32) {
    let days = timestamp.div_euclid(86_400);
    let seconds_of_day = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = (seconds_of_day / 3600) as i32;
    let minute = ((seconds_of_day % 3600) / 60) as i32;
    let second = (seconds_of_day % 60) as i32;

    (year, month, day, hour, minute, second)
}

#[cfg(test)]
mod fs_total_space_tests {
    use super::{clamp_native_fs_space, is_native_fs_space_ex_access};

    #[test]
    fn native_fs_total_space_preserves_handset_scale_capacity() {
        assert_eq!(clamp_native_fs_space(32 * 1024 * 1024), 32 * 1024 * 1024);
    }

    #[test]
    fn native_fs_total_space_saturates_at_signed_int_max() {
        assert_eq!(clamp_native_fs_space(i32::MAX as u64), i32::MAX as u32);
        assert_eq!(clamp_native_fs_space(i32::MAX as u64 + 1), i32::MAX as u32);
        assert_eq!(clamp_native_fs_space(u64::MAX), i32::MAX as u32);
    }

    #[test]
    fn native_fs_total_space_ex_accepts_only_canonical_access_selectors() {
        assert!(is_native_fs_space_ex_access(1));
        assert!(is_native_fs_space_ex_access(2));
        assert!(is_native_fs_space_ex_access(3));
        assert!(is_native_fs_space_ex_access(100));

        for access in [-1, 0, 4, 99, 101, i32::MAX] {
            assert!(!is_native_fs_space_ex_access(access));
        }
    }

    #[test]
    fn native_fs_available_ex_uses_total_space_ex_access_contract() {
        for access in [1, 2, 3, 100] {
            assert!(is_native_fs_space_ex_access(access));
        }

        for access in [-1, 0, 4, 99, 101, i32::MAX] {
            assert!(!is_native_fs_space_ex_access(access));
        }
    }

    #[test]
    fn native_fs_available_uses_same_signed_int_boundary() {
        assert_eq!(clamp_native_fs_space(16 * 1024 * 1024), 16 * 1024 * 1024);
        assert_eq!(clamp_native_fs_space(i32::MAX as u64), i32::MAX as u32);
        assert_eq!(clamp_native_fs_space(i32::MAX as u64 + 1), i32::MAX as u32);
        assert_eq!(clamp_native_fs_space(u64::MAX), i32::MAX as u32);
    }
}

fn civil_from_days(days: i64) -> (i32, i32, i32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_param = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_param + 2) / 5 + 1;
    let month = month_param + if month_param < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };

    (year as i32, month as i32, day as i32)
}

async fn unk10(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("stub unk10({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    Ok(0)
}

async fn unk13(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("stub unk13({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    // kernel

    Ok(0)
}

async fn unk14(_context: &mut dyn WIPICContext, a0: u32, a1: u32, a2: u32, a3: u32) -> Result<u32> {
    tracing::warn!("stub unk14({a0:#x}, {a1:#x}, {a2:#x}, {a3:#x})");

    // media

    Ok(0)
}

#[cfg(test)]
mod im_handle_input_tests {
    use super::im_provider_key;

    #[test]
    fn im_provider_key_matches_native() {
        // Native MC_imHandleInput 35..57 jump table is identity.
        for key in 35u32..=57 {
            assert_eq!(im_provider_key(key), key as u8 as i8);
        }

        // ime_handle special signed key.
        assert_eq!(im_provider_key((-3i32) as u32), -16);

        // Other directly supplied signed negative WIPI keys are rejected to
        // provider flush key -99.
        for key in [-16i32, -7, -4, -2, -1, -99] {
            assert_eq!(im_provider_key(key as u32), -99);
        }

        // UIC masks special keys before calling the public API. These values
        // survive ime_handle and become signed again at the provider boundary.
        assert_eq!(im_provider_key(157), -99);
        assert_eq!(im_provider_key(240), -16);
        assert_eq!(im_provider_key(249), -7);
        assert_eq!(im_provider_key(252), -4);
        assert_eq!(im_provider_key(253), -3);
        assert_eq!(im_provider_key(254), -2);
        assert_eq!(im_provider_key(255), -1);

        // Accepted printable/range values remain their low byte.
        for key in [32u32, 34, 35, 36, 42, 47, 48, 57, 58, 240, 255] {
            assert_eq!(im_provider_key(key), key as u8 as i8);
        }

        assert_eq!(im_provider_key(0), -99);
        assert_eq!(im_provider_key(31), -99);
    }
}
