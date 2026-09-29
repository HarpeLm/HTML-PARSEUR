mod char_ref;
mod entities;
pub mod scan;
mod token;
mod tokenizer;

pub use token::{Attribute, Doctype, Tag, Token};
pub use tokenizer::{InitialState, Tokenizer};
