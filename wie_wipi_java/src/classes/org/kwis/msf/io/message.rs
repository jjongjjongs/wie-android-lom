use alloc::vec;

use java_class_proto::{JavaFieldProto, JavaMethodProto};
use java_runtime::classes::java::{lang::String, util::Date};
use jvm::{Array, ClassInstanceRef, Jvm, Result as JvmResult};

use wie_jvm_support::{WieJavaClassProto, WieJvmContext};

/// One datagram a message socket carries: an address and the bytes, with the
/// window into them a title reads and writes through.
///
/// A pure holder - the reference's is too. Everything about it that a title can
/// observe is set by the title itself or filled in when a message arrives, so
/// there is nothing here for the platform to decide.
// class org.kwis.msf.io.Message
pub struct Message;

impl Message {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msf/io/Message",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![
                JavaMethodProto::new("<init>", "([B)V", Self::init, Default::default()),
                JavaMethodProto::new("<init>", "(Ljava/lang/String;[B)V", Self::init_with_address, Default::default()),
                JavaMethodProto::new("<init>", "(Ljava/lang/String;[BII)V", Self::init_with_window, Default::default()),
                JavaMethodProto::new("getData", "()[B", Self::get_data, Default::default()),
                JavaMethodProto::new("getLength", "()I", Self::get_length, Default::default()),
                JavaMethodProto::new("setLength", "(I)I", Self::set_length, Default::default()),
                JavaMethodProto::new("getOffset", "()I", Self::get_offset, Default::default()),
                JavaMethodProto::new("setOffset", "(I)I", Self::set_offset, Default::default()),
                JavaMethodProto::new("getAddress", "()Ljava/lang/String;", Self::get_address, Default::default()),
                JavaMethodProto::new("setAddress", "(Ljava/lang/String;)V", Self::set_address, Default::default()),
                JavaMethodProto::new("getAddressInt", "()I", Self::get_address_int, Default::default()),
                JavaMethodProto::new("setAddressInt", "(I)V", Self::set_address_int, Default::default()),
                JavaMethodProto::new("getDate", "()Ljava/util/Date;", Self::get_date, Default::default()),
                JavaMethodProto::new("setDate", "(Ljava/util/Date;)V", Self::set_date, Default::default()),
                JavaMethodProto::new("getIndex", "()B", Self::get_index, Default::default()),
                JavaMethodProto::new("setIndex", "(B)V", Self::set_index, Default::default()),
                JavaMethodProto::new("getTeleServiceID", "()I", Self::get_tele_service_id, Default::default()),
                JavaMethodProto::new("setTeleServiceID", "(I)V", Self::set_tele_service_id, Default::default()),
                JavaMethodProto::new("getClassification", "()B", Self::get_classification, Default::default()),
                JavaMethodProto::new("setClassification", "(B)V", Self::set_classification, Default::default()),
            ],
            fields: vec![
                JavaFieldProto::new("addr", "Ljava/lang/String;", Default::default()),
                JavaFieldProto::new("data", "[B", Default::default()),
                JavaFieldProto::new("offset", "I", Default::default()),
                JavaFieldProto::new("length", "I", Default::default()),
                JavaFieldProto::new("addressInt", "I", Default::default()),
                JavaFieldProto::new("date", "Ljava/util/Date;", Default::default()),
                JavaFieldProto::new("index", "B", Default::default()),
                JavaFieldProto::new("teleServiceId", "I", Default::default()),
                JavaFieldProto::new("classification", "B", Default::default()),
            ],
            access_flags: Default::default(),
        }
    }

    async fn init(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>, data: ClassInstanceRef<Array<i8>>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::<init>({this:?}, {data:?})");

        Self::store(jvm, this, None, data, 0, None).await
    }

    async fn init_with_address(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        address: ClassInstanceRef<String>,
        data: ClassInstanceRef<Array<i8>>,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::<init>({this:?}, {address:?}, {data:?})");

        Self::store(jvm, this, Some(address), data, 0, None).await
    }

    async fn init_with_window(
        jvm: &Jvm,
        _: &mut WieJvmContext,
        this: ClassInstanceRef<Self>,
        address: ClassInstanceRef<String>,
        data: ClassInstanceRef<Array<i8>>,
        offset: i32,
        length: i32,
    ) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::<init>({this:?}, {address:?}, {data:?}, {offset}, {length})");

        Self::store(jvm, this, Some(address), data, offset, Some(length)).await
    }

    /// The length a message defaults to is all of the array it was given.
    async fn store(
        jvm: &Jvm,
        mut this: ClassInstanceRef<Self>,
        address: Option<ClassInstanceRef<String>>,
        data: ClassInstanceRef<Array<i8>>,
        offset: i32,
        length: Option<i32>,
    ) -> JvmResult<()> {
        let length = match length {
            Some(length) => length,
            None if data.is_null() => 0,
            None => jvm.array_length(&data).await? as i32,
        };

        if let Some(address) = address {
            jvm.put_field(&mut this, "addr", "Ljava/lang/String;", address).await?;
        }
        jvm.put_field(&mut this, "data", "[B", data).await?;
        jvm.put_field(&mut this, "offset", "I", offset).await?;
        jvm.put_field(&mut this, "length", "I", length).await
    }

    async fn get_data(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Array<i8>>> {
        tracing::debug!("org.kwis.msf.io.Message::getData({this:?})");

        jvm.get_field(&this, "data", "[B").await
    }

    async fn get_length(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::getLength({this:?})");

        jvm.get_field(&this, "length", "I").await
    }

    /// The setters answer with the value that was there before, which is what
    /// the reference's `(I)I` shape is for.
    async fn set_length(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, length: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::setLength({this:?}, {length})");

        let previous = jvm.get_field(&this, "length", "I").await?;
        jvm.put_field(&mut this, "length", "I", length).await?;

        Ok(previous)
    }

    async fn get_offset(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::getOffset({this:?})");

        jvm.get_field(&this, "offset", "I").await
    }

    async fn set_offset(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, offset: i32) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::setOffset({this:?}, {offset})");

        let previous = jvm.get_field(&this, "offset", "I").await?;
        jvm.put_field(&mut this, "offset", "I", offset).await?;

        Ok(previous)
    }

    async fn get_address(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<String>> {
        tracing::debug!("org.kwis.msf.io.Message::getAddress({this:?})");

        jvm.get_field(&this, "addr", "Ljava/lang/String;").await
    }

    async fn set_address(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, address: ClassInstanceRef<String>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setAddress({this:?}, {address:?})");

        jvm.put_field(&mut this, "addr", "Ljava/lang/String;", address).await
    }

    async fn get_address_int(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::getAddressInt({this:?})");

        jvm.get_field(&this, "addressInt", "I").await
    }

    async fn set_address_int(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, address: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setAddressInt({this:?}, {address})");

        jvm.put_field(&mut this, "addressInt", "I", address).await
    }

    async fn get_date(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<ClassInstanceRef<Date>> {
        tracing::debug!("org.kwis.msf.io.Message::getDate({this:?})");

        jvm.get_field(&this, "date", "Ljava/util/Date;").await
    }

    async fn set_date(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, date: ClassInstanceRef<Date>) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setDate({this:?}, {date:?})");

        jvm.put_field(&mut this, "date", "Ljava/util/Date;", date).await
    }

    async fn get_index(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i8> {
        tracing::debug!("org.kwis.msf.io.Message::getIndex({this:?})");

        jvm.get_field(&this, "index", "B").await
    }

    async fn set_index(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, index: i8) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setIndex({this:?}, {index})");

        jvm.put_field(&mut this, "index", "B", index).await
    }

    async fn get_tele_service_id(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i32> {
        tracing::debug!("org.kwis.msf.io.Message::getTeleServiceID({this:?})");

        jvm.get_field(&this, "teleServiceId", "I").await
    }

    async fn set_tele_service_id(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, id: i32) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setTeleServiceID({this:?}, {id})");

        jvm.put_field(&mut this, "teleServiceId", "I", id).await
    }

    async fn get_classification(jvm: &Jvm, _: &mut WieJvmContext, this: ClassInstanceRef<Self>) -> JvmResult<i8> {
        tracing::debug!("org.kwis.msf.io.Message::getClassification({this:?})");

        jvm.get_field(&this, "classification", "B").await
    }

    async fn set_classification(jvm: &Jvm, _: &mut WieJvmContext, mut this: ClassInstanceRef<Self>, classification: i8) -> JvmResult<()> {
        tracing::debug!("org.kwis.msf.io.Message::setClassification({this:?}, {classification})");

        jvm.put_field(&mut this, "classification", "B", classification).await
    }
}
