//! Loader for the real LGT firmware image (`libarm32_lgt_system.so`).
//!
//! This is the P1 spike from `docs/firmware-emulation.md`: map the firmware's
//! `ET_DYN` ARM image into the same address space the game runs in, apply its
//! relocations, bind its libc/libm/allocator imports to host (Rust) HLE
//! handlers, and read back its own export table so later phases can route a
//! subsystem's calls into real firmware code instead of a Rust stand-in.
//!
//! The firmware is treated as a BIOS: it is proprietary, never committed, and
//! supplied by the user at runtime. Nothing here loads or references the binary
//! at build time, and the loader is dormant until a later phase wires it into
//! startup — so it changes no existing behaviour.
//!
//! Unlike the game's prelinked Raptor `.mod` (handled by
//! `init::load_executable`), the firmware is an ordinary ARM shared object:
//! standard `ET_DYN`, based at 0, with a `PT_DYNAMIC` segment and standard
//! `R_ARM_RELATIVE` / `R_ARM_GLOB_DAT` / `R_ARM_JUMP_SLOT` relocations. The
//! loader drives everything from `PT_DYNAMIC` so it works whether or not the
//! image still carries section headers (production `.so`s are often stripped).

use alloc::{collections::BTreeMap, format, string::String, vec, vec::Vec};

use wie_core_arm::ArmCore;
use wie_util::{Result, WieError, write_generic};

use crate::relocation::{R_ARM_ABS32, R_ARM_GLOB_DAT, R_ARM_JUMP_SLOT, R_ARM_NONE, R_ARM_RELATIVE, arm_abs32, arm_relative};

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;

const DT_NULL: u32 = 0;
const DT_PLTRELSZ: u32 = 2;
const DT_STRTAB: u32 = 5;
const DT_SYMTAB: u32 = 6;
const DT_SYMENT: u32 = 11;
const DT_REL: u32 = 17;
const DT_RELSZ: u32 = 18;
const DT_JMPREL: u32 = 23;

const STT_OBJECT: u8 = 1;
const STT_FUNC: u8 = 2;
const SHN_UNDEF: u16 = 0;

const EM_ARM: u16 = 40;
const ET_DYN: u16 = 3;
const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;

/// A firmware import (name) the loader could not bind to a host handler.
///
/// P1 records these rather than failing: it is expected that the first pass
/// leaves some standard-C or allocator names unbound until their HLE handlers
/// are stood up. A later phase turns any remaining entry here into a hard error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedImport {
    pub name: String,
    /// Guest address of the relocation slot that wanted this symbol.
    pub place: u32,
}

/// The result of mapping the firmware into the ARM address space.
pub struct FirmwareImage {
    /// Address the image was loaded at (`load_bias`); the image is linked at 0.
    pub base: u32,
    /// Guest entry point (`e_entry + base`).
    pub entry: u32,
    /// `(address, size)` of each loaded `PT_LOAD` segment.
    pub segments: Vec<(u32, u32)>,
    /// The firmware's own exported symbols, name -> guest address. This is the
    /// table re-derived against the exact binary that was loaded, replacing the
    /// build-specific address guesses in `platform_metadata`.
    pub exports: BTreeMap<String, u32>,
    /// Imports left unbound by the resolver in this pass.
    pub unresolved_imports: Vec<UnresolvedImport>,
}

impl FirmwareImage {
    /// Convenience lookup for a firmware export by name.
    pub fn export(&self, name: &str) -> Option<u32> {
        self.exports.get(name).copied()
    }
}

/// Resolves a firmware import name to a callable guest address (typically an
/// SVC trampoline minted with `ArmCore::make_svc_stub`).
///
/// Returning `Ok(None)` means "not handled yet" - the loader records the import
/// as unresolved and leaves the slot untouched, rather than failing the load.
pub trait ImportResolver {
    fn resolve(&mut self, core: &mut ArmCore, name: &str) -> Result<Option<u32>>;
}

