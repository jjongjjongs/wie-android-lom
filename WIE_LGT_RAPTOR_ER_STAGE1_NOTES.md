# WIE LGT Raptor ER relocation — Stage 1

## Added

- Private relocation numbers recovered from the reference `libraptor_er.so`:
  - `R_ARM_RREL32 = 252`
  - `R_ARM_RABS32 = 253`
  - `R_ARM_RPC24 = 254`
  - `R_ARM_RBASE = 255`
- Raptor ER relocation helpers and unit tests.
- `R_ARM_RBASE` segment-context registration in the real LGT ELF relocation path.
- Safe handling for Raptor ER relocation sections without a conventional ELF symbol table.
- Diagnostics for unknown Raptor segments, invalid relocation places, non-branch RPC24 instructions, alignment, and range failures.

## Important scope

WIE currently maps allocatable ELF sections at their linked virtual addresses, so their load bias is zero. This stage implements the correct Raptor ER relocation model and record parsing, while producing no unnecessary address changes for fixed-address images. It is groundwork for future rebased loading and prevents private relocation records from being mistaken for ordinary ELF symbols.

## Reference evidence

The private relocation values and formulas were recovered from the uploaded WipiPlayer reference APK's unstripped `libraptor_er.so`. The loader dispatches types `252`, `253`, `254`, and `255`, corresponding respectively to relative, absolute, ARM PC24, and segment-base records.

## Validation required in Termux

```bash
cargo fmt --all
cargo check -p wie_core_arm -p wie_lgt -p wie_wipi_c
cargo test -p wie_core_arm -p wie_lgt -p wie_wipi_c
cargo check --workspace --exclude wie_cli
```
