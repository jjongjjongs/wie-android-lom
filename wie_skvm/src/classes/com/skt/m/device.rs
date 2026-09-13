use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use jvm::{Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class com.skt.m.Device
pub struct Device;

impl Device {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "com/skt/m/Device",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, MethodAccessFlags::STATIC),
                JavaMethodProto::new("setColorMode", "(I)V", Self::set_color_mode, MethodAccessFlags::STATIC),
                JavaMethodProto::new("isBacklightEnabled", "()Z", Self::is_backlight_enabled, MethodAccessFlags::STATIC),
                JavaMethodProto::new("setBacklightEnabled", "(Z)V", Self::set_backlight_enabled, MethodAccessFlags::STATIC),
                JavaMethodProto::new("setKeyToneEnabled", "(Z)V", Self::set_key_tone_enabled, MethodAccessFlags::STATIC),
                JavaMethodProto::new("isKeyToneEnabled", "()Z", Self::is_key_tone_enabled, MethodAccessFlags::STATIC),
                JavaMethodProto::new(
                    "invokeWapBrowser",
                    "(Ljava/lang/String;)V",
                    Self::invoke_wap_browser,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("enableRestoreLCD", "(Z)V", Self::enable_restore_lcd, MethodAccessFlags::STATIC),
                JavaMethodProto::new("setKeyRepeatTime", "(II)V", Self::set_key_repeat_time, MethodAccessFlags::STATIC),
            ],
            fields: vec![JavaFieldProto::new("keyToneEnabled", "Z", FieldAccessFlags::STATIC)],
            access_flags: Default::default(),
        }
    }

    async fn cl_init(jvm: &Jvm, _context: &mut WieJvmContext) -> JvmResult<()> {
        tracing::debug!("com.skt.m.Device::<clinit>()");

        // A handset ships with its key tone on, and a title that has not
        // turned it off is entitled to read that back.
        jvm.put_static_field("com/skt/m/Device", "keyToneEnabled", "Z", true).await?;

        Ok(())
    }

    async fn set_color_mode(_jvm: &Jvm, _context: &mut WieJvmContext, mode: i32) -> JvmResult<()> {
        tracing::warn!("stub com.skt.m.Device::setColorMode({mode})");

        Ok(())
    }

    async fn is_backlight_enabled(_jvm: &Jvm, _context: &mut WieJvmContext) -> JvmResult<bool> {
        tracing::warn!("stub com.skt.m.Device::isBacklightEnabled()");

        Ok(true)
    }

    async fn set_backlight_enabled(_jvm: &Jvm, _context: &mut WieJvmContext, enabled: bool) -> JvmResult<()> {
        tracing::warn!("stub com.skt.m.Device::setBacklightEnabled({enabled:?})");

        Ok(())
    }

    /// There is no key tone to turn on or off here, but a title that sets it
    /// and reads it back has to see what it set - some drive their own sound
    /// menus off exactly that.
    async fn set_key_tone_enabled(jvm: &Jvm, _context: &mut WieJvmContext, enabled: bool) -> JvmResult<()> {
        tracing::debug!("com.skt.m.Device::setKeyToneEnabled({enabled:?})");

        jvm.put_static_field("com/skt/m/Device", "keyToneEnabled", "Z", enabled).await?;

        Ok(())
    }

    async fn is_key_tone_enabled(jvm: &Jvm, _context: &mut WieJvmContext) -> JvmResult<bool> {
        tracing::debug!("com.skt.m.Device::isKeyToneEnabled()");

        jvm.get_static_field("com/skt/m/Device", "keyToneEnabled", "Z").await
    }

    /// Hands the handset's WAP browser a URL, which is where a title sends the
    /// player to buy something or read a notice.
    ///
    /// There is no browser to hand it to, and opening one would take the
    /// player out of the emulator for a gateway that has not answered in
    /// years, so this records the URL and returns - the title carries on
    /// rather than waiting on a browser that never comes back.
    async fn invoke_wap_browser(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        url: jvm::ClassInstanceRef<java_runtime::classes::java::lang::String>,
    ) -> JvmResult<()> {
        let url = if url.is_null() {
            alloc::string::String::new()
        } else {
            jvm::runtime::JavaLangString::to_rust_string(jvm, &url).await?
        };

        tracing::warn!("com.skt.m.Device::invokeWapBrowser({url}): no browser to open it in");

        Ok(())
    }

    async fn enable_restore_lcd(_jvm: &Jvm, _context: &mut WieJvmContext, enabled: bool) -> JvmResult<()> {
        tracing::warn!("stub com.skt.m.Device::enableRestoreLCD({enabled:?})");

        Ok(())
    }

    async fn set_key_repeat_time(_jvm: &Jvm, _context: &mut WieJvmContext, delay: i32, interval: i32) -> JvmResult<()> {
        tracing::warn!("stub com.skt.m.Device::setKeyRepeatTime({delay}, {interval})");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::get_protos;

    fn protos() -> Box<[Box<[wie_jvm_support::WieJavaClassProto]>]> {
        Box::new([wie_midp::get_protos().into(), get_protos().into()])
    }

    async fn key_tone(jvm: &jvm::Jvm) -> jvm::Result<bool> {
        jvm.invoke_static("com/skt/m/Device", "isKeyToneEnabled", "()Z", ()).await
    }

    /// A handset ships with its key tone on.
    #[test]
    fn the_key_tone_starts_on() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            assert!(key_tone(&jvm).await?);

            Ok(())
        })
    }

    /// Titles drive their own sound menus off what they set here, so it has to
    /// read back.
    #[test]
    fn the_key_tone_reads_back_what_was_set() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let _: () = jvm.invoke_static("com/skt/m/Device", "setKeyToneEnabled", "(Z)V", (false,)).await?;
            assert!(!key_tone(&jvm).await?);

            let _: () = jvm.invoke_static("com/skt/m/Device", "setKeyToneEnabled", "(Z)V", (true,)).await?;
            assert!(key_tone(&jvm).await?);

            Ok(())
        })
    }

    /// There is no browser to open, but the title must come back from asking
    /// for one rather than waiting on it.
    #[test]
    fn asking_for_the_wap_browser_returns() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let url = jvm::runtime::JavaLangString::from_rust_string(&jvm, "http://wap.example/buy").await?;
            let _: () = jvm
                .invoke_static("com/skt/m/Device", "invokeWapBrowser", "(Ljava/lang/String;)V", (url,))
                .await?;

            Ok(())
        })
    }

    /// A title that passes no URL must not be answered with a panic.
    #[test]
    fn a_null_url_is_survivable() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let _: () = jvm
                .invoke_static(
                    "com/skt/m/Device",
                    "invokeWapBrowser",
                    "(Ljava/lang/String;)V",
                    (jvm::ClassInstanceRef::<java_runtime::classes::java::lang::String>::new(None),),
                )
                .await?;

            Ok(())
        })
    }
}