impl<F> ImportResolver for F
where
    F: FnMut(&mut ArmCore, &str) -> Result<Option<u32>>,
{
    fn resolve(&mut self, core: &mut ArmCore, name: &str) -> Result<Option<u32>> {
        self(core, name)
    }
}

fn rd_u16(data: &[u8], off: usize) -> Result<u16> {
    let bytes = data
        .get(off..off + 2)
        .ok_or_else(|| WieError::FatalError(format!("truncated firmware ELF at {off:#x}")))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn rd_u32(data: &[u8], off: usize) -> Result<u32> {
    let bytes = data
        .get(off..off + 4)
        .ok_or_else(|| WieError::FatalError(format!("truncated firmware ELF at {off:#x}")))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

struct ProgramHeader {
    p_type: u32,
    p_offset: u32,
    p_vaddr: u32,
    p_filesz: u32,
    p_memsz: u32,
}

/// Translates a virtual address (as stored in `PT_DYNAMIC` pointers) to a file
/// offset in `data`, using the `PT_LOAD` segment that contains it.
fn vaddr_to_offset(loads: &[ProgramHeader], vaddr: u32) -> Result<usize> {
    for ph in loads {
        let start = ph.p_vaddr;
        let end = ph.p_vaddr.wrapping_add(ph.p_filesz);
        if vaddr >= start && vaddr < end {
            return Ok((ph.p_offset + (vaddr - start)) as usize);
        }
    }
    Err(WieError::FatalError(format!("firmware vaddr {vaddr:#x} is not in any PT_LOAD segment")))
}

/// Maps the firmware ELF at `base`, relocates it, binds its imports through
/// `resolver`, and returns the loaded image with its export table.
pub fn load_firmware(core: &mut ArmCore, data: &[u8], base: u32, resolver: &mut dyn ImportResolver) -> Result<FirmwareImage> {
    // ELF identification and header validation.
    if data.get(0..4) != Some(b"\x7fELF") {
        return Err(WieError::FatalError("firmware is not an ELF file".into()));
    }
    if data.get(4).copied() != Some(ELFCLASS32) {
        return Err(WieError::FatalError("firmware ELF is not 32-bit".into()));
    }
    if data.get(5).copied() != Some(ELFDATA2LSB) {
        return Err(WieError::FatalError("firmware ELF is not little-endian".into()));
    }

    let e_type = rd_u16(data, 16)?;
    let e_machine = rd_u16(data, 18)?;
    let e_entry = rd_u32(data, 24)?;
    let e_phoff = rd_u32(data, 28)? as usize;
    let e_phentsize = rd_u16(data, 42)? as usize;
    let e_phnum = rd_u16(data, 44)? as usize;

    if e_machine != EM_ARM {
        return Err(WieError::FatalError(format!("firmware ELF is not ARM (machine {e_machine})")));
    }
    if e_type != ET_DYN {
        return Err(WieError::FatalError(format!("firmware ELF is not ET_DYN (type {e_type})")));
    }
    if e_phentsize < 32 {
        return Err(WieError::FatalError(format!("firmware program header entry too small: {e_phentsize}")));
    }

    // Read the program headers.
    let mut loads = Vec::new();
    let mut dynamic: Option<ProgramHeader> = None;
    for index in 0..e_phnum {
        let off = e_phoff + index * e_phentsize;
        let ph = ProgramHeader {
            p_type: rd_u32(data, off)?,
            p_offset: rd_u32(data, off + 4)?,
            p_vaddr: rd_u32(data, off + 8)?,
            p_filesz: rd_u32(data, off + 16)?,
            p_memsz: rd_u32(data, off + 20)?,
        };
        match ph.p_type {
            PT_LOAD => loads.push(ph),
            PT_DYNAMIC => dynamic = Some(ph),
            _ => {}
        }
    }

    if loads.is_empty() {
        return Err(WieError::FatalError("firmware ELF has no PT_LOAD segments".into()));
    }
    let dynamic = dynamic.ok_or_else(|| WieError::FatalError("firmware ELF has no PT_DYNAMIC segment".into()))?;

    // Map each PT_LOAD segment at base + p_vaddr, zero-filling bss (memsz > filesz).
    let mut segments = Vec::new();
    for ph in &loads {
        let addr = base.wrapping_add(ph.p_vaddr);
        let file_start = ph.p_offset as usize;
        let file_end = file_start + ph.p_filesz as usize;
        let file_bytes = data
            .get(file_start..file_end)
            .ok_or_else(|| WieError::FatalError(format!("firmware PT_LOAD file range {file_start:#x}..{file_end:#x} out of bounds")))?;

        let mut image = vec![0u8; ph.p_memsz as usize];
        image[..file_bytes.len()].copy_from_slice(file_bytes);
        core.load(&image, addr, image.len())?;

        segments.push((addr, ph.p_memsz));
    }

    // Parse the dynamic array into the tags the loader needs.
    let dyn_off = dynamic.p_offset as usize;
    let dyn_size = dynamic.p_filesz as usize;
    let mut d_strtab = None;
    let mut d_symtab = None;
    let mut d_syment = 16u32;
    let mut d_rel = None;
    let mut d_relsz = 0u32;
    let mut d_jmprel = None;
    let mut d_pltrelsz = 0u32;

    let mut cursor = dyn_off;
    let dyn_end = dyn_off + dyn_size;
    while cursor + 8 <= dyn_end {
        let tag = rd_u32(data, cursor)?;
        let val = rd_u32(data, cursor + 4)?;
        cursor += 8;
        match tag {
            DT_NULL => break,
            DT_STRTAB => d_strtab = Some(val),
            DT_SYMTAB => d_symtab = Some(val),
            DT_SYMENT => d_syment = val,
            DT_REL => d_rel = Some(val),
            DT_RELSZ => d_relsz = val,
            DT_JMPREL => d_jmprel = Some(val),
            DT_PLTRELSZ => d_pltrelsz = val,
            _ => {}
        }
    }

    let strtab_vaddr = d_strtab.ok_or_else(|| WieError::FatalError("firmware ELF has no DT_STRTAB".into()))?;
    let symtab_vaddr = d_symtab.ok_or_else(|| WieError::FatalError("firmware ELF has no DT_SYMTAB".into()))?;
    if d_syment < 16 {
        return Err(WieError::FatalError(format!("firmware DT_SYMENT too small: {d_syment}")));
    }

    let strtab_off = vaddr_to_offset(&loads, strtab_vaddr)?;
    let symtab_off = vaddr_to_offset(&loads, symtab_vaddr)?;

    // The dynamic symbol table has no explicit count in DT_*; it ends where the
    // string table begins, the standard layout produced by every ARM linker.
    if strtab_vaddr <= symtab_vaddr {
        return Err(WieError::FatalError("firmware DT_STRTAB does not follow DT_SYMTAB".into()));
    }
    let symbol_count = ((strtab_vaddr - symtab_vaddr) / d_syment) as usize;

    let read_symbol_name = |data: &[u8], name_off: u32| -> Result<String> {
        let start = strtab_off + name_off as usize;
        let mut end = start;
        while data.get(end).copied().unwrap_or(0) != 0 {
            end += 1;
        }
        let bytes = data
            .get(start..end)
            .ok_or_else(|| WieError::FatalError(format!("firmware string at {start:#x} out of bounds")))?;
        Ok(bytes.iter().map(|&byte| char::from(byte)).collect())
    };

    // Resolves relocation symbol `index` to a guest address. Defined symbols
    // land at base + st_value; imports are handed to the resolver.
    let mut resolve_symbol = |core: &mut ArmCore, index: usize, unresolved: &mut Vec<UnresolvedImport>, place: u32| -> Result<Option<u32>> {
        let sym_off = symtab_off + index * d_syment as usize;
        let st_name = rd_u32(data, sym_off)?;
        let st_value = rd_u32(data, sym_off + 4)?;
        let st_shndx = rd_u16(data, sym_off + 14)?;

        if st_shndx != SHN_UNDEF {
            return Ok(Some(base.wrapping_add(st_value)));
        }

        let name = read_symbol_name(data, st_name)?;
        match resolver.resolve(core, &name)? {
            Some(addr) => Ok(Some(addr)),
            None => {
                unresolved.push(UnresolvedImport { name, place });
                Ok(None)
            }
        }
    };

    let mut unresolved_imports = Vec::new();

    // Apply the two relocation tables. Both are REL (8-byte) on ARM: DT_REL is
    // the bulk R_ARM_RELATIVE table, DT_JMPREL the PLT import bindings.
    for (table_vaddr, table_size) in [(d_rel, d_relsz), (d_jmprel, d_pltrelsz)] {
        let Some(table_vaddr) = table_vaddr else { continue };
        if table_size == 0 {
            continue;
        }
        let table_off = vaddr_to_offset(&loads, table_vaddr)?;
        let count = table_size as usize / 8;

        for index in 0..count {
            let entry = table_off + index * 8;
            let r_offset = rd_u32(data, entry)?;
            let r_info = rd_u32(data, entry + 4)?;
            let r_type = r_info & 0xff;
            let sym_index = (r_info >> 8) as usize;
            let place = base.wrapping_add(r_offset);

            match r_type {
                R_ARM_NONE => {}
                R_ARM_RELATIVE => {
                    // REL: the in-place word is the link-time address (addend).
                    let addend = rd_u32(data, vaddr_to_offset(&loads, r_offset)?)?;
                    write_generic(core, place, arm_relative(base, addend))?;
                }
                R_ARM_GLOB_DAT | R_ARM_JUMP_SLOT => {
                    if let Some(symbol) = resolve_symbol(core, sym_index, &mut unresolved_imports, place)? {
                        write_generic(core, place, symbol)?;
                    }
                }
                R_ARM_ABS32 => {
                    if let Some(symbol) = resolve_symbol(core, sym_index, &mut unresolved_imports, place)? {
                        let addend = rd_u32(data, vaddr_to_offset(&loads, r_offset)?)?;
                        write_generic(core, place, arm_abs32(addend, symbol))?;
                    }
                }
                other => {
                    tracing::warn!("Unsupported firmware relocation type {other} at {place:#x}; leaving slot unchanged");
                }
            }
        }
    }

    // Read back the firmware's own export table, name -> guest address.
    let mut exports = BTreeMap::new();
    for index in 0..symbol_count {
        let sym_off = symtab_off + index * d_syment as usize;
        let st_name = rd_u32(data, sym_off)?;
        let st_value = rd_u32(data, sym_off + 4)?;
        let st_info = data.get(sym_off + 12).copied().unwrap_or(0);
        let st_shndx = rd_u16(data, sym_off + 14)?;

        let st_type = st_info & 0xf;
        if st_shndx == SHN_UNDEF || st_name == 0 || (st_type != STT_FUNC && st_type != STT_OBJECT) {
            continue;
        }

        let name = read_symbol_name(data, st_name)?;
        if !name.is_empty() {
            exports.insert(name, base.wrapping_add(st_value));
        }
    }

    let entry = base.wrapping_add(e_entry);
    tracing::debug!(
        "Loaded firmware at base {base:#x}: entry {entry:#x}, {} segment(s), {} export(s), {} unresolved import(s)",
        segments.len(),
        exports.len(),
        unresolved_imports.len()
    );

    Ok(FirmwareImage {
        base,
        entry,
        segments,
        exports,
        unresolved_imports,
    })
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use wie_core_arm::{Allocator, ArmCore};
    use wie_util::{Result, read_generic};

    use super::{ImportResolver, load_firmware};

    fn core() -> ArmCore {
        let mut core = ArmCore::new(false, None).unwrap();
        Allocator::init(&mut core).unwrap();
        core
    }

    /// Builds a minimal but valid `ET_DYN` ARM ELF, driven entirely by
    /// `PT_LOAD` + `PT_DYNAMIC` (no section headers), exercising the same path
    /// the real firmware takes. One `R_ARM_RELATIVE`, one imported
    /// `R_ARM_JUMP_SLOT`, and one exported function.
    fn build_test_dylib() -> Vec<u8> {
        // Layout inside a single PT_LOAD, vaddr == file offset:
        //   0x0000 ELF header (52) + program headers (2 * 32)
        //   0x0100 .dynsym  (3 entries * 16)
        //   0x0130 .dynstr
        //   0x0160 .rel.dyn (1 entry, R_ARM_RELATIVE)
        //   0x0168 .rel.plt (1 entry, R_ARM_JUMP_SLOT)
        //   0x0170 .dynamic
        //   0x0200 data: relative slot (0x0200) + plt slot (0x0204) + export body (0x0208)
        let mut image = vec![0u8; 0x300];

        let put_u16 = |img: &mut [u8], off: usize, v: u16| img[off..off + 2].copy_from_slice(&v.to_le_bytes());
        let put_u32 = |img: &mut [u8], off: usize, v: u32| img[off..off + 4].copy_from_slice(&v.to_le_bytes());

        // ELF header.
        image[0..4].copy_from_slice(b"\x7fELF");
        image[4] = 1; // ELFCLASS32
        image[5] = 1; // ELFDATA2LSB
        image[6] = 1; // EV_CURRENT
        put_u16(&mut image, 16, 3); // e_type = ET_DYN
        put_u16(&mut image, 18, 40); // e_machine = EM_ARM
        put_u32(&mut image, 24, 0x0208); // e_entry (export body)
        put_u32(&mut image, 28, 52); // e_phoff
        put_u16(&mut image, 42, 32); // e_phentsize
        put_u16(&mut image, 44, 2); // e_phnum

        // Program header 0: PT_LOAD covering the whole image.
        let ph0 = 52;
        put_u32(&mut image, ph0, 1); // PT_LOAD
        put_u32(&mut image, ph0 + 4, 0); // p_offset
        put_u32(&mut image, ph0 + 8, 0); // p_vaddr
        put_u32(&mut image, ph0 + 16, 0x300); // p_filesz
        put_u32(&mut image, ph0 + 20, 0x300); // p_memsz

        // Program header 1: PT_DYNAMIC.
        let ph1 = 52 + 32;
        put_u32(&mut image, ph1, 2); // PT_DYNAMIC
        put_u32(&mut image, ph1 + 4, 0x0170); // p_offset
        put_u32(&mut image, ph1 + 8, 0x0170); // p_vaddr
        put_u32(&mut image, ph1 + 16, 0x90); // p_filesz
        put_u32(&mut image, ph1 + 20, 0x90); // p_memsz

        // .dynsym: [0]=undef, [1]=imported "host_fn", [2]=exported "fw_export".
        let symtab = 0x0100;
        // sym 1: st_name -> "host_fn", st_shndx = UNDEF (import).
        put_u32(&mut image, symtab + 16, 1); // st_name
        put_u32(&mut image, symtab + 16 + 4, 0); // st_value
        image[symtab + 16 + 12] = 0x12; // st_info: bind GLOBAL, type FUNC
        put_u16(&mut image, symtab + 16 + 14, 0); // st_shndx = UNDEF
        // sym 2: st_name -> "fw_export", defined at 0x0208.
        put_u32(&mut image, symtab + 32, 9); // st_name
        put_u32(&mut image, symtab + 32 + 4, 0x0208); // st_value
        image[symtab + 32 + 12] = 0x12; // FUNC
        put_u16(&mut image, symtab + 32 + 14, 1); // st_shndx = defined

        // .dynstr: "\0host_fn\0fw_export\0"
        let dynstr = 0x0130;
        let s = b"\x00host_fn\x00fw_export\x00";
        image[dynstr..dynstr + s.len()].copy_from_slice(s);

        // .rel.dyn: one R_ARM_RELATIVE at 0x0200.
        let rel = 0x0160;
        put_u32(&mut image, rel, 0x0200); // r_offset
        put_u32(&mut image, rel + 4, 23); // r_info: type R_ARM_RELATIVE, sym 0

        // .rel.plt: one R_ARM_JUMP_SLOT at 0x0204 for sym 1 (import).
        let relplt = 0x0168;
        put_u32(&mut image, relplt, 0x0204); // r_offset
        put_u32(&mut image, relplt + 4, (1 << 8) | 22); // sym 1, type R_ARM_JUMP_SLOT

        // .dynamic array.
        let dynamic = 0x0170;
        let dyn_entries: [(u32, u32); 9] = [
            (6, symtab as u32),   // DT_SYMTAB
            (11, 16),             // DT_SYMENT
            (5, dynstr as u32),   // DT_STRTAB
            (17, rel as u32),     // DT_REL
            (18, 8),              // DT_RELSZ
            (23, relplt as u32),  // DT_JMPREL
            (2, 8),               // DT_PLTRELSZ
            (10, s.len() as u32), // DT_STRSZ (ignored by loader)
            (0, 0),               // DT_NULL
        ];
        for (index, (tag, val)) in dyn_entries.iter().enumerate() {
            put_u32(&mut image, dynamic + index * 8, *tag);
            put_u32(&mut image, dynamic + index * 8 + 4, *val);
        }

        // Relative slot: link-time value 0x0208, to be rebased by the load bias.
        put_u32(&mut image, 0x0200, 0x0208);
        // PLT slot starts zero; the import binding fills it.
        put_u32(&mut image, 0x0204, 0);

        image
    }

    #[test]
    fn maps_relocates_binds_and_exports() {
        let mut core = core();
        let image = build_test_dylib();
        let base = 0x0040_0000;

        // Resolver binds the one import to a fixed stub address.
        let stub = 0x00ab_cdefu32;
        let mut resolver = |_core: &mut ArmCore, name: &str| -> Result<Option<u32>> {
            assert_eq!(name, "host_fn");
            Ok(Some(stub))
        };

        let loaded = load_firmware(&mut core, &image, base, &mut resolver as &mut dyn ImportResolver).unwrap();

        // Entry is rebased.
        assert_eq!(loaded.entry, base + 0x0208);

        // R_ARM_RELATIVE rebased the slot: 0x0208 + base.
        let relative: u32 = read_generic(&core, base + 0x0200).unwrap();
        assert_eq!(relative, base + 0x0208);

        // R_ARM_JUMP_SLOT bound the import to the stub.
        let plt: u32 = read_generic(&core, base + 0x0204).unwrap();
        assert_eq!(plt, stub);

        // The export table was read back and rebased.
        assert_eq!(loaded.export("fw_export"), Some(base + 0x0208));
        // The undefined import is not an export.
        assert_eq!(loaded.export("host_fn"), None);

        assert!(loaded.unresolved_imports.is_empty());
    }

    #[test]
    fn records_unresolved_imports_without_failing() {
        let mut core = core();
        let image = build_test_dylib();
        let base = 0x0050_0000;

        // Resolver that handles nothing.
        let mut resolver = |_core: &mut ArmCore, _name: &str| -> Result<Option<u32>> { Ok(None) };

        let loaded = load_firmware(&mut core, &image, base, &mut resolver as &mut dyn ImportResolver).unwrap();

        assert_eq!(loaded.unresolved_imports.len(), 1);
        assert_eq!(loaded.unresolved_imports[0].name, "host_fn");
        assert_eq!(loaded.unresolved_imports[0].place, base + 0x0204);

        // The unbound PLT slot is left untouched (still zero).
        let plt: u32 = read_generic(&core, base + 0x0204).unwrap();
        assert_eq!(plt, 0);

        // A relative reloc with no symbol still applies.
        let relative: u32 = read_generic(&core, base + 0x0200).unwrap();
        assert_eq!(relative, base + 0x0208);
    }

    #[test]
    fn rejects_non_arm_et_dyn() {
        let mut core = core();
        let mut image = build_test_dylib();
        // Flip e_machine to something other than ARM.
        image[18..20].copy_from_slice(&99u16.to_le_bytes());

        let mut resolver = |_core: &mut ArmCore, _name: &str| -> Result<Option<u32>> { Ok(None) };
        let result = load_firmware(&mut core, &image, 0x0040_0000, &mut resolver as &mut dyn ImportResolver);
        assert!(result.is_err());
    }
}
