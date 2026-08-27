//! A faithful port of the reference LGT player's MA-3 renderer,
//! `com.keitaiwiki.music` (`OracleSmaf` + `OracleMmfAnalysis` + `OracleMa3Synth`).
//!
//! The existing [`crate::ma3`] engine renders the same files, but an offline
//! A/B against the reference showed its FM output only weakly correlates with
//! the reference's for the same notes - the tables and recordings match, yet
//! the rendering logic diverges. Rather than chase each difference, this module
//! reproduces the reference renderer directly, so every title sounds the way
//! the reference plays it. It is built and verified bottom-up against the
//! reference running as an oracle; until it is wired in, [`crate::ma3`] stays
//! the live path.
//!
//! - [`tables`] is the reference's own data.
//!
//! The port is built up over several steps; while it is incomplete the unused
//! tables and helpers are expected, so dead code is allowed here rather than
//! littering each item with an attribute.
#![allow(dead_code, unused_imports)]

pub mod analysis;
pub mod audio;
pub mod rhythm;
pub mod smaf;
pub mod synth;
pub mod tables;
pub mod tone;
