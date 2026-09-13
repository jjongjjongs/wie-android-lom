# WIE LGT compatibility completion patch

This revision focuses on the common runtime gaps identified by comparison with the legacy LGT player.

## Implemented

- Added ARM ELF32 relocation helpers and connected them to the actual `binary.mod` loading path.
- Handles `R_ARM_ABS32`, `R_ARM_REL32`, `R_ARM_PC24`, `R_ARM_CALL`, `R_ARM_JUMP24`, `R_ARM_THM_CALL`, and `R_ARM_THM_JUMP24`.
- Added REL and RELA parsing, symbol-table lookup, range/alignment validation, and diagnostics for unsupported or unresolved relocations.
- Added explicit zero initialization for `SHT_NOBITS` sections.
- Replaced fatal termination for unknown LGT import-table entries with callable diagnostic SVC stubs. This prevents the resolver from returning address zero while preserving detailed table/function/register logs.

## Deliberately not guessed

- Unknown imports still return zero when invoked. Their real semantics must be implemented from game logs/reference behavior.
- Unsupported relocation types are logged and left unchanged rather than applying an unsafe guess.
- Missing game classes/resources/DLC are not fabricated.

## Validation commands

```sh
cargo fmt --all
cargo check -p wie_core_arm -p wie_lgt -p wie_wipi_c
cargo test -p wie_core_arm -p wie_lgt -p wie_wipi_c
cargo check --workspace --exclude wie_android --exclude wie_cli
```
