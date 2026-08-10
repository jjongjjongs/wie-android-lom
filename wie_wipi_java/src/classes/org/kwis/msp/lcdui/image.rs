use alloc::{boxed::Box, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto, MethodBody};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use java_runtime::classes::java::lang::String;
use jvm::{
    Array, ClassInstanceRef, JavaError, JavaValue, Jvm, Result as JvmResult,
    runtime::{JavaIoInputStream, JavaLangString},
};

use wie_backend::canvas::decode_gif_animation;
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};
use wie_midp::classes::javax::microedition::lcdui::{Graphics as MidpGraphics, Image as MidpImage};

use crate::classes::org::kwis::msp::lcdui::{Graphics, ImageObserver};

// class org.kwis.msp.lcdui.Image
pub struct Image;

impl Image {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lcdui/Image",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init_empty, MethodAccessFlags::PROTECTED),
                JavaMethodProto::new("<init>", "(Ljavax/microedition/lcdui/Image;)V", Self::init, Default::default()),
                JavaMethodProto::new(
                    "loadImage",
                    "(Ljava/lang/String;Lorg/kwis/msp/lcdui/ImageObserver;)Lorg/kwis/msp/lcdui/Image;",
                    Self::load_image,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "(II)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "(Ljava/lang/String;)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image_from_name,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "([BII)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image_from_data,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "createImage",
                    "(Lorg/kwis/msp/lcdui/Image;)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_image_from_image,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("getGraphics", "()Lorg/kwis/msp/lcdui/Graphics;", Self::get_graphics, Default::default()),
                JavaMethodProto::new("getWidth", "()I", Self::get_width, Default::default()),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, Default::default()),
                JavaMethodProto::new("isMutable", "()Z", Self::is_mutable, Default::default()),
                JavaMethodProto::new("isAnimated", "()Z", Self::is_animated, Default::default()),
                JavaMethodProto::new("play", "(Lorg/kwis/msp/lcdui/ImageObserver;)V", Self::play, Default::default()),
                JavaMethodProto::new("stop", "()V", Self::stop, Default::default()),
                JavaMethodProto::new(
                    "stopImage",
                    "(Lorg/kwis/msp/lcdui/ImageObserver;)V",
                    Self::stop_image,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new("drawImage", "(Lorg/kwis/msp/lcdui/Image;IIIIIIII)V", Self::draw_image, Default::default()),
                JavaMethodProto::new(
                    "createSubImage",
                    "(IIIIZ)Lorg/kwis/msp/lcdui/Image;",
                    Self::create_sub_image,
                    Default::default(),
                ),
                JavaMethodProto::new("setTransparentColor", "(I)V", Self::set_transparent_color, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("midpImage", "Ljavax/microedition/lcdui/Image;", Default::default()),
                JavaFieldProto::new("mutable", "Z", Default::default()),
                JavaFieldProto::new("transparentColor", "I", Default::default()),
                JavaFieldProto::new("source", "Ljava/lang/String;", Default::default()),
                JavaFieldProto::new(
                    "animationFrames",
                    "[Ljavax/microedition/lcdui/Image;",
                    Default::default(),
                ),
                JavaFieldProto::new("animationDelays", "[I", Default::default()),
                JavaFieldProto::new("frameCount", "I", Default::default()),
                JavaFieldProto::new("currentFrame", "I", Default::default()),
                JavaFieldProto::new("animationGeneration", "I", Default::default()),
                JavaFieldProto::new(
                    "animationObserver",
                    "Lorg/kwis/msp/lcdui/ImageObserver;",
                    Default::default(),
                ),
                JavaFieldProto::new(
                    "activeAnimations",
                    "Ljava/util/Vector;",
                    FieldAccessFlags::STATIC,
                ),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init_empty(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Image>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Image::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;
        jvm.put_field(&mut this, "midpImage", "Ljavax/microedition/lcdui/Image;", None).await?;
        jvm.put_field(&mut this, "mutable", "Z", false).await?;
        jvm.put_field(&mut this, "transparentColor", "I", -1).await?;
        jvm.put_field(&mut this, "source", "Ljava/lang/String;", None).await?;
        jvm.put_field(
            &mut this,
            "animationFrames",
            "[Ljavax/microedition/lcdui/Image;",
            None,
        )
        .await?;
        jvm.put_field(&mut this, "animationDelays", "[I", None).await?;
        jvm.put_field(&mut this, "frameCount", "I", 0).await?;
        jvm.put_field(&mut this, "currentFrame", "I", 0).await?;
        jvm.put_field(&mut this, "animationGeneration", "I", 0)
            .await?;
        jvm.put_field(
            &mut this,
            "animationObserver",
            "Lorg/kwis/msp/lcdui/ImageObserver;",
            None,
        )
        .await
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Image>, image: ClassInstanceRef<MidpImage>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Image::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;
        jvm.put_field(&mut this, "midpImage", "Ljavax/microedition/lcdui/Image;", image).await?;
        jvm.put_field(&mut this, "mutable", "Z", false).await?;
        jvm.put_field(&mut this, "transparentColor", "I", -1).await?;
        jvm.put_field(&mut this, "source", "Ljava/lang/String;", None).await?;
        jvm.put_field(
            &mut this,
            "animationFrames",
            "[Ljavax/microedition/lcdui/Image;",
            None,
        )
        .await?;
        jvm.put_field(&mut this, "animationDelays", "[I", None).await?;
        jvm.put_field(&mut this, "frameCount", "I", 0).await?;
        jvm.put_field(&mut this, "currentFrame", "I", 0).await?;
        jvm.put_field(&mut this, "animationGeneration", "I", 0)
            .await?;
        jvm.put_field(
            &mut this,
            "animationObserver",
            "Lorg/kwis/msp/lcdui/ImageObserver;",
            None,
        )
        .await?;

        Ok(())
    }

    async fn load_image(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        name: ClassInstanceRef<String>,
        observer: ClassInstanceRef<ImageObserver>,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::loadImage({name:?}, {observer:?})");

        if name.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "name is null").await);
        }

        // Native WipiPlayer Plus normalizes classpath resource names before
        // retaining them in the Image object.
        let mut source_name = JavaLangString::to_rust_string(jvm, &name).await?;
        if !source_name.contains(':') && !source_name.starts_with('/') {
            source_name.insert(0, '/');
        }
        let source = JavaLangString::from_rust_string(jvm, &source_name).await?;

