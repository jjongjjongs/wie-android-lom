use alloc::vec;

use java_class_proto::JavaMethodProto;
use java_constants::MethodAccessFlags;
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

// class com.xce.io.ByteToCharConverter
//
// SK-VM's copy of `sun.io.ByteToCharConverter`, which titles use to turn the
// EUC-KR bytes they ship - in resources, in save files, off a socket - into
// the chars they draw. Without it those strings reach the screen as mojibake,
// or the title fails on a class it expects the platform to provide.
//
// Only `convert` is evidenced as a platform method: the reference emulator
// registers this class and `com/xce/io/ByteToCharEUC_KR`, and carries exactly
// one `([BII[CII)I` descriptor. A title that reaches for one of `sun.io`'s
// other entry points will show up as a missing method rather than as a wrong
// answer, so nothing more is guessed at here.
pub struct ByteToCharConverter;

impl ByteToCharConverter {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "com/xce/io/ByteToCharConverter",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "()V", Self::init, Default::default()),
                JavaMethodProto::new("convert", "([BII[CII)I", Self::convert, Default::default()),
                JavaMethodProto::new(
                    "getConverter",
                    "(Ljava/lang/String;)Lcom/xce/io/ByteToCharConverter;",
                    Self::get_converter,
                    MethodAccessFlags::STATIC,
                ),
            ],
            fields: vec![],
            access_flags: Default::default(),
        }
    }

    async fn init(_jvm: &Jvm, _context: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<()> {
        tracing::debug!("com.xce.io.ByteToCharConverter::<init>({this:?})");

        Ok(())
    }

    /// The platform ships one encoding, so every converter is that one.
    async fn get_converter(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        encoding: ClassInstanceRef<java_runtime::classes::java::lang::String>,
    ) -> JvmResult<ClassInstanceRef<Self>> {
        tracing::debug!("com.xce.io.ByteToCharConverter::getConverter({encoding:?})");

        let converter = jvm.new_class("com/xce/io/ByteToCharConverter", "()V", ()).await?;

        Ok(converter.into())
    }

    /// Decodes `input[in_start..in_end]` into `output[out_start..out_end]`,
    /// returning how many chars were written.
    ///
    /// `sun.io` throws when the output buffer fills before the input runs out.
    /// This stops at `out_end` and reports what fitted instead: a title that
    /// sized its buffer correctly cannot tell the difference, and one that did
    /// not gets a short answer rather than an exception it has no handler for.
    // The argument list is `sun.io`'s, so it is as long as the descriptor says.
    #[allow(clippy::too_many_arguments)]
    async fn convert(
        jvm: &Jvm,
        _context: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        input: ClassInstanceRef<Array<i8>>,
        in_start: i32,
        in_end: i32,
        mut output: ClassInstanceRef<Array<u16>>,
        out_start: i32,
        out_end: i32,
    ) -> JvmResult<i32> {
        tracing::debug!("com.xce.io.ByteToCharConverter::convert({this:?}, {input:?}, {in_start}, {in_end}, {out_start}, {out_end})");

        if in_start < 0 || in_end < in_start || out_start < 0 || out_end < out_start {
            return Ok(0);
        }

        let bytes: alloc::vec::Vec<i8> = jvm.load_array(&input, in_start as _, (in_end - in_start) as _).await?;
        let bytes = bytemuck::cast_slice::<i8, u8>(&bytes);

        let decoded = encoding_rs::EUC_KR.decode(bytes).0;

        let chars = decoded.encode_utf16().take((out_end - out_start) as _).collect::<alloc::vec::Vec<_>>();
        let written = chars.len() as i32;

        jvm.store_array(&mut output, out_start as _, chars).await?;

        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, string::String, vec, vec::Vec};

    use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult, runtime::JavaLangString};

    use test_utils::run_jvm_test;
    use wie_util::Result;

    use crate::get_protos;

    fn protos() -> Box<[Box<[wie_jvm_support::WieJavaClassProto]>]> {
        Box::new([wie_midp::get_protos().into(), get_protos().into()])
    }

    /// Runs `class`'s `convert` over `bytes` into a `out_len` char buffer,
    /// answering what it returned and what landed in the buffer.
    async fn convert(jvm: &Jvm, class: &str, bytes: &[u8], out_len: usize) -> JvmResult<(i32, String)> {
        let converter = jvm.new_class(class, "()V", ()).await?;

        let mut input = jvm.instantiate_array("B", bytes.len() as _).await?;
        jvm.store_array(&mut input, 0, bytes.iter().map(|x| *x as i8).collect::<Vec<_>>()).await?;
        let output = jvm.instantiate_array("C", out_len as _).await?;

        let written: i32 = jvm
            .invoke_virtual(
                &converter,
                "convert",
                "([BII[CII)I",
                (input, 0i32, bytes.len() as i32, output.clone(), 0i32, out_len as i32),
            )
            .await?;

        let output: ClassInstanceRef<Array<u16>> = output.into();
        let chars: Vec<u16> = jvm.load_array(&output, 0, written.max(0) as _).await?;

        Ok((written, String::from_utf16_lossy(&chars)))
    }

    /// The encoding the whole platform is written in: 안녕 is four EUC-KR
    /// bytes and two chars.
    #[test]
    fn euc_kr_bytes_become_the_chars_they_stand_for() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let (written, text) = convert(&jvm, "com/xce/io/ByteToCharConverter", &[0xBE, 0xC8, 0xB3, 0xE7], 8).await?;

            assert_eq!(written, 2);
            assert_eq!(text, "안녕");

            Ok(())
        })
    }

    #[test]
    fn ascii_passes_through_one_byte_to_one_char() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let (written, text) = convert(&jvm, "com/xce/io/ByteToCharConverter", b"OK", 4).await?;

            assert_eq!(written, 2);
            assert_eq!(text, "OK");

            Ok(())
        })
    }

    /// An output buffer that fills first reports what fitted rather than
    /// throwing, which is the one place this departs from `sun.io`.
    #[test]
    fn a_full_output_buffer_reports_what_fitted() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let (written, text) = convert(&jvm, "com/xce/io/ByteToCharConverter", &[0xBE, 0xC8, 0xB3, 0xE7], 1).await?;

            assert_eq!(written, 1);
            assert_eq!(text, "안");

            Ok(())
        })
    }

    #[test]
    fn an_empty_range_converts_to_nothing() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let (written, text) = convert(&jvm, "com/xce/io/ByteToCharConverter", &[], 4).await?;

            assert_eq!(written, 0);
            assert_eq!(text, "");

            Ok(())
        })
    }

    /// The EUC-KR subclass is the one titles name, and it decodes the same
    /// through the `convert` it inherits.
    #[test]
    fn the_euc_kr_subclass_converts_through_its_parent() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let (written, text) = convert(&jvm, "com/xce/io/ByteToCharEUC_KR", &[0xBE, 0xC8, 0xB3, 0xE7], 4).await?;

            assert_eq!(written, 2);
            assert_eq!(text, "안녕");

            Ok(())
        })
    }

    /// The factory answers for the platform's one encoding whatever it is
    /// asked for.
    #[test]
    fn the_factory_hands_back_a_working_converter() -> Result<()> {
        run_jvm_test(protos(), |jvm| async move {
            let name = JavaLangString::from_rust_string(&jvm, "EUC-KR").await?;
            let converter: ClassInstanceRef<()> = jvm
                .invoke_static(
                    "com/xce/io/ByteToCharConverter",
                    "getConverter",
                    "(Ljava/lang/String;)Lcom/xce/io/ByteToCharConverter;",
                    (name,),
                )
                .await?;

            assert!(!converter.is_null());

            let mut input = jvm.instantiate_array("B", 2).await?;
            jvm.store_array(&mut input, 0, vec![0x4Fi8, 0x4Bi8]).await?;
            let output = jvm.instantiate_array("C", 2).await?;
            let written: i32 = jvm
                .invoke_virtual(&converter, "convert", "([BII[CII)I", (input, 0i32, 2i32, output, 0i32, 2i32))
                .await?;

            assert_eq!(written, 2);

            Ok(())
        })
    }
}
