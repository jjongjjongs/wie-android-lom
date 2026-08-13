use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use jvm::{ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class org.kwis.msp.lwc.ScrollbarComponent
pub struct ScrollbarComponent;

impl ScrollbarComponent {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/ScrollbarComponent",
            parent_class: Some("org/kwis/msp/lwc/Component"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new(
                    "<clinit>",
                    "()V",
                    Self::cl_init,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "<init>",
                    "()V",
                    Self::init,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(I)V",
                    Self::init_with_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "<init>",
                    "(IIIIII)V",
                    Self::init_with_values,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getDirection",
                    "()I",
                    Self::get_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setDirection",
                    "(I)V",
                    Self::set_direction,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getCurrentValue",
                    "()I",
                    Self::get_current_value,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setCurrentValue",
                    "(I)V",
                    Self::set_current_value,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getMinimum",
                    "()I",
                    Self::get_minimum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setMinimum",
                    "(I)V",
                    Self::set_minimum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getMaximum",
                    "()I",
                    Self::get_maximum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setMaximum",
                    "(I)V",
                    Self::set_maximum,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getViewAmount",
                    "()I",
                    Self::get_view_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setViewAmount",
                    "(I)V",
                    Self::set_view_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "getChangeAmount",
                    "()I",
                    Self::get_change_amount,
                    Default::default(),
                ),
                JavaMethodProto::new(
                    "setChangeAmount",
                    "(I)V",
                    Self::set_change_amount,
                    Default::default(),
                ),
            ],
            fields: vec![
                // Native Java-visible fields.
                JavaFieldProto::new(
                    "HORIZONTAL",
                    "I",
                    FieldAccessFlags::STATIC,
                ),
                JavaFieldProto::new(
                    "VERTICAL",
                    "I",
                    FieldAccessFlags::STATIC,
                ),

                // WIE-private storage for native per-instance slots.
                JavaFieldProto::new(
                    "__wieScrollbarDirection",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarCurrentValue",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarViewAmount",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarMaximum",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarMinimum",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarChangeAmount",
                    "I",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "__wieScrollbarInitialized",
                    "Z",
                    Default::default(),
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn cl_init(
        jvm: &Jvm,
        _: &mut WieJvmContext,
    ) -> JvmResult<()> {
        jvm.put_static_field(
            "org/kwis/msp/lwc/ScrollbarComponent",
            "HORIZONTAL",
            "I",
            1i32,
        )
        .await?;

        jvm.put_static_field(
            "org/kwis/msp/lwc/ScrollbarComponent",
            "VERTICAL",
            "I",
            2i32,
        )
        .await?;

        Ok(())
    }

    async fn init(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<()> {
        Self::init_with_values(
            jvm,
            context,
            this,
            2,
            0,
            1,
            0,
            10,
            1,
        )
        .await
    }

    async fn init_with_direction(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        direction: i32,
    ) -> JvmResult<()> {
        Self::init_with_values(
            jvm,
            context,
            this,
            direction,
            0,
            1,
            0,
            10,
            1,
        )
        .await
    }

    async fn init_with_values(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        direction: i32,
        current_value: i32,
        view_amount: i32,
        minimum: i32,
        maximum: i32,
        change_amount: i32,
    ) -> JvmResult<()> {
        let _: () = jvm
            .invoke_special(
                &this,
                "org/kwis/msp/lwc/Component",
                "<init>",
                "()V",
                (),
            )
            .await?;

        if direction != 1 && direction != 2 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "illegal ScrollbarComponent direction",
                )
                .await,
            );
        }

        if maximum <= minimum {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid maximum <= minimum value",
                )
                .await,
            );
        }

        if view_amount < 1 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid viewAmount < 1 value",
                )
                .await,
            );
        }

        if change_amount > view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount > viewAmount value",
                )
                .await,
            );
        }

        if current_value < minimum {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue< minimum value",
                )
                .await,
            );
        }

        if current_value > maximum - view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue > maximum - viewAmount value",
                )
                .await,
            );
        }

        if change_amount < 1 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount < 1",
                )
                .await,
            );
        }

        let mut this = this;

        // Native constructor sets Component mask bit 2.
        let mask: i32 = jvm.get_field(&this, "mask", "I").await?;
        jvm.put_field(&mut this, "mask", "I", mask | 4).await?;

        jvm.put_field(
            &mut this,
            "__wieScrollbarDirection",
            "I",
            direction,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarCurrentValue",
            "I",
            current_value,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarViewAmount",
            "I",
            view_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMaximum",
            "I",
            maximum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMinimum",
            "I",
            minimum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarChangeAmount",
            "I",
            change_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarInitialized",
            "Z",
            true,
        )
        .await?;

        Ok(())
    }

    async fn get_direction(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarDirection",
            "I",
        )
        .await
    }

    async fn set_direction(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        direction: i32,
    ) -> JvmResult<()> {
        let old: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarDirection",
                "I",
            )
            .await?;

        if old == direction {
            return Ok(());
        }

        if direction != 1 && direction != 2 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "illegal ScrollbarComponent direction",
                )
                .await,
            );
        }

        jvm.put_field(
            &mut this,
            "__wieScrollbarDirection",
            "I",
            direction,
        )
        .await?;

        Ok(())
    }

    async fn get_current_value(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarCurrentValue",
            "I",
        )
        .await
    }

    async fn set_current_value(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        value: i32,
    ) -> JvmResult<()> {
        let view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        // Native setCurrentValue clamps negative input to zero before
        // delegating to synchronized setValues().
        let value = value.max(0);

        Self::set_values(
            jvm,
            this,
            value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_minimum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarMinimum",
            "I",
        )
        .await
    }

    async fn set_minimum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        minimum: i32,
    ) -> JvmResult<()> {
        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let old_minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let mut view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let mut change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let range = maximum - minimum;

        // Native adjusts existing slots only while raising minimum.
        if minimum > old_minimum {
            if range < view_amount {
                view_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarViewAmount",
                    "I",
                    view_amount,
                )
                .await?;
            }

            if range < change_amount {
                change_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarChangeAmount",
                    "I",
                    change_amount,
                )
                .await?;
            }
        }

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_maximum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarMaximum",
            "I",
        )
        .await
    }

    async fn set_maximum(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Self>,
        maximum: i32,
    ) -> JvmResult<()> {
        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let old_maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let mut view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let mut change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        let mut current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let range = maximum - minimum;

        // Native pre-adjusts the existing state only when maximum shrinks.
        if maximum < old_maximum {
            if range < view_amount {
                view_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarViewAmount",
                    "I",
                    view_amount,
                )
                .await?;
            }

            if range < change_amount {
                change_amount = range;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarChangeAmount",
                    "I",
                    change_amount,
                )
                .await?;
            }

            let max_current = maximum - view_amount;

            if max_current < current_value {
                current_value = max_current;
                jvm.put_field(
                    &mut this,
                    "__wieScrollbarCurrentValue",
                    "I",
                    current_value,
                )
                .await?;
            }
        }

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_view_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarViewAmount",
            "I",
        )
        .await
    }

    async fn set_view_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        view_amount: i32,
    ) -> JvmResult<()> {
        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        let change_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarChangeAmount",
                "I",
            )
            .await?;

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn get_change_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
    ) -> JvmResult<i32> {
        jvm.get_field(
            &this,
            "__wieScrollbarChangeAmount",
            "I",
        )
        .await
    }

    async fn set_change_amount(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        change_amount: i32,
    ) -> JvmResult<()> {
        let current_value: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarCurrentValue",
                "I",
            )
            .await?;

        let view_amount: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarViewAmount",
                "I",
            )
            .await?;

        let minimum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMinimum",
                "I",
            )
            .await?;

        let maximum: i32 = jvm
            .get_field(
                &this,
                "__wieScrollbarMaximum",
                "I",
            )
            .await?;

        Self::set_values(
            jvm,
            this,
            current_value,
            view_amount,
            minimum,
            maximum,
            change_amount,
        )
        .await
    }

    async fn set_values(
        jvm: &Jvm,
        mut this: ClassInstanceRef<Self>,
        mut current_value: i32,
        mut view_amount: i32,
        minimum: i32,
        mut maximum: i32,
        mut change_amount: i32,
    ) -> JvmResult<()> {
        // Native ScrollbarComponent.setValues():
        // maximum <= minimum is normalized, not rejected.
        if maximum <= minimum {
            maximum = minimum + 1;
        }

        let range = maximum - minimum;

        if view_amount >= range {
            view_amount = range;
        }

        if current_value < 0 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid currentValue < 0",
                )
                .await,
            );
        }

        if view_amount <= 0 {
            view_amount = 1;
        }

        // Defensive native branch. With the preceding normalization this
        // normally cannot fire, but preserve it exactly.
        if range < view_amount {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "Invalid chAmount > viewAmount value",
                )
                .await,
            );
        }

        if minimum > current_value {
            current_value = minimum;
        }

        let max_current = maximum - view_amount;

        if max_current < current_value {
            current_value = max_current;
        }

        if change_amount > view_amount {
            change_amount = view_amount;
        }

        // Native uses IllegalArgumentException() without a message here.
        if current_value < 0 {
            return Err(
                jvm.exception(
                    "java/lang/IllegalArgumentException",
                    "",
                )
                .await,
            );
        }

        jvm.put_field(
            &mut this,
            "__wieScrollbarInitialized",
            "Z",
            true,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarCurrentValue",
            "I",
            current_value,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarViewAmount",
            "I",
            view_amount,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMinimum",
            "I",
            minimum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarMaximum",
            "I",
            maximum,
        )
        .await?;
        jvm.put_field(
            &mut this,
            "__wieScrollbarChangeAmount",
            "I",
            change_amount,
        )
        .await?;

        Ok(())
    }
}
