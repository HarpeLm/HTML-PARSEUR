//! # lumen-css
//!
//! Parser CSS écrit en Rust, suivant la spec
//! [CSS Syntax Level 3](https://www.w3.org/TR/css-syntax-3/) et testé contre
//! [css-parsing-tests](https://github.com/SimonSapin/css-parsing-tests).
//!
//! C'est une brique de Lumen, un navigateur web écrit de zéro.
//!
//! ```
//! use lumen_css::{preprocess, ComponentValue, Parser};
//!
//! let css = preprocess("rgb(255, 0, 0)");
//! let values = Parser::new(&css).parse_component_value_list();
//! assert!(matches!(&values[0], ComponentValue::Function { name, .. } if name == "rgb"));
//! ```

#![warn(missing_docs)]

mod parser;
mod rules;
mod tokenizer;

pub use parser::{BlockKind, ComponentValue, ParseError, Parser};
pub use rules::{AtRule, Declaration, Item, QualifiedRule};
pub use tokenizer::{Numeric, Token, TokenError, Tokenizer, preprocess};
