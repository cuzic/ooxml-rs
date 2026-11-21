//! Formula text: lexing, parsing, translation and precedent analysis
//! (ADR-0003 §2, §4.2). Nothing here evaluates a formula.

pub(crate) mod functions;
pub(crate) mod future;
pub(crate) mod lexer;
pub(crate) mod parse;
pub(crate) mod precedents;
#[expect(dead_code, reason = "the row and column edit entry points arrive in ADR-0018 P7a")]
pub(crate) mod structural;
pub(crate) mod structured;
pub(crate) mod translate;
