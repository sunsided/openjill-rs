//! Editor / data-tool subcommand implementations behind the `editors` feature.
//!
//! Each module backs one `openjill <format> <action>` subcommand, porting the
//! former standalone `tools/openjill-*` binaries onto `openjill-data` parsers
//! and `openjill-export` converters.

pub mod cfg;
pub mod sha;
pub mod vcl;
