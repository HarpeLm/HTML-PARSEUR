//! # html-parseur
//!
//! Tokenizer et parser HTML écrits en Rust, conformes à la spec
//! [WHATWG](https://html.spec.whatwg.org/multipage/parsing.html) : 100 % des tests
//! html5lib (tokenizer) et WPT (construction de l'arbre). Aucune dépendance.
//!
//! C'est la première brique d'un navigateur web écrit de zéro.
//!
//! ## Parser une page
//!
//! ```
//! use html_parseur::{parse_document, dom::NodeId};
//!
//! let doc = parse_document("<p>Bonjour <b>le monde</b>");
//! // Le parser corrige le HTML comme un navigateur : <html>, <head> et <body>
//! // sont ajoutés, les balises non fermées sont fermées.
//! assert_eq!(doc.to_test_string(), "\
//! | <html>
//! |   <head>
//! |   <body>
//! |     <p>
//! |       \"Bonjour \"
//! |       <b>
//! |         \"le monde\"");
//!
//! // Parcourir l'arbre : tous les textes de la page.
//! let texts: Vec<&str> = doc.descendants(NodeId::DOCUMENT).filter_map(|n| doc.text(n)).collect();
//! assert_eq!(texts, ["Bonjour ", "le monde"]);
//! ```
//!
//! ## Tokenizer seul
//!
//! ```
//! use html_parseur::{Token, Tokenizer};
//!
//! let tokens: Vec<Token> = Tokenizer::new("<a href=x>lien</a>").collect();
//! assert!(matches!(&tokens[0], Token::StartTag(tag) if tag.name == "a"));
//! ```
//!
//! ## Fragments (`innerHTML`)
//!
//! ```
//! use html_parseur::{parse_fragment, ParseOptions, dom::Namespace};
//!
//! // Le même "<td>" ne donne pas le même arbre selon le contexte.
//! let (doc, root) = parse_fragment("<td>A", Namespace::Html, "tr", ParseOptions::default());
//! assert_eq!(doc.to_test_string_from(root), "| <td>\n|   \"A\"");
//! ```

#![warn(missing_docs)]
pub mod atoms;
mod char_ref;
pub mod dom;
mod entities;
mod foreign;
pub mod scan;
mod token;
mod tokenizer;
mod tree_builder;

pub use token::{Attribute, Doctype, Tag, Token};
pub use tokenizer::{InitialState, Tokenizer};
pub use tree_builder::{
    ParseOptions, parse_document, parse_document_owned, parse_document_owned_with,
    parse_document_with, parse_fragment,
};
