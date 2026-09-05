use alloc::vec;

use wie_jvm_support::WieJavaClassProto;

// Synthetic compiler marker used only by
// AnnunciatorComponent$AnnunciatorEventListener synthetic constructor.
pub struct AnnunciatorComponent1;

impl AnnunciatorComponent1 {
    pub fn as_proto() -> WieJavaClassProto {
        WieJavaClassProto {
            name: "org/kwis/msp/lwc/AnnunciatorComponent$1",
            parent_class: Some("java/lang/Object"),
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            access_flags: Default::default(),
        }
    }
}
