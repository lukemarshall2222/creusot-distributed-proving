//! Format_Codec (design C16, R7.10): one module per exchanged format, each
//! with a strict parser and a canonical printer.

pub mod hash;
pub mod sexp;
pub mod dispatch;
pub mod lockfile;