        // Native WipiPlayer Plus returns an empty Image immediately and lets
        // ImageReader populate it asynchronously.
        let mut image: ClassInstanceRef<Image> = jvm
            .new_class("org/kwis/msp/lcdui/Image", "()V", ())
            .await?
            .into();

        jvm.put_field(
            &mut image,
            "source",
            "Ljava/lang/String;",
            source.clone(),
        )
        .await?;

        context.spawn(
            jvm,
            Box::new(ImageLoadRunner {
                image: image.clone(),
                name: source.into(),
                observer,
            }),
        )?;

        Ok(image)
    }

    async fn load_image_for_runner(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        name: ClassInstanceRef<String>,
    ) -> JvmResult<Option<ClassInstanceRef<Image>>> {
        let mut resource_name = JavaLangString::to_rust_string(jvm, &name).await?;

        if !resource_name.contains(':') {
            if !resource_name.starts_with('/') {
                resource_name.insert(0, '/');
            }

            let java_name = JavaLangString::from_rust_string(jvm, &resource_name).await?;
            let probe = jvm.new_class("org/kwis/msp/lcdui/Image", "()V", ()).await?;
            let class = jvm
                .invoke_virtual(&probe, "getClass", "()Ljava/lang/Class;", ())
                .await?;

            let resource_stream: ClassInstanceRef<java_runtime::classes::java::io::InputStream> = jvm
                .invoke_virtual(
                    &class,
                    "getResourceAsStream",
                    "(Ljava/lang/String;)Ljava/io/InputStream;",
                    (java_name,),
                )
                .await?;

            if resource_stream.is_null() {
                return Ok(None);
            }

            let data = JavaIoInputStream::read_until_end(jvm, &resource_stream).await?;
            let data_len = data.len();
            let mut data_array = jvm.instantiate_array("B", data_len).await?;

            jvm.store_array(
                &mut data_array,
                0,
                data.iter().copied()
                    .map(|x| x as i8)
                    .collect::<alloc::vec::Vec<_>>(),
            )
            .await?;

            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "([BII)Ljavax/microedition/lcdui/Image;",
                    (data_array, 0, data_len as i32),
                )
                .await?;

            let mut image: ClassInstanceRef<Image> = jvm
                .new_class(
                    "org/kwis/msp/lcdui/Image",
                    "(Ljavax/microedition/lcdui/Image;)V",
                    (midp_image,),
                )
                .await?
                .into();

            Self::attach_gif_animation(jvm, &mut image, &data).await?;

        return Ok(Some(image));
        }

        match Self::create_image_from_name(jvm, context, name).await {
            Ok(image) => Ok(Some(image)),
            Err(error) => Err(error),
        }
    }

    async fn create_image(jvm: &Jvm, _: &mut WieJvmContext, width: i32, height: i32) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({width}, {height})");

        let midp_image: ClassInstanceRef<MidpImage> = jvm
            .invoke_static(
                "javax/microedition/lcdui/Image",
                "createImage",
                "(II)Ljavax/microedition/lcdui/Image;",
                (width, height),
            )
            .await?;

        let mut instance: ClassInstanceRef<Image> = jvm
            .new_class("org/kwis/msp/lcdui/Image", "(Ljavax/microedition/lcdui/Image;)V", (midp_image,))
            .await?
            .into();
        jvm.put_field(&mut instance, "mutable", "Z", true).await?;

        Ok(instance)
    }

    async fn create_image_from_name(jvm: &Jvm, _: &mut WieJvmContext, name: ClassInstanceRef<String>) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({name:?})");

        let mut resource_name = JavaLangString::to_rust_string(jvm, &name).await?;

        if !resource_name.contains(':') {
            if !resource_name.starts_with('/') {
                resource_name.insert(0, '/');
            }

            let resource_name = JavaLangString::from_rust_string(jvm, &resource_name).await?;
            let image = jvm.new_class("org/kwis/msp/lcdui/Image", "()V", ()).await?;
            let class = jvm.invoke_virtual(&image, "getClass", "()Ljava/lang/Class;", ()).await?;
            let resource_stream = jvm
                .invoke_virtual(
                    &class,
                    "getResourceAsStream",
                    "(Ljava/lang/String;)Ljava/io/InputStream;",
                    (resource_name,),
                )
                .await?;

            let data = JavaIoInputStream::read_until_end(jvm, &resource_stream).await?;
            let data_len = data.len();
            let mut data_array = jvm.instantiate_array("B", data_len).await?;
            jvm.store_array(&mut data_array, 0, data.iter().copied().map(|x| x as i8).collect::<alloc::vec::Vec<_>>())
                .await?;

            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "([BII)Ljavax/microedition/lcdui/Image;",
                    (data_array, 0, data_len as i32),
                )
                .await?;

            let mut instance: ClassInstanceRef<Image> = jvm
                .new_class("org/kwis/msp/lcdui/Image", "(Ljavax/microedition/lcdui/Image;)V", (midp_image,))
                .await?
                .into();

            Self::attach_gif_animation(jvm, &mut instance, &data).await?;

            return Ok(instance);
        }

        if let Some(file_name) = resource_name.strip_prefix("file://") {
            let file_name = JavaLangString::from_rust_string(jvm, file_name).await?;
            let file = jvm
                .new_class(
                    "org/kwis/msp/io/File",
                    "(Ljava/lang/String;I)V",
                    (file_name, 1),
                )
                .await?;

            let size: i32 = jvm.invoke_virtual(&file, "sizeOf", "()I", ()).await?;
            let data_array = jvm.instantiate_array("B", size as usize).await?;
            let read: i32 = jvm
                .invoke_virtual(
                    &file,
                    "read",
                    "([BII)I",
                    (data_array.clone(), 0, size),
                )
                .await?;

            let _: () = jvm.invoke_virtual(&file, "close", "()V", ()).await?;

            let image_length = if read < 0 { 0 } else { read };

            let raw_data: alloc::vec::Vec<i8> =
                jvm.load_array(&data_array, 0, image_length as usize).await?;
            let raw_data: alloc::vec::Vec<u8> =
                raw_data.into_iter().map(|value| value as u8).collect();
            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "([BII)Ljavax/microedition/lcdui/Image;",
                    (data_array, 0, image_length),
                )
                .await?;

            let mut instance: ClassInstanceRef<Image> = jvm
                .new_class(
                    "org/kwis/msp/lcdui/Image",
                    "(Ljavax/microedition/lcdui/Image;)V",
                    (midp_image,),
                )
                .await?
                .into();

            Self::attach_gif_animation(jvm, &mut instance, &raw_data).await?;

            return Ok(instance);
        }

        let midp_image: ClassInstanceRef<MidpImage> = jvm
            .invoke_static(
                "javax/microedition/lcdui/Image",
                "createImage",
                "(Ljava/lang/String;)Ljavax/microedition/lcdui/Image;",
                (name,),
            )
            .await?;

        let instance = jvm
            .new_class("org/kwis/msp/lcdui/Image", "(Ljavax/microedition/lcdui/Image;)V", (midp_image,))
            .await?;

        Ok(instance.into())
    }

    async fn attach_gif_animation(
        jvm: &Jvm,
        image: &mut ClassInstanceRef<Image>,
        data: &[u8],
    ) -> JvmResult<()> {
        let animation = match decode_gif_animation(data) {
            Ok(animation) => animation,
            Err(_) => return Ok(()),
        };

        let Some(animation) = animation else {
            return Ok(());
        };

        let frame_count = animation.frames.len();
        let mut frames = jvm
            .instantiate_array("Ljavax/microedition/lcdui/Image;", frame_count)
            .await?;
        let mut delays = jvm.instantiate_array("I", frame_count).await?;

        for (index, frame) in animation.frames.into_iter().enumerate() {
            let raw = frame.image.raw();
            let midp_frame = MidpImage::create_image_instance(
                jvm,
                frame.image.width(),
                frame.image.height(),
                &raw,
                frame.image.bytes_per_pixel(),
            )
            .await?;

            jvm.store_array(&mut frames, index, [midp_frame.clone()])
                .await?;

            let delay = frame.delay_ms.min(i32::MAX as u32) as i32;
            jvm.store_array(&mut delays, index, [delay]).await?;

            if index == 0 {
                jvm.put_field(
                    image,
                    "midpImage",
                    "Ljavax/microedition/lcdui/Image;",
                    midp_frame,
                )
                .await?;
            }
        }

        jvm.put_field(
            image,
            "animationFrames",
            "[Ljavax/microedition/lcdui/Image;",
            frames,
        )
        .await?;
        jvm.put_field(image, "animationDelays", "[I", delays).await?;
        jvm.put_field(image, "frameCount", "I", frame_count as i32)
            .await?;
        jvm.put_field(image, "currentFrame", "I", 0).await?;

        Ok(())
    }

    async fn create_image_from_data(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        data: ClassInstanceRef<Array<i8>>,
        image_offset: i32,
        image_length: i32,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({data:?}, {image_offset}, {image_length})");

        let raw_data: alloc::vec::Vec<i8> = jvm
            .load_array(
                &data,
                image_offset as usize,
                image_length as usize,
            )
            .await?;
        let raw_data: alloc::vec::Vec<u8> =
            raw_data.into_iter().map(|value| value as u8).collect();

        let midp_image: ClassInstanceRef<MidpImage> = jvm
            .invoke_static(
                "javax/microedition/lcdui/Image",
                "createImage",
                "([BII)Ljavax/microedition/lcdui/Image;",
                (data, image_offset, image_length),
            )
            .await?;

        let mut instance: ClassInstanceRef<Image> = jvm
            .new_class(
                "org/kwis/msp/lcdui/Image",
                "(Ljavax/microedition/lcdui/Image;)V",
                (midp_image,),
            )
            .await?
            .into();

        Self::attach_gif_animation(jvm, &mut instance, &raw_data).await?;

        Ok(instance)
    }

    async fn create_image_from_image(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        image: ClassInstanceRef<Image>,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({image:?})");

        if image.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "image is null").await);
        }

        let width: i32 = jvm.invoke_virtual(&image, "getWidth", "()I", ()).await?;
        let height: i32 = jvm.invoke_virtual(&image, "getHeight", "()I", ()).await?;

        let instance = Self::create_image(jvm, context, width, height).await?;

        let graphics: ClassInstanceRef<Graphics> = jvm
            .invoke_virtual(
                &instance,
                "getGraphics",
                "()Lorg/kwis/msp/lcdui/Graphics;",
                (),
            )
            .await?;

        let _: () = jvm
            .invoke_virtual(
                &graphics,
                "drawImage",
                "(Lorg/kwis/msp/lcdui/Image;III)V",
                (image, 0, 0, 4),
            )
            .await?;

        Ok(instance)
    }

    async fn get_graphics(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<ClassInstanceRef<Graphics>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getGraphics({this:?})");

        let mutable: bool = jvm.get_field(&this, "mutable", "Z").await?;
        if !mutable {
            return Ok(None.into());
        }

        let midp_image: ClassInstanceRef<MidpImage> = jvm.get_field(&this, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;

        let midp_graphics: ClassInstanceRef<MidpGraphics> = jvm
            .invoke_virtual(&midp_image, "getGraphics", "()Ljavax/microedition/lcdui/Graphics;", ())
            .await?;

        let instance = jvm
            .new_class("org/kwis/msp/lcdui/Graphics", "(Ljavax/microedition/lcdui/Graphics;)V", (midp_graphics,))
            .await?;

        Ok(instance.into())
    }

    async fn get_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getWidth({this:?})");

        let midp_image: ClassInstanceRef<MidpImage> = jvm.get_field(&this, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;

        jvm.invoke_virtual(&midp_image, "getWidth", "()I", ()).await
    }

    async fn get_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msp.lcdui.Image::getHeight({this:?})");

        let midp_image: ClassInstanceRef<MidpImage> = jvm.get_field(&this, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;

        jvm.invoke_virtual(&midp_image, "getHeight", "()I", ()).await
    }

    async fn is_mutable(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lcdui.Image::isMutable({this:?})");

        jvm.get_field(&this, "mutable", "Z").await
    }

    async fn is_animated(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<bool> {
        tracing::debug!("org.kwis.msp.lcdui.Image::isAnimated({this:?})");

        let frame_count: i32 = jvm.get_field(&this, "frameCount", "I").await?;
        Ok(frame_count > 1)
    }

    async fn active_animations(jvm: &Jvm) -> JvmResult<ClassInstanceRef<()>> {
        let active: ClassInstanceRef<()> = jvm
            .get_static_field(
                "org/kwis/msp/lcdui/Image",
                "activeAnimations",
                "Ljava/util/Vector;",
            )
            .await?;

        if !active.is_null() {
            return Ok(active);
        }

        let active: ClassInstanceRef<()> =
            jvm.new_class("java/util/Vector", "()V", ()).await?.into();

        jvm.put_static_field(
            "org/kwis/msp/lcdui/Image",
            "activeAnimations",
            "Ljava/util/Vector;",
            active.clone(),
        )
        .await?;

        Ok(active)
    }

    async fn add_active_animation(
        jvm: &Jvm,
        image: &ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        let active = Self::active_animations(jvm).await?;
        let size: i32 = jvm.invoke_virtual(&active, "size", "()I", ()).await?;

        for index in 0..size {
            let current: ClassInstanceRef<Image> = jvm
                .invoke_virtual(
                    &active,
                    "elementAt",
                    "(I)Ljava/lang/Object;",
                    (index,),
                )
                .await?;

            if !current.is_null() && current.identity() == image.identity() {
                return Ok(());
            }
        }

        let _: () = jvm
            .invoke_virtual(
                &active,
                "addElement",
                "(Ljava/lang/Object;)V",
                (image.clone(),),
            )
            .await?;

        Ok(())
    }

    async fn remove_active_animation(
        jvm: &Jvm,
        image: &ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        let active = Self::active_animations(jvm).await?;
        let size: i32 = jvm.invoke_virtual(&active, "size", "()I", ()).await?;

        for index in 0..size {
            let current: ClassInstanceRef<Image> = jvm
                .invoke_virtual(
                    &active,
                    "elementAt",
                    "(I)Ljava/lang/Object;",
                    (index,),
                )
                .await?;

            if !current.is_null() && current.identity() == image.identity() {
                let _: bool = jvm
                    .invoke_virtual(
                        &active,
                        "removeElement",
                        "(Ljava/lang/Object;)Z",
                        (current,),
                    )
                    .await?;
                break;
            }
        }

        Ok(())
    }

    async fn play(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Image>,
        observer: ClassInstanceRef<ImageObserver>,
    ) -> JvmResult<()> {
        let frame_count: i32 = jvm.get_field(&this, "frameCount", "I").await?;
        if frame_count <= 1 {
            return Ok(());
        }

        // Native Image.play removes an existing ImageElement for this Image
        // before adding a new one. A generation token gives the same effect
        // for already sleeping runner tasks.
        let generation: i32 = jvm
            .get_field::<i32>(&this, "animationGeneration", "I")
            .await?
            .wrapping_add(1);

        let mut image = this.clone();
        jvm.put_field(
            &mut image,
            "animationGeneration",
            "I",
            generation,
        )
        .await?;
        jvm.put_field(
            &mut image,
            "animationObserver",
            "Lorg/kwis/msp/lcdui/ImageObserver;",
            observer.clone(),
        )
        .await?;

        Self::add_active_animation(jvm, &image).await?;

        context.spawn(
            jvm,
            Box::new(ImageAnimationRunner {
                image,
                observer,
                generation,
            }),
        )?;

        Ok(())
    }

    async fn stop(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Image>,
    ) -> JvmResult<()> {
        let frame_count: i32 = jvm.get_field(&this, "frameCount", "I").await?;
        if frame_count <= 1 {
            return Ok(());
        }

        let generation: i32 = jvm
            .get_field::<i32>(&this, "animationGeneration", "I")
            .await?
            .wrapping_add(1);

        let mut image = this.clone();
        jvm.put_field(
            &mut image,
            "animationGeneration",
            "I",
            generation,
        )
        .await?;
        jvm.put_field(
            &mut image,
            "animationObserver",
            "Lorg/kwis/msp/lcdui/ImageObserver;",
            None,
        )
        .await?;

        Self::remove_active_animation(jvm, &image).await
    }

    async fn stop_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        observer: ClassInstanceRef<ImageObserver>,
    ) -> JvmResult<()> {
        let active = Self::active_animations(jvm).await?;
        let size: i32 = jvm.invoke_virtual(&active, "size", "()I", ()).await?;

        // Iterate backwards because matching entries are removed in place.
        for index in (0..size).rev() {
            let image: ClassInstanceRef<Image> = jvm
                .invoke_virtual(
                    &active,
                    "elementAt",
                    "(I)Ljava/lang/Object;",
                    (index,),
                )
                .await?;

            if image.is_null() {
                continue;
            }

            let current_observer: ClassInstanceRef<ImageObserver> = jvm
                .get_field(
                    &image,
                    "animationObserver",
                    "Lorg/kwis/msp/lcdui/ImageObserver;",
                )
                .await?;

            let matches = if observer.is_null() {
                current_observer.is_null()
            } else {
                !current_observer.is_null()
                    && current_observer.identity() == observer.identity()
            };

            if !matches {
                continue;
            }

            let generation: i32 = jvm
                .get_field::<i32>(&image, "animationGeneration", "I")
                .await?
                .wrapping_add(1);

            let mut image_mut = image.clone();
            jvm.put_field(
                &mut image_mut,
                "animationGeneration",
                "I",
                generation,
            )
            .await?;
            jvm.put_field(
                &mut image_mut,
                "animationObserver",
                "Lorg/kwis/msp/lcdui/ImageObserver;",
                None,
            )
            .await?;

            let _: ClassInstanceRef<()> = jvm
                .invoke_virtual(
                    &active,
                    "remove",
                    "(I)Ljava/lang/Object;",
                    (index,),
                )
                .await?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn draw_image(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Image>,
        image: ClassInstanceRef<Image>,
        src_x: i32,
        src_y: i32,
        src_width: i32,
        src_height: i32,
        dest_x: i32,
        dest_y: i32,
        transform: i32,
        anchor: i32,
    ) -> JvmResult<()> {
        tracing::debug!(
            "org.kwis.msp.lcdui.Image::drawImage({this:?}, {image:?}, {src_x}, {src_y}, {src_width}, {src_height}, {dest_x}, {dest_y}, {transform}, {anchor})"
        );

        if image.is_null() {
            return Err(jvm.exception("java/lang/NullPointerException", "image is null").await);
        }

        let target: ClassInstanceRef<MidpImage> = jvm.get_field(&this, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;
        let source: ClassInstanceRef<MidpImage> = jvm.get_field(&image, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;
        let graphics: ClassInstanceRef<MidpGraphics> = jvm
            .invoke_virtual(&target, "getGraphics", "()Ljavax/microedition/lcdui/Graphics;", ())
            .await?;
        jvm.invoke_virtual(
            &graphics,
            "drawRegion",
            "(Ljavax/microedition/lcdui/Image;IIIIIIII)V",
            [
                source.into(),
                src_x.into(),
                src_y.into(),
                src_width.into(),
                src_height.into(),
                transform.into(),
                dest_x.into(),
                dest_y.into(),
                anchor.into(),
            ],
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_sub_image(
        jvm: &Jvm,
        context: &mut WieJvmContext,
        this: ClassInstanceRef<Image>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        mutable: bool,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createSubImage({this:?}, {x}, {y}, {width}, {height}, {mutable})");

        let source_width: i32 = jvm.invoke_virtual(&this, "getWidth", "()I", ()).await?;
        let source_height: i32 = jvm.invoke_virtual(&this, "getHeight", "()I", ()).await?;

        if x < 0
            || y < 0
            || width < 0
            || height < 0
            || x + width > source_width
            || y + height > source_height
        {
            return Err(jvm
                .exception("java/lang/IllegalArgumentException", "")
                .await);
        }

        let animated: bool = jvm.invoke_virtual(&this, "isAnimated", "()Z", ()).await?;
        if animated {
            return Err(jvm
                .exception(
                    "java/lang/IllegalArgumentException",
                    "Animation image cannot be editable",
                )
                .await);
        }

        let mut result = Self::create_image(jvm, context, width, height).await?;

        let target: ClassInstanceRef<MidpImage> =
            jvm.get_field(&result, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;
        let source: ClassInstanceRef<MidpImage> =
            jvm.get_field(&this, "midpImage", "Ljavax/microedition/lcdui/Image;").await?;
        let target_graphics: ClassInstanceRef<MidpGraphics> = jvm
            .invoke_virtual(
                &target,
                "getGraphics",
                "()Ljavax/microedition/lcdui/Graphics;",
                (),
            )
            .await?;

        // Native dimage_create_sub_image copies the source pixels directly.
        // It does not render through the source transparent-color key.
        let _: () = jvm
            .invoke_virtual(
                &target_graphics,
                "drawRegion",
                "(Ljavax/microedition/lcdui/Image;IIIIIIII)V",
                [
                    source.into(),
                    x.into(),
                    y.into(),
                    width.into(),
                    height.into(),
                    0.into(),
                    0.into(),
                    0.into(),
                    20.into(),
                ],
            )
            .await?;

        let transparent_color: i32 =
            jvm.get_field(&this, "transparentColor", "I").await?;
        jvm.put_field(
            &mut result,
            "transparentColor",
            "I",
            transparent_color,
        )
        .await?;

        if !mutable {
            jvm.put_field(&mut result, "mutable", "Z", false).await?;
        }

        Ok(result)
    }

    async fn set_transparent_color(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        mut this: ClassInstanceRef<Image>,
        rgb: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Image::setTransparentColor({this:?}, {rgb})");

        // Native dimage_set_trans_color stores the Java int unchanged.
        jvm.put_field(&mut this, "transparentColor", "I", rgb).await
    }

    pub async fn midp_image(jvm: &Jvm, this: &ClassInstanceRef<Image>) -> JvmResult<ClassInstanceRef<MidpImage>> {
        jvm.get_field(this, "midpImage", "Ljavax/microedition/lcdui/Image;").await
    }
}


struct ImageAnimationRunner {
    image: ClassInstanceRef<Image>,
    observer: ClassInstanceRef<ImageObserver>,
    generation: i32,
}

#[async_trait::async_trait]
impl MethodBody<JavaError, WieJvmContext> for ImageAnimationRunner {
    async fn call(
        &self,
        jvm: &Jvm,
        context: &mut WieJvmContext,
        _args: Box<[JavaValue]>,
    ) -> Result<JavaValue, JavaError> {
        const FRAME_END: i32 = 0;
        const IMAGE_END: i32 = 1;

        jvm.attach_thread(None).await?;

        loop {
            let active_generation: i32 = jvm
                .get_field(&self.image, "animationGeneration", "I")
                .await?;
            if active_generation != self.generation {
                break;
            }

            let frame_count: i32 =
                jvm.get_field(&self.image, "frameCount", "I").await?;
            if frame_count <= 1 {
                break;
            }

            let cursor: i32 =
                jvm.get_field(&self.image, "currentFrame", "I").await?;

            let delay_index = if cursor <= 0 {
                0usize
            } else {
                ((cursor - 1) % frame_count) as usize
            };

            let delays: ClassInstanceRef<Array<i32>> = jvm
                .get_field(&self.image, "animationDelays", "[I")
                .await?;

            if delays.is_null() {
                break;
            }

            let delay_values: alloc::vec::Vec<i32> =
                jvm.load_array(&delays, delay_index, 1).await?;
            let delay = delay_values[0].max(0) as u64;

            if delay != 0 {
                context.system().sleep(delay).await;
            }

            // stop(), stopImage(), or a later play() may have invalidated
            // this runner while it was sleeping.
            let active_generation: i32 = jvm
                .get_field(&self.image, "animationGeneration", "I")
                .await?;
            if active_generation != self.generation {
                break;
            }

            // Native decodeNextFrame:
            //   cursor = (cursor % frameCount) + 1
            //   decodeFrame(cursor)
            let next_cursor = cursor.rem_euclid(frame_count) + 1;
            let frame_index = (next_cursor - 1) as usize;

            let frames: ClassInstanceRef<Array<ClassInstanceRef<MidpImage>>> = jvm
                .get_field(
                    &self.image,
                    "animationFrames",
                    "[Ljavax/microedition/lcdui/Image;",
                )
                .await?;

            if frames.is_null() {
                break;
            }

            let frame_values: alloc::vec::Vec<ClassInstanceRef<MidpImage>> =
                jvm.load_array(&frames, frame_index, 1).await?;
            let frame = frame_values[0].clone();

            let mut image = self.image.clone();
            jvm.put_field(
                &mut image,
                "midpImage",
                "Ljavax/microedition/lcdui/Image;",
                frame,
            )
            .await?;
            jvm.put_field(
                &mut image,
                "currentFrame",
                "I",
                next_cursor,
            )
            .await?;

            let status = if next_cursor == frame_count {
                IMAGE_END
            } else {
                FRAME_END
            };

            if !self.observer.is_null() {
                let _: () = jvm
                    .invoke_virtual(
                        &self.observer,
                        "notify",
                        "(Lorg/kwis/msp/lcdui/Image;I)V",
                        (self.image.clone(), status),
                    )
                    .await?;
            }

            if status == IMAGE_END {
                Image::remove_active_animation(jvm, &self.image).await?;

                let active_generation: i32 = jvm
                    .get_field(&self.image, "animationGeneration", "I")
                    .await?;
                if active_generation == self.generation {
                    let mut image = self.image.clone();
                    jvm.put_field(
                        &mut image,
                        "animationObserver",
                        "Lorg/kwis/msp/lcdui/ImageObserver;",
                        None,
                    )
                    .await?;
                }

                break;
            }
        }

        Ok(JavaValue::Void)
    }
}

struct ImageLoadRunner {
    image: ClassInstanceRef<Image>,
    name: ClassInstanceRef<String>,
    observer: ClassInstanceRef<ImageObserver>,
}

#[async_trait::async_trait]
impl MethodBody<JavaError, WieJvmContext> for ImageLoadRunner {
    async fn call(
        &self,
        jvm: &Jvm,
        context: &mut WieJvmContext,
        _args: Box<[JavaValue]>,
    ) -> Result<JavaValue, JavaError> {
        const IMAGE_END: i32 = 1;
        const NOT_EXIST: i32 = -1;
        const DECODE_ERROR: i32 = -2;
        const OUT_OF_MEMORY: i32 = -3;

        jvm.attach_thread(None).await?;

        let status = match Image::load_image_for_runner(jvm, context, self.name.clone()).await {
            Ok(Some(loaded)) => {
                let midp_image: ClassInstanceRef<MidpImage> = jvm
                    .get_field(
                        &loaded,
                        "midpImage",
                        "Ljavax/microedition/lcdui/Image;",
                    )
                    .await?;
                let animation_frames: ClassInstanceRef<Array<ClassInstanceRef<MidpImage>>> = jvm
                    .get_field(
                        &loaded,
                        "animationFrames",
                        "[Ljavax/microedition/lcdui/Image;",
                    )
                    .await?;
                let animation_delays: ClassInstanceRef<Array<i32>> = jvm
                    .get_field(&loaded, "animationDelays", "[I")
                    .await?;
                let frame_count: i32 =
                    jvm.get_field(&loaded, "frameCount", "I").await?;
                let current_frame: i32 =
                    jvm.get_field(&loaded, "currentFrame", "I").await?;

                let mut image = self.image.clone();
                jvm.put_field(
                    &mut image,
                    "midpImage",
                    "Ljavax/microedition/lcdui/Image;",
                    midp_image,
                )
                .await?;
                jvm.put_field(
                    &mut image,
                    "animationFrames",
                    "[Ljavax/microedition/lcdui/Image;",
                    animation_frames,
                )
                .await?;
                jvm.put_field(
                    &mut image,
                    "animationDelays",
                    "[I",
                    animation_delays,
                )
                .await?;
                jvm.put_field(&mut image, "frameCount", "I", frame_count)
                    .await?;
                jvm.put_field(&mut image, "currentFrame", "I", current_frame)
                    .await?;

                IMAGE_END
            }
            Ok(None) => NOT_EXIST,
            Err(JavaError::JavaException(exception)) => {
                if jvm.is_instance(&*exception, "java/lang/OutOfMemoryError") {
                    OUT_OF_MEMORY
                } else if jvm.is_instance(&*exception, "java/lang/IllegalArgumentException") {
                    DECODE_ERROR
                } else if jvm.is_instance(&*exception, "java/io/IOException")
                    || jvm.is_instance(&*exception, "java/io/FileNotFoundException")
                {
                    NOT_EXIST
                } else {
                    DECODE_ERROR
                }
            }
        };

        if !self.observer.is_null() {
            let _: () = jvm
                .invoke_virtual(
                    &self.observer,
                    "notify",
                    "(Lorg/kwis/msp/lcdui/Image;I)V",
                    (self.image.clone(), status),
                )
                .await?;
        }

        Ok(JavaValue::Void)
    }
}

#[cfg(test)]
mod test {
    use alloc::boxed::Box;

    use jvm::ClassInstanceRef;
    use test_utils::run_jvm_test;
    use wie_midp::classes::javax::microedition::lcdui::Image as MidpImage;
    use wie_util::Result;


    #[test]
    fn test_transparent_color_draw_and_subimage_preservation() -> Result<()> {
        run_jvm_test(
            Box::new([wie_midp::get_protos().into(), get_protos().into()]),
            |jvm| async move {
                let source: ClassInstanceRef<Image> = jvm
                    .invoke_static(
                        "org/kwis/msp/lcdui/Image",
                        "createImage",
                        "(II)Lorg/kwis/msp/lcdui/Image;",
                        (2, 1),
                    )
                    .await?;

                let source_graphics: ClassInstanceRef<
                    crate::classes::org::kwis::msp::lcdui::Graphics,
                > = jvm
                    .invoke_virtual(
                        &source,
                        "getGraphics",
                        "()Lorg/kwis/msp/lcdui/Graphics;",
                        (),
                    )
                    .await?;

                // x0 = 0x123456, x1 = green.
                let _: () = jvm
                    .invoke_virtual(&source_graphics, "setColor", "(I)V", (0x123456,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&source_graphics, "setPixel", "(II)V", (0, 0))
                    .await?;

                let _: () = jvm
                    .invoke_virtual(&source_graphics, "setColor", "(I)V", (0x00ff00,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&source_graphics, "setPixel", "(II)V", (1, 0))
                    .await?;

                let _: () = jvm
                    .invoke_virtual(
                        &source,
                        "setTransparentColor",
                        "(I)V",
                        (0x123456,),
                    )
                    .await?;

                // Sub-image must preserve raw pixels and inherit the key.
                let sub: ClassInstanceRef<Image> = jvm
                    .invoke_virtual(
                        &source,
                        "createSubImage",
                        "(IIIIZ)Lorg/kwis/msp/lcdui/Image;",
                        (0, 0, 2, 1, false),
                    )
                    .await?;

                let sub_key: i32 =
                    jvm.get_field(&sub, "transparentColor", "I").await?;
                assert_eq!(sub_key, 0x123456);

                let sub_midp = Image::midp_image(&jvm, &sub).await?;
                let sub_backend = MidpImage::image(&jvm, &sub_midp).await?;

                let preserved = sub_backend.get_pixel(0, 0);
                assert_eq!(
                    (preserved.r, preserved.g, preserved.b),
                    (0x12, 0x34, 0x56)
                );

                // Drawing the source must treat the key pixel as transparent.
                let target: ClassInstanceRef<Image> = jvm
                    .invoke_static(
                        "org/kwis/msp/lcdui/Image",
                        "createImage",
                        "(II)Lorg/kwis/msp/lcdui/Image;",
                        (2, 1),
                    )
                    .await?;

                let target_graphics: ClassInstanceRef<
                    crate::classes::org::kwis::msp::lcdui::Graphics,
                > = jvm
                    .invoke_virtual(
                        &target,
                        "getGraphics",
                        "()Lorg/kwis/msp/lcdui/Graphics;",
                        (),
                    )
                    .await?;

                // Blue background makes the skipped key pixel observable.
                let _: () = jvm
                    .invoke_virtual(&target_graphics, "setColor", "(I)V", (0x0000ff,))
                    .await?;
                let _: () = jvm
                    .invoke_virtual(&target_graphics, "fillRect", "(IIII)V", (0, 0, 2, 1))
                    .await?;

                let _: () = jvm
                    .invoke_virtual(
                        &target_graphics,
                        "drawImage",
                        "(Lorg/kwis/msp/lcdui/Image;III)V",
                        (source, 0, 0, 20),
                    )
                    .await?;

                let target_midp = Image::midp_image(&jvm, &target).await?;
                let target_backend = MidpImage::image(&jvm, &target_midp).await?;

                let skipped = target_backend.get_pixel(0, 0);
                assert_eq!((skipped.r, skipped.g, skipped.b), (0x00, 0x00, 0xff));

                let drawn = target_backend.get_pixel(1, 0);
                assert_eq!((drawn.r, drawn.g, drawn.b), (0x00, 0xff, 0x00));

                Ok(())
            },
        )
    }

    use crate::{
        classes::org::kwis::msp::lcdui::{Graphics, Image, ImageObserver},
        get_protos,
    };


    #[test]
    fn test_animated_gif_storage_and_state() -> Result<()> {
        run_jvm_test(
            Box::new([wie_midp::get_protos().into(), get_protos().into()]),
            |jvm| async move {
                use jvm::Array;

                // Two-frame 1x1 GIF89a.
                // Frame 0: red, 20 ms
                // Frame 1: green, 40 ms
                let gif: &[u8] = &[
                    0x47, 0x49, 0x46, 0x38, 0x39, 0x61,
                    0x01, 0x00, 0x01, 0x00,
                    0x80, 0x00, 0x00,
                    0xff, 0x00, 0x00,
                    0x00, 0xff, 0x00,

                    0x21, 0xf9, 0x04, 0x00,
                    0x02, 0x00,
                    0x00, 0x00,
                    0x2c,
                    0x00, 0x00, 0x00, 0x00,
                    0x01, 0x00, 0x01, 0x00,
                    0x00,
                    0x02, 0x02, 0x44, 0x01, 0x00,

                    0x21, 0xf9, 0x04, 0x00,
                    0x04, 0x00,
                    0x00, 0x00,
                    0x2c,
                    0x00, 0x00, 0x00, 0x00,
                    0x01, 0x00, 0x01, 0x00,
                    0x00,
                    0x02, 0x02, 0x4c, 0x01, 0x00,

                    0x3b,
                ];

                let mut data = jvm.instantiate_array("B", gif.len()).await?;
                jvm.store_array(
                    &mut data,
                    0,
                    gif.iter().map(|&value| value as i8).collect::<alloc::vec::Vec<_>>(),
                )
                .await?;

                let image: ClassInstanceRef<Image> = jvm
                    .invoke_static(
                        "org/kwis/msp/lcdui/Image",
                        "createImage",
                        "([BII)Lorg/kwis/msp/lcdui/Image;",
                        (data, 0, gif.len() as i32),
                    )
                    .await?;

                let animated: bool =
                    jvm.invoke_virtual(&image, "isAnimated", "()Z", ()).await?;
                assert!(animated);

                let frame_count: i32 =
                    jvm.get_field(&image, "frameCount", "I").await?;
                let current_frame: i32 =
                    jvm.get_field(&image, "currentFrame", "I").await?;

                assert_eq!(frame_count, 2);
                assert_eq!(current_frame, 0);

                let width: i32 =
                    jvm.invoke_virtual(&image, "getWidth", "()I", ()).await?;
                let height: i32 =
                    jvm.invoke_virtual(&image, "getHeight", "()I", ()).await?;

                assert_eq!(width, 1);
                assert_eq!(height, 1);

                let delays: ClassInstanceRef<Array<i32>> =
                    jvm.get_field(&image, "animationDelays", "[I").await?;
                let delay_values: alloc::vec::Vec<i32> =
                    jvm.load_array(&delays, 0, 2).await?;

                assert_eq!(delay_values, [20, 40]);

                Ok(())
            },
        )
    }

    #[test]
    fn test_animated_gif_play_and_stop_lifecycle() -> Result<()> {
        run_jvm_test(
            Box::new([wie_midp::get_protos().into(), get_protos().into()]),
            |jvm| async move {
                // Two-frame 1x1 GIF89a.
                // Frame 0: red, 20 ms
                // Frame 1: green, 40 ms
                let gif: &[u8] = &[
                    0x47, 0x49, 0x46, 0x38, 0x39, 0x61,
                    0x01, 0x00, 0x01, 0x00,
                    0x80, 0x00, 0x00,
                    0xff, 0x00, 0x00,
                    0x00, 0xff, 0x00,

                    0x21, 0xf9, 0x04, 0x00,
                    0x02, 0x00,
                    0x00, 0x00,
                    0x2c,
                    0x00, 0x00, 0x00, 0x00,
                    0x01, 0x00, 0x01, 0x00,
                    0x00,
                    0x02, 0x02, 0x44, 0x01, 0x00,

                    0x21, 0xf9, 0x04, 0x00,
                    0x04, 0x00,
                    0x00, 0x00,
                    0x2c,
                    0x00, 0x00, 0x00, 0x00,
                    0x01, 0x00, 0x01, 0x00,
                    0x00,
                    0x02, 0x02, 0x4c, 0x01, 0x00,

                    0x3b,
                ];

                let mut data = jvm.instantiate_array("B", gif.len()).await?;
                jvm.store_array(
                    &mut data,
                    0,
                    gif.iter()
                        .map(|&value| value as i8)
                        .collect::<alloc::vec::Vec<_>>(),
                )
                .await?;

                let image: ClassInstanceRef<Image> = jvm
                    .invoke_static(
                        "org/kwis/msp/lcdui/Image",
                        "createImage",
                        "([BII)Lorg/kwis/msp/lcdui/Image;",
                        (data, 0, gif.len() as i32),
                    )
                    .await?;

                let observer: ClassInstanceRef<ImageObserver> =
                    ClassInstanceRef::new(None);

                let initial_cursor: i32 =
                    jvm.get_field(&image, "currentFrame", "I").await?;
                assert_eq!(initial_cursor, 0);

                let _: () = jvm
                    .invoke_virtual(
                        &image,
                        "play",
                        "(Lorg/kwis/msp/lcdui/ImageObserver;)V",
                        (observer.clone(),),
                    )
                    .await?;

                // Keep the test task alive while the spawned animation runner
                // advances through both native-style frame delays.
                let _: () = jvm
                    .invoke_static(
                        "java/lang/Thread",
                        "sleep",
                        "(J)V",
                        (120i64,),
                    )
                    .await?;

                // TestPlatform uses one global synthetic clock across parallel
                // tests. Give spawned animation work explicit scheduling turns
                // after the sleep deadline instead of assuming one wakeup is
                // enough for the runner to be polled.
                for _ in 0..4 {
                    let _: () = jvm
                        .invoke_static("java/lang/Thread", "yield", "()V", ())
                        .await?;
                }

                let completed_cursor: i32 =
                    jvm.get_field(&image, "currentFrame", "I").await?;
                assert_eq!(completed_cursor, 2);

                let active: ClassInstanceRef<()> = jvm
                    .get_static_field(
                        "org/kwis/msp/lcdui/Image",
                        "activeAnimations",
                        "Ljava/util/Vector;",
                    )
                    .await?;
                assert!(!active.is_null());

                let active_size: i32 =
                    jvm.invoke_virtual(&active, "size", "()I", ()).await?;
                assert_eq!(active_size, 0);

                // A new play() starts another generation. stop() immediately
                // invalidates that generation before its sleeping runner can
                // change the frame cursor.
                let _: () = jvm
                    .invoke_virtual(
                        &image,
                        "play",
                        "(Lorg/kwis/msp/lcdui/ImageObserver;)V",
                        (observer,),
                    )
                    .await?;

                let cursor_before_stop: i32 =
                    jvm.get_field(&image, "currentFrame", "I").await?;

                let _: () =
                    jvm.invoke_virtual(&image, "stop", "()V", ()).await?;

                let generation_after_stop: i32 =
                    jvm.get_field(&image, "animationGeneration", "I").await?;

                let _: () = jvm
                    .invoke_static(
                        "java/lang/Thread",
                        "sleep",
                        "(J)V",
                        (120i64,),
                    )
                    .await?;

                for _ in 0..4 {
                    let _: () = jvm
                        .invoke_static("java/lang/Thread", "yield", "()V", ())
                        .await?;
                }

                let cursor_after_wait: i32 =
                    jvm.get_field(&image, "currentFrame", "I").await?;
                assert_eq!(cursor_after_wait, cursor_before_stop);

                let generation_final: i32 =
                    jvm.get_field(&image, "animationGeneration", "I").await?;
                assert_eq!(generation_final, generation_after_stop);

                let active_size: i32 =
                    jvm.invoke_virtual(&active, "size", "()I", ()).await?;
                assert_eq!(active_size, 0);

                Ok(())
            },
        )
    }

    #[test]
    fn test_mutability_and_region_draw() -> Result<()> {
        run_jvm_test(Box::new([wie_midp::get_protos().into(), get_protos().into()]), |jvm| async move {
            let source: ClassInstanceRef<Image> = jvm
                .invoke_static("org/kwis/msp/lcdui/Image", "createImage", "(II)Lorg/kwis/msp/lcdui/Image;", (1, 1))
                .await?;
            let target: ClassInstanceRef<Image> = jvm
                .invoke_static("org/kwis/msp/lcdui/Image", "createImage", "(II)Lorg/kwis/msp/lcdui/Image;", (2, 1))
                .await?;
            assert!(jvm.invoke_virtual::<_, bool>(&source, "isMutable", "()Z", ()).await?);

            let clone: ClassInstanceRef<Image> = jvm
                .invoke_static(
                    "org/kwis/msp/lcdui/Image",
                    "createImage",
                    "(Lorg/kwis/msp/lcdui/Image;)Lorg/kwis/msp/lcdui/Image;",
                    (source.clone(),),
                )
                .await?;
            assert!(jvm.invoke_virtual::<_, bool>(&clone, "isMutable", "()Z", ()).await?);
            assert!(!jvm.invoke_virtual::<_, bool>(&source, "isAnimated", "()Z", ()).await?);

            let mutable_sub: ClassInstanceRef<Image> = jvm
                .invoke_virtual(
                    &source,
                    "createSubImage",
                    "(IIIIZ)Lorg/kwis/msp/lcdui/Image;",
                    (0, 0, 1, 1, true),
                )
                .await?;
            assert!(jvm.invoke_virtual::<_, bool>(&mutable_sub, "isMutable", "()Z", ()).await?);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&mutable_sub, "getWidth", "()I", ()).await?, 1);
            assert_eq!(jvm.invoke_virtual::<_, i32>(&mutable_sub, "getHeight", "()I", ()).await?, 1);

            let immutable_sub: ClassInstanceRef<Image> = jvm
                .invoke_virtual(
                    &source,
                    "createSubImage",
                    "(IIIIZ)Lorg/kwis/msp/lcdui/Image;",
                    (0, 0, 1, 1, false),
                )
                .await?;
            assert!(!jvm.invoke_virtual::<_, bool>(&immutable_sub, "isMutable", "()Z", ()).await?);

            let graphics: ClassInstanceRef<Graphics> = jvm.invoke_virtual(&source, "getGraphics", "()Lorg/kwis/msp/lcdui/Graphics;", ()).await?;
            let _: () = jvm.invoke_virtual(&graphics, "setColor", "(I)V", (0xff0000,)).await?;
            let _: () = jvm.invoke_virtual(&graphics, "fillRect", "(IIII)V", (0, 0, 1, 1)).await?;
            let _: () = jvm
                .invoke_virtual(
                    &target,
                    "drawImage",
                    "(Lorg/kwis/msp/lcdui/Image;IIIIIIII)V",
                    [
                        source.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        1.into(),
                        1.into(),
                        0.into(),
                        0.into(),
                        20.into(),
                    ],
                )
                .await?;

            let target_midp = Image::midp_image(&jvm, &target).await?;
            let target_backend = MidpImage::image(&jvm, &target_midp).await?;
            let pixel = target_backend.get_pixel(1, 0);
            assert_eq!((pixel.r, pixel.g, pixel.b), (0xff, 0, 0));

            Ok(())
        })
    }
}
