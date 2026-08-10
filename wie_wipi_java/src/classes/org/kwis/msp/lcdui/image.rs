use alloc::{boxed::Box, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto, MethodBody};
use java_constants::MethodAccessFlags;
use java_runtime::classes::java::lang::String;
use jvm::{
    Array, ClassInstanceRef, JavaError, JavaValue, Jvm, Result as JvmResult,
    runtime::{JavaIoInputStream, JavaLangString},
};

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
        jvm.put_field(&mut this, "source", "Ljava/lang/String;", None).await
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Image>, image: ClassInstanceRef<MidpImage>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msp.lcdui.Image::<init>({this:?})");

        let _: () = jvm.invoke_special(&this, "java/lang/Object", "<init>", "()V", ()).await?;
        jvm.put_field(&mut this, "midpImage", "Ljavax/microedition/lcdui/Image;", image).await?;
        jvm.put_field(&mut this, "mutable", "Z", false).await?;
        jvm.put_field(&mut this, "transparentColor", "I", -1).await?;
        jvm.put_field(&mut this, "source", "Ljava/lang/String;", None).await?;

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
                data.into_iter()
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

            let image: ClassInstanceRef<Image> = jvm
                .new_class(
                    "org/kwis/msp/lcdui/Image",
                    "(Ljavax/microedition/lcdui/Image;)V",
                    (midp_image,),
                )
                .await?
                .into();

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
            jvm.store_array(&mut data_array, 0, data.into_iter().map(|x| x as i8).collect::<alloc::vec::Vec<_>>())
                .await?;

            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "([BII)Ljavax/microedition/lcdui/Image;",
                    (data_array, 0, data_len as i32),
                )
                .await?;

            let instance = jvm
                .new_class("org/kwis/msp/lcdui/Image", "(Ljavax/microedition/lcdui/Image;)V", (midp_image,))
                .await?;

            return Ok(instance.into());
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
            let midp_image: ClassInstanceRef<MidpImage> = jvm
                .invoke_static(
                    "javax/microedition/lcdui/Image",
                    "createImage",
                    "([BII)Ljavax/microedition/lcdui/Image;",
                    (data_array, 0, image_length),
                )
                .await?;

            let instance = jvm
                .new_class(
                    "org/kwis/msp/lcdui/Image",
                    "(Ljavax/microedition/lcdui/Image;)V",
                    (midp_image,),
                )
                .await?;

            return Ok(instance.into());
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

    async fn create_image_from_data(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        data: ClassInstanceRef<Array<i8>>,
        image_offset: i32,
        image_length: i32,
    ) -> JvmResult<ClassInstanceRef<Image>> {
        tracing::debug!("org.kwis.msp.lcdui.Image::createImage({data:?}, {image_offset}, {image_length})");

        let midp_image: ClassInstanceRef<MidpImage> = jvm
            .invoke_static(
                "javax/microedition/lcdui/Image",
                "createImage",
                "([BII)Ljavax/microedition/lcdui/Image;",
                (data, image_offset, image_length),
            )
            .await?;

        let instance = jvm
            .new_class("org/kwis/msp/lcdui/Image", "(Ljavax/microedition/lcdui/Image;)V", (midp_image,))
            .await?;

        Ok(instance.into())
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

    async fn is_animated(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<bool> {
        tracing::warn!("stub org.kwis.msp.lcdui.Image::isAnimated({this:?})");

        Ok(false)
    }

    async fn play(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>, observer: ClassInstanceRef<ImageObserver>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Image::play({this:?}, {observer:?})");

        Ok(())
    }

    async fn stop(_: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Image>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Image::stop({this:?})");

        Ok(())
    }

    async fn stop_image(_: &Jvm, _: &mut WieJvmContext, observer: ClassInstanceRef<ImageObserver>) -> JvmResult<()> {
        tracing::warn!("stub org.kwis.msp.lcdui.Image::stopImage({observer:?})");

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

                let mut image = self.image.clone();
                jvm.put_field(
                    &mut image,
                    "midpImage",
                    "Ljavax/microedition/lcdui/Image;",
                    midp_image,
                )
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
        classes::org::kwis::msp::lcdui::{Graphics, Image},
        get_protos,
    };

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
