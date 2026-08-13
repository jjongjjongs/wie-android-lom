use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

pub struct FormComponent;

impl FormComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/FormComponent",
            parent_class: Some("org/kwis/msp/lwc/ContainerComponent"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<init>",
                    "()V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;)V",
                    Self::init_with_display,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Z)V",
                    Self::init_with_packed,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(Lorg/kwis/msp/lcdui/Display;Z)V",
                    Self::init_with_display_packed,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setPacked",
                    "(Z)V",
                    Self::set_packed,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getPacked",
                    "()Z",
                    Self::get_packed,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setGab",
                    "(I)V",
                    Self::set_gab,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getGab",
                    "()I",
                    Self::get_gab,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "layout",
                    "()V",
                    Self::layout,
                    Default::default(),
                ),
            ],
            fields: vec![
                // Native platform-visible FormComponent declares only cmpScroll.
                JavaFieldProto::new(
                    "cmpScroll",
                    "Lorg/kwis/msp/lwc/ScrollbarComponent;",
                    Default::default(),
                ),

                // WIE-private equivalents of native hidden state.
                JavaFieldProto::new(
                    "__wieFormPacked",
                    "Z",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieFormGab",
                    "I",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        // Native FormComponent() -> FormComponent(true).
        Self::init_with_packed(jvm, context, this, true).await
    }

    async fn init_with_display(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<()>,
    ) -> JvmResult<()> {
        // Native FormComponent(Display) -> FormComponent(Display, true).
        Self::init_with_display_packed(
            jvm,
            context,
            this,
            display,
            true,
        )
        .await
    }

    async fn init_with_packed(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        packed: bool,
    ) -> JvmResult<()> {
        // Native FormComponent(boolean):
        //   FormComponent(Display.getDefaultDisplay(), packed)
        let display: ClassInstanceRef<()> = jvm
            .invoke_static(
                "org/kwis/msp/lcdui/Display",
                "getDefaultDisplay",
                "()Lorg/kwis/msp/lcdui/Display;",
                (),
            )
            .await?;

        Self::init_with_display_packed(
            jvm,
            context,
            this,
            display,
            packed,
        )
        .await
    }

    async fn init_with_display_packed(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        display: ClassInstanceRef<()>,
        packed: bool,
    ) -> JvmResult<()> {
        // Native main constructor:
        //
        //   ContainerComponent.<init>()
        //   +0x80 (gab)    = 0
        //   +0x64 (packed) = 0
        //   cmpScroll = new ScrollbarComponent()
        //   if (display == null) throw NPE
        //   width = display.getWidth()
        //   packed = argument
        //
        // Config/log-only paths are intentionally omitted.

        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/ContainerComponent",
                "<init>",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "__wieFormGab",
            "I",
            0i32,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "__wieFormPacked",
            "Z",
            false,
        )
        .await?;

        let scrollbar = jvm
            .instantiate_class(
                "org/kwis/msp/lwc/ScrollbarComponent",
            )
            .await?;

        let _: () = jvm
            .invoke_special(
                &scrollbar,
                "org/kwis/msp/lwc/ScrollbarComponent",
                "<init>",
                "()V",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "cmpScroll",
            "Lorg/kwis/msp/lwc/ScrollbarComponent;",
            scrollbar,
        )
        .await?;

        if display.is_null() {
            return Err(
                jvm.exception(
                    "java/lang/NullPointerException",
                    "",
                )
                .await,
            );
        }

        // Native vtable +0x64 on Display returns the width used to
        // initialize the inherited Component width slot.
        let width: i32 = jvm
            .invoke_virtual(
                &display,
                "getWidth",
                "()I",
                (),
            )
            .await?;

        jvm.put_field(
            &mut this,
            "w",
            "I",
            width,
        )
        .await?;

        jvm.put_field(
            &mut this,
            "__wieFormPacked",
            "Z",
            packed,
        )
        .await?;

        Ok(())
    }

    async fn get_gab(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieFormGab",
            "I",
        )
        .await
    }

    async fn set_gab(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        gab: i32,
    ) -> JvmResult<()> {
        // Native clamps negative values to zero.
        let gab = gab.max(0);

        let old: i32 = jvm
            .get_field(
                &this,
                "__wieFormGab",
                "I",
            )
            .await?;

        if gab == old {
            return Ok(());
        }

        jvm.put_field(
            &mut this,
            "__wieFormGab",
            "I",
            gab,
        )
        .await?;

        // Native vtable +0x70 = Component.invalidate().
        jvm.invoke_virtual(
            &this,
            "invalidate",
            "()V",
            (),
        )
        .await
    }

    async fn get_packed(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<bool> {
        jvm.get_field(
            &this,
            "__wieFormPacked",
            "Z",
        )
        .await
    }

    async fn set_packed(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        packed: bool,
    ) -> JvmResult<()> {
        // Native is an unconditional direct store.
        // No equality test, invalidate, layout, or repaint.
        jvm.put_field(
            &mut this,
            "__wieFormPacked",
            "Z",
            packed,
        )
        .await
    }

    async fn layout(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        let packed: bool = jvm
            .get_field(
                &this,
                "__wieFormPacked",
                "Z",
            )
            .await?;

        if packed {
            // Native vtable +0x118.
            jvm.invoke_virtual(
                &this,
                "layoutChildVertical",
                "()V",
                (),
            )
            .await
        } else {
            // Native vtable +0x114.
            jvm.invoke_virtual(
                &this,
                "layoutChildHorizontal",
                "()V",
                (),
            )
            .await
        }
    }
}
