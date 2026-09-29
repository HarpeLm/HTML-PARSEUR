pub mod atoms;
mod char_ref;
pub mod dom;
mod foreign;
mod entities;
pub mod scan;
mod token;
mod tokenizer;
mod tree_builder;

pub use token::{Attribute, Doctype, Tag, Token};
pub use tokenizer::{InitialState, Tokenizer};
pub use tree_builder::{
    parse_document, parse_document_owned, parse_document_owned_with, parse_document_with, parse_fragment,
    ParseOptions,
};
