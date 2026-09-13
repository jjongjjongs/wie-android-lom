use alloc::{string::String as RustString, vec};

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_constants::{FieldAccessFlags, MethodAccessFlags};
use java_runtime::classes::java::lang::String;
use jvm::{Array, ClassInstanceRef, JavaChar, Jvm, Result as JvmResult, runtime::JavaLangString};

use wie_backend::canvas;
use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class javax.microedition.lcdui.Font
pub struct Font;

impl Font {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "javax/microedition/lcdui/Font",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<clinit>", "()V", Self::cl_init, MethodAccessFlags::STATIC),
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("getHeight", "()I", Self::get_height, Default::default()),
                JavaMethodProto::new("stringWidth", "(Ljava/lang/String;)I", Self::string_width, Default::default()),
                JavaMethodProto::new("substringWidth", "(Ljava/lang/String;II)I", Self::substring_width, Default::default()),
                JavaMethodProto::new("charWidth", "(C)I", Self::char_width, Default::default()),
                JavaMethodProto::new("charsWidth", "([CII)I", Self::chars_width, Default::default()),
                JavaMethodProto::new(
                    "getFont",
                    "(III)Ljavax/microedition/lcdui/Font;",
                    Self::get_font,
                    MethodAccessFlags::STATIC,
                ),
                JavaMethodProto::new(
                    "getDefaultFont",
                    "()Ljavax/microedition/lcdui/Font;",
                    Self::get_default_font,
                    MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![
                JavaFieldProto::new("face", "I", Default::default()),
                JavaFieldProto::new("style", "I", Default::default()),
                JavaFieldProto::new("size", "I", Default::default()),
                JavaFieldProto::new("FACE_SYSTEM", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("FACE_MONOSPACE", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("FACE_PROPORTIONAL", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("STYLE_PLAIN", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("STYLE_BOLD", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("STYLE_ITALIC", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("STYLE_UNDERLINED", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("SIZE_SMALL", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("SIZE_MEDIUM", "I", FieldAccessFlags::STATIC),
                JavaFieldProto::new("SIZE_LARGE", "I", FieldAccessFlags::STATIC),
            ],
            access_flags: Default::default(),
        }
    }

    pub(crate) fn pixel_height(size: i32) -> i32 {
        match size {
            8 => 10,  // SIZE_SMALL
            0 => 12,  // SIZE_MEDIUM
            16 => 14, // SIZE_LARGE
            4096 => 16,
            8192 => 20,
            16384 => 22,
            32768 => 24,
            _ => 12,
        }
    }

    pub(crate) fn baseline(size: i32) -> i32 {
        match size {
            8 => 8,   // SIZE_SMALL
            0 => 9,   // SIZE_MEDIUM
            16 => 10, // SIZE_LARGE
            4096 => 12,
            8192 => 14,
            16384 => 15,
            32768 => 17,
            _ => 9,
        }
    }

    async fn cl_init(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Font::<clinit>");

        jvm.put_static_field("javax/microedition/lcdui/Font", "FACE_SYSTEM", "I", 0).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "FACE_MONOSPACE", "I", 32).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "FACE_PROPORTIONAL", "I", 64)
            .await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "STYLE_PLAIN", "I", 0).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "STYLE_BOLD", "I", 1).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "STYLE_ITALIC", "I", 2).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "STYLE_UNDERLINED", "I", 4).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "SIZE_MEDIUM", "I", 0).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "SIZE_SMALL", "I", 8).await?;
        jvm.put_static_field("javax/microedition/lcdui/Font", "SIZE_LARGE", "I", 16).await?;

        Ok(())
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Font>) -> JvmResult<()> {
        tracing::debug!("javax.microedition.lcdui.Font::<init>({this:?})");

        jvm.put_field(&mut this, "face", "I", 0).await?;
        jvm.put_field(&mut this, "style", "I", 0).await?;
        jvm.put_field(&mut this, "size", "I", 0).await?;

        Ok(())
    }

    async fn get_height(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("javax.microedition.lcdui.Font::getHeight({this:?})");

        let size: i32 = jvm.get_field(&this, "size", "I").await?;
        Ok(Self::pixel_height(size))
    }

    async fn get_default_font(jvm: &Jvm, _: &mut WieJvmContext) -> JvmResult<ClassInstanceRef<Self>> {
        tracing::warn!("stub javax.microedition.lcdui.Font::getDefaultFont");

        let instance = jvm.new_class("javax/microedition/lcdui/Font", "()V", []).await?;

        Ok(instance.into())
    }

    async fn get_font(jvm: &Jvm, _: &mut WieJvmContext, face: i32, style: i32, size: i32) -> JvmResult<ClassInstanceRef<Font>> {
        tracing::debug!("javax.microedition.lcdui.Font::getFont({face:?}, {style:?}, {size:?})");

        let mut instance: ClassInstanceRef<Font> = jvm.new_class("javax/microedition/lcdui/Font", "()V", []).await?.into();

        jvm.put_field(&mut instance, "face", "I", face).await?;
        jvm.put_field(&mut instance, "style", "I", style).await?;
        jvm.put_field(&mut instance, "size", "I", size).await?;

        Ok(instance)
    }

    async fn string_width(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, string: ClassInstanceRef<String>) -> JvmResult<i32> {
        tracing::debug!("javax.microedition.lcdui.Font::stringWidth({string:?})");

        let string = JavaLangString::to_rust_string(jvm, &string).await?;
        let size: i32 = jvm.get_field(&this, "size", "I").await?;

        Ok(canvas::string_width_px(&string, Self::pixel_height(size) as f32) as _)
    }

    async fn substring_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        string: ClassInstanceRef<String>,
        offset: i32,
        len: i32,
    ) -> JvmResult<i32> {
        tracing::debug!("javax.microedition.lcdui.Font::substringWidth({string:?}, {offset:?}, {len:?})");

        let string = JavaLangString::to_rust_string(jvm, &string).await?;
        let substring = string.chars().skip(offset as usize).take(len as usize).collect::<RustString>();
        let size: i32 = jvm.get_field(&this, "size", "I").await?;

        Ok(canvas::string_width_px(&substring, Self::pixel_height(size) as f32) as _)
    }

    async fn char_width(_: &Jvm, _: &mut WieJvmContext, _: ClassInstanceRef<Self>, char: JavaChar) -> JvmResult<i32> {
        tracing::warn!("stub javax.microedition.lcdui.Font::charWidth({char:?})");

        let string = RustString::from_utf16(&[char]).unwrap();

        Ok(canvas::string_width(&string, 10.0) as _)
    }

    async fn chars_width(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        chars: ClassInstanceRef<Array<JavaChar>>,
        offset: i32,
        len: i32,
    ) -> JvmResult<i32> {
        tracing::debug!("javax.microedition.lcdui.Font::charsWidth({chars:?}, {offset:?}, {len:?})");

        let chars = jvm.load_array(&chars, offset as _, len as _).await?;
        let string = RustString::from_utf16(&chars).unwrap();
        let size: i32 = jvm.get_field(&this, "size", "I").await?;

        Ok(canvas::string_width_px(&string, Self::pixel_height(size) as f32) as _)
    }
}
