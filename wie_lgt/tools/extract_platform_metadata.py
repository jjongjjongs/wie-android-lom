#!/usr/bin/env python3
from pathlib import Path
import argparse
import re
import struct
import subprocess


def rust_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


DISPATCH_ONLY_METHODS = {
    "java/lang/StringBuffer": [
        ("setShared", "()V", 39, 0x00135780),
        ("getValue", "()[C", 40, 0x00135764),
    ],
    "org/kwis/msp/lcdui/Display": [
        ("serviceRepaints", "(Z)V", 33, 0x001F2C24),
        ("repaint", "(Lorg/kwis/msp/lcdui/Card;)V", 34, 0x001EFB80),
        ("repaint", "(Lorg/kwis/msp/lcdui/Card;IIII)V", 35, 0x001F0A20),
        ("eventNotify", "(III)V", 36, 0x001F2740),
        ("keyNotify", "(II)Z", 37, 0x001F13F4),
        ("pointerNotify", "(III)Z", 38, 0x001F130C),
        ("postCallSeriallyEvent", "()V", 39, 0x001F0FB8),
        ("getFreeIndex", "()I", 40, 0x001F0D74),
    ],
    "org/kwis/msp/lcdui/Image": [
        ("getDelay", "()I", 20, 0x001FFF98),
        ("decodeNextFrame", "()I", 23, 0x00203C2C),
        ("decodeFrame", "(I)Z", 24, 0x00205C6C),
        ("getImageHandle", "()I", 25, 0x001FFF34),
        ("loadImage0", "(Ljava/lang/String;)I", 26, 0x00202DD4),
    ],
    "org/kwis/msp/lcdui/JletWrapper": [
        ("<init>", "()V", 0, 0x0020D5A0),
    ],
    "org/kwis/msp/lwc/TextBoxComponent": [
        ("setString", "(Ljava/lang/String;I)V", 59, 0x00233C90),
        ("controlPopup", "()V", 64, 0x00233DCC),
    ],
    "org/kwis/msp/lwc/TextComponent": [
        ("replace", "(Ljava/lang/String;II)V", 54, 0x00241A14),
        ("setConstraint", "(I)V", 55, 0x00241948),
        ("controlCursor", "(III)V", 56, 0x00236BD8),
        ("controlInputMethodHandler", "(I)V", 57, 0x00241884),
        ("modeSetting", "(I)V", 58, 0x00236F10),
        ("setString", "(Ljava/lang/String;I)V", 59, 0x00236BA0),
        ("setSymbolPosition", "()V", 60, 0x00236D40),
        ("changeModeCard", "()V", 61, 0x00236D00),
        ("countModeYPos", "()I", 62, 0x00236C74),
        ("calcViewPortArea", "()V", 63, 0x00241680),
    ],
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("library", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    lib = args.library
    data = lib.read_bytes()

    sections = []
    for line in subprocess.check_output(["readelf", "-SW", str(lib)], text=True).splitlines():
        m = re.search(
            r'\[\s*\d+\]\s+\S+\s+\S+\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)\s+([0-9a-fA-F]+)',
            line,
        )
        if m:
            vma, off, size = (int(x, 16) for x in m.groups())
            if size:
                sections.append((vma, vma + size, off))

    def file_offset(vma: int) -> int:
        for lo, hi, off in sections:
            if lo <= vma < hi:
                return off + vma - lo
        raise ValueError(f"unmapped VMA {vma:#x}")

    def u16(vma: int) -> int:
        return struct.unpack_from("<H", data, file_offset(vma))[0]

    def u32(vma: int) -> int:
        return struct.unpack_from("<I", data, file_offset(vma))[0]

    def cstr(vma: int) -> str:
        if vma == 0:
            raise ValueError("null string pointer")
        pos = file_offset(vma)
        end = data.index(b"\0", pos)
        return data[pos:end].decode("utf-8")

    symbols = {}
    for line in subprocess.check_output(
        ["nm", "-nS", "--defined-only", str(lib)], text=True
    ).splitlines():
        parts = line.split()
        if len(parts) >= 4:
            try:
                symbols[parts[3]] = (int(parts[0], 16), int(parts[1], 16), parts[2])
            except ValueError:
                pass

    roots = {}
    for symbol, (root, size, _) in symbols.items():
        if (
            symbol.startswith("class_shared_")
            and not symbol.endswith("_fields")
            and size == 0x0C
        ):
            metadata = u32(root + 8)
            name = cstr(u32(metadata + 8))
            roots[root] = name

    classes = []
    for root, name in roots.items():
        metadata = u32(root + 8)
        flags = u32(metadata)
        superclass_ptr = u32(metadata + 0x10)
        superclass = roots.get(superclass_ptr)
        if superclass_ptr and superclass is None:
            # Native platform metadata normally stores the class_shared root here,
            # but accept a direct string pointer as well.
            superclass = cstr(superclass_ptr)

        methods_ptr = u32(metadata + 0x38)
        fields_ptr = u32(metadata + 0x3C)
        dispatch_ptr = u32(metadata + 0x0C)

        dispatch = []
        if dispatch_ptr:
            dispatch_symbol = next(
                (
                    (symbol, size)
                    for symbol, (address, size, _) in symbols.items()
                    if symbol.startswith("dt_") and address == dispatch_ptr
                ),
                None,
            )
            if dispatch_symbol is None:
                raise ValueError(f"{name}: dispatch table {dispatch_ptr:#x} has no dt_* symbol")

            _, dispatch_size = dispatch_symbol
            if dispatch_size < 4 or dispatch_size % 4:
                raise ValueError(f"{name}: invalid dispatch table size {dispatch_size:#x}")
            if u32(dispatch_ptr) != root:
                raise ValueError(
                    f"{name}: dispatch header {u32(dispatch_ptr):#x} != root {root:#x}"
                )

            dispatch = [
                u32(dispatch_ptr + 4 + slot * 4)
                for slot in range(dispatch_size // 4 - 1)
            ]

        fields = []
        if fields_ptr:
            count = u32(fields_ptr)
            for index in range(count):
                row = fields_ptr + 4 + index * 20
                owner = u32(row)
                if owner != root:
                    raise ValueError(
                        f"{name}: field[{index}] owner {owner:#x} != root {root:#x}"
                    )
                fields.append(
                    {
                        "name": cstr(u32(row + 4)),
                        "descriptor": cstr(u32(row + 8)),
                        "flags": u32(row + 0x0C),
                        "slot": u32(row + 0x10),
                    }
                )

        methods = []
        if methods_ptr:
            count = u32(methods_ptr)
            for index in range(count):
                row = methods_ptr + 4 + index * 28
                owner = u32(row)
                if owner != root:
                    raise ValueError(
                        f"{name}: method[{index}] owner {owner:#x} != root {root:#x}"
                    )
                methods.append(
                    {
                        "name": cstr(u32(row + 4)),
                        "descriptor": cstr(u32(row + 8)),
                        "flags": u32(row + 0x0C),
                        "slot": u32(row + 0x10),
                        "entry": u32(row + 0x14),
                    }
                )

        classes.append(
            {
                "name": name,
                "superclass": superclass,
                "flags": flags,
                "get_class": u32(metadata + 0x30),
                "get_raw_class": u32(metadata + 0x34),
                "fields": fields,
                "methods": methods,
                "dispatch": dispatch,
            }
        )

    classes.sort(key=lambda x: x["name"])

    lines = []
    lines.append("//! Static metadata extracted from the original LGT `liblgt_system.so`.")
    lines.append("//!")
    lines.append("//! Generated by `wie_lgt/tools/extract_platform_metadata.py`.")
    lines.append("//! Do not edit the generated class/member tables by hand.")
    lines.append("")
    lines.append("#[derive(Debug, Clone, Copy)]")
    lines.append("pub struct PlatformField {")
    lines.append("    pub name: &'static str,")
    lines.append("    pub descriptor: &'static str,")
    lines.append("    pub flags: u32,")
    lines.append("    pub slot: u32,")
    lines.append("}")
    lines.append("")
    lines.append("#[derive(Debug, Clone, Copy)]")
    lines.append("pub struct PlatformMethod {")
    lines.append("    pub name: &'static str,")
    lines.append("    pub descriptor: &'static str,")
    lines.append("    pub flags: u32,")
    lines.append("    pub slot: u32,")
    lines.append("    pub entry: u32,")
    lines.append("}")
    lines.append("")
    lines.append("#[derive(Debug, Clone, Copy)]")
    lines.append("pub struct PlatformDispatchMethod {")
    lines.append("    pub name: &'static str,")
    lines.append("    pub descriptor: &'static str,")
    lines.append("    pub slot: u32,")
    lines.append("    pub entry: u32,")
    lines.append("}")
    lines.append("")
    lines.append("#[derive(Debug)]")
    lines.append("pub struct PlatformClass {")
    lines.append("    pub name: &'static str,")
    lines.append("    pub superclass: Option<&'static str>,")
    lines.append("    pub flags: u32,")
    lines.append("    pub get_class: u32,")
    lines.append("    pub get_raw_class: u32,")
    lines.append("    pub fields: &'static [PlatformField],")
    lines.append("    pub methods: &'static [PlatformMethod],")
    lines.append("    pub dispatch_methods: &'static [PlatformDispatchMethod],")
    lines.append("    pub dispatch: &'static [u32],")
    lines.append("}")
    lines.append("")

    for index, cls in enumerate(classes):
        if cls["fields"]:
            lines.append(f"static FIELDS_{index}: &[PlatformField] = &[")
            for field in cls["fields"]:
                lines.append(
                    "    PlatformField { "
                    f"name: {rust_string(field['name'])}, "
                    f"descriptor: {rust_string(field['descriptor'])}, "
                    f"flags: {field['flags']:#010x}, "
                    f"slot: {field['slot']} "
                    "},"
                )
            lines.append("];")
        else:
            lines.append(f"static FIELDS_{index}: &[PlatformField] = &[];")
        lines.append("")

        if cls["methods"]:
            lines.append(f"static METHODS_{index}: &[PlatformMethod] = &[")
            for method in cls["methods"]:
                lines.append(
                    "    PlatformMethod { "
                    f"name: {rust_string(method['name'])}, "
                    f"descriptor: {rust_string(method['descriptor'])}, "
                    f"flags: {method['flags']:#010x}, "
                    f"slot: {method['slot']}, "
                    f"entry: {method['entry']:#010x} "
                    "},"
                )
            lines.append("];")
        else:
            lines.append(f"static METHODS_{index}: &[PlatformMethod] = &[];")
        lines.append("")

        dispatch_only = DISPATCH_ONLY_METHODS.get(cls["name"], [])
        if dispatch_only:
            lines.append(f"static DISPATCH_METHODS_{index}: &[PlatformDispatchMethod] = &[")
            for name, descriptor, slot, entry in dispatch_only:
                if slot >= len(cls["dispatch"]) or cls["dispatch"][slot] != entry:
                    raise ValueError(
                        f"{cls['name']}: dispatch-only method {name}{descriptor} "
                        f"slot {slot} expected {entry:#x}, got "
                        f"{cls['dispatch'][slot] if slot < len(cls['dispatch']) else None}"
                    )
                lines.append(
                    "    PlatformDispatchMethod { "
                    f"name: {rust_string(name)}, "
                    f"descriptor: {rust_string(descriptor)}, "
                    f"slot: {slot}, "
                    f"entry: {entry:#010x} "
                    "},"
                )
            lines.append("];")
        else:
            lines.append(f"static DISPATCH_METHODS_{index}: &[PlatformDispatchMethod] = &[];")
        lines.append("")

        if cls["dispatch"]:
            values = ", ".join(f"{entry:#010x}" for entry in cls["dispatch"])
            lines.append(f"static DISPATCH_{index}: &[u32] = &[{values}];")
        else:
            lines.append(f"static DISPATCH_{index}: &[u32] = &[];")
        lines.append("")

    lines.append("pub static PLATFORM_CLASSES: &[PlatformClass] = &[")
    for index, cls in enumerate(classes):
        superclass = (
            f"Some({rust_string(cls['superclass'])})"
            if cls["superclass"] is not None
            else "None"
        )
        lines.append("    PlatformClass {")
        lines.append(f"        name: {rust_string(cls['name'])},")
        lines.append(f"        superclass: {superclass},")
        lines.append(f"        flags: {cls['flags']:#010x},")
        lines.append(f"        get_class: {cls['get_class']:#010x},")
        lines.append(f"        get_raw_class: {cls['get_raw_class']:#010x},")
        lines.append(f"        fields: FIELDS_{index},")
        lines.append(f"        methods: METHODS_{index},")
        lines.append(f"        dispatch_methods: DISPATCH_METHODS_{index},")
        lines.append(f"        dispatch: DISPATCH_{index},")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    lines.append("pub fn platform_class(name: &str) -> Option<&'static PlatformClass> {")
    lines.append("    PLATFORM_CLASSES.iter().find(|class| class.name == name)")
    lines.append("}")
    lines.append("")
    lines.append("impl PlatformClass {")
    lines.append(
        "    pub fn field(&self, name: &str, descriptor: &str, want_static: bool) -> Option<&PlatformField> {"
    )
    lines.append("        self.fields.iter().find(|field| {")
    lines.append("            field.name == name")
    lines.append("                && field.descriptor == descriptor")
    lines.append("                && ((field.flags & 0x8) != 0) == want_static")
    lines.append("        })")
    lines.append("    }")
    lines.append("")
    lines.append(
        "    pub fn method(&self, name: &str, descriptor: &str) -> Option<&PlatformMethod> {"
    )
    lines.append("        self.methods")
    lines.append("            .iter()")
    lines.append(
        "            .find(|method| method.name == name && method.descriptor == descriptor)"
    )
    lines.append("    }")
    lines.append("")
    lines.append("    pub fn dispatch_method(&self, slot: u32) -> Option<(&'static str, &'static str)> {")
    lines.append("        let mut class = self;")
    lines.append("        loop {")
    lines.append("            if let Some(entry) = class.dispatch.get(slot as usize).copied()")
    lines.append("                && entry != 0")
    lines.append("            {")
    lines.append("                if let Some(method) = PLATFORM_CLASSES")
    lines.append("                    .iter()")
    lines.append("                    .flat_map(|candidate| candidate.methods)")
    lines.append("                    .find(|method| method.entry == entry)")
    lines.append("                {")
    lines.append("                    return Some((method.name, method.descriptor));")
    lines.append("                }")
    lines.append("")
    lines.append("                return class")
    lines.append("                    .dispatch_methods")
    lines.append("                    .iter()")
    lines.append("                    .find(|method| method.slot == slot && method.entry == entry)")
    lines.append("                    .map(|method| (method.name, method.descriptor));")
    lines.append("            }")
    lines.append("")
    lines.append("            class = platform_class(class.superclass?)?;")
    lines.append("        }")
    lines.append("    }")
    lines.append("")
    lines.append(
        "    pub fn virtual_method(&self, name: &str, descriptor: &str) -> Option<&PlatformMethod> {"
    )
    lines.append("        let mut class = self;")
    lines.append("        loop {")
    lines.append("            if let Some(method) = class.methods.iter().find(|method| {")
    lines.append("                method.name == name")
    lines.append("                    && method.descriptor == descriptor")
    lines.append("                    && (method.slot as u16 as i16) > 0")
    lines.append("            }) {")
    lines.append("                return Some(method);")
    lines.append("            }")
    lines.append("")
    lines.append("            class = platform_class(class.superclass?)?;")
    lines.append("        }")
    lines.append("    }")
    lines.append("}")
    lines.append("")
    lines.append("#[cfg(test)]")
    lines.append("mod tests {")
    lines.append("    use super::{PLATFORM_CLASSES, platform_class};")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn contains_original_platform_class_set() {")
    lines.append("        assert_eq!(PLATFORM_CLASSES.len(), 338);")
    lines.append('        assert!(platform_class("java/lang/Object").is_some());')
    lines.append(
        '        assert!(platform_class("org/kwis/msp/lwc/TextComponent").is_some());'
    )
    lines.append('        assert!(platform_class("no/such/Class").is_none());')
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn native_text_component_mode_slot_is_preserved() {")
    lines.append(
        '        let class = platform_class("org/kwis/msp/lwc/TextComponent").unwrap();'
    )
    lines.append('        let field = class.field("iMode", "I", false).unwrap();')
    lines.append("        assert_eq!(field.slot, 19);")
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn native_string_compare_to_slot_is_preserved() {")
    lines.append('        let class = platform_class("java/lang/String").unwrap();')
    lines.append(
        '        let method = class.method("compareTo", "(Ljava/lang/String;)I").unwrap();'
    )
    lines.append("        assert_eq!(method.slot, 16);")
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn virtual_lookup_follows_native_superclass_chain() {")
    lines.append('        let class = platform_class("org/kwis/msp/media/Clip").unwrap();')
    lines.append(
        '        let method = class.virtual_method("availableDataSize", "()I").unwrap();'
    )
    lines.append("        assert_eq!(method.slot, 32);")
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn native_dispatch_tables_are_preserved() {")
    lines.append('        let string = platform_class("java/lang/String").unwrap();')
    lines.append("        assert_eq!(string.dispatch.len(), 36);")
    lines.append('        assert_eq!(string.dispatch_method(16).unwrap().0, "compareTo");')
    lines.append('        let clip = platform_class("org/kwis/msp/media/Clip").unwrap();')
    lines.append("        assert_eq!(clip.dispatch.len(), 80);")
    lines.append('        assert_eq!(clip.dispatch_method(32).unwrap().0, "availableDataSize");')
    lines.append('        let socket = platform_class("org/kwis/msf/io/Socket").unwrap();')
    lines.append("        assert!(socket.dispatch.is_empty());")
    lines.append('        let display = platform_class("org/kwis/msp/lcdui/Display").unwrap();')
    lines.append('        assert_eq!(display.dispatch_method(33), Some(("serviceRepaints", "(Z)V")));')
    lines.append('        assert_eq!(display.dispatch_method(40), Some(("getFreeIndex", "()I")));')
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn dispatch_only_methods_are_resolved() {")
    lines.append("        let cases = [")
    for class_name, methods in DISPATCH_ONLY_METHODS.items():
        for name, descriptor, slot, _entry in methods:
            lines.append(
                "            ("
                f"{rust_string(class_name)}, "
                f"{slot}, "
                f"{rust_string(name)}, "
                f"{rust_string(descriptor)}"
                "),"
            )
    lines.append("        ];")
    lines.append("")
    lines.append("        assert_eq!(cases.len(), 28);")
    lines.append("        for (class_name, slot, name, descriptor) in cases {")
    lines.append("            let class = platform_class(class_name).unwrap();")
    lines.append("            assert_eq!(")
    lines.append("                class.dispatch_method(slot),")
    lines.append("                Some((name, descriptor)),")
    lines.append('                "{class_name} slot {slot}",')
    lines.append("            );")
    lines.append("        }")
    lines.append("    }")
    lines.append("")
    lines.append("    #[test]")
    lines.append("    fn interface_lookup_keeps_zero_slot() {")
    lines.append('        let class = platform_class("org/kwis/msf/io/Socket").unwrap();')
    lines.append(
        '        let method = class.method("getInputStream", "()Ljava/io/InputStream;").unwrap();'
    )
    lines.append("        assert_eq!(method.slot, 0);")
    lines.append("        assert_eq!(method.entry, 0);")
    lines.append("    }")
    lines.append("}")
    lines.append("")

    args.output.write_text("\n".join(lines))

    total_fields = sum(len(x["fields"]) for x in classes)
    total_methods = sum(len(x["methods"]) for x in classes)
    dispatch_classes = sum(bool(x["dispatch"]) for x in classes)
    dispatch_slots = sum(len(x["dispatch"]) for x in classes)
    print(
        f"generated {args.output}: "
        f"{len(classes)} classes, {total_fields} fields, {total_methods} methods, "
        f"{dispatch_classes} dispatch tables, {dispatch_slots} dispatch slots"
    )


if __name__ == "__main__":
    main()
