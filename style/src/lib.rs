//! # lumen-style
//!
//! La cascade CSS : le style calculé de chaque élément d'un document HTML.
//! Elle assemble les briques de Lumen : le DOM de `html-parseur`, et de
//! `lumen-css` les sélecteurs, `@media`, `var()` et les propriétés.
//!
//! 1. **Collecter les règles** : la feuille par défaut du navigateur
//!    ([`USER_AGENT_CSS`]), les `<style>` du document (avec leurs `@media` et
//!    `@supports`), l'attribut `style=""`.
//! 2. **Trier les déclarations** : origine et `!important`, attribut `style`,
//!    spécificité, ordre.
//! 3. **Calculer les valeurs** : héritage, `initial` / `inherit` / `unset`,
//!    `var()`, unités relatives converties en px.
//!
//! Vérifié contre `getComputedStyle()` de Chromium (tests/oracle_cascade.rs).
//!
//! ```
//! use html_parseur::parse_document;
//! use html_parseur::dom::NodeId;
//! use lumen_css::media::Environment;
//! use lumen_style::style_document;
//!
//! let doc = parse_document("<style>p { font-size: 2em } .x { font-weight: bold }</style>\
//!                           <p class=x>Bonjour");
//! let styles = style_document(&doc, &Environment::default());
//! let p = doc.descendants(NodeId::DOCUMENT)
//!     .find(|&n| doc.element(n).is_some_and(|e| doc.atoms.name(e.name) == "p"))
//!     .unwrap();
//! let style = styles.get(p).unwrap();
//! assert_eq!(style.resolved("display").as_deref(), Some("block"));
//! assert_eq!(style.resolved("font-size").as_deref(), Some("32px"));
//! assert_eq!(style.resolved("font-weight").as_deref(), Some("700"));
//! ```

#![warn(missing_docs)]

mod bloom;
pub mod cascade;
pub mod computed;
pub mod sheet;

pub use cascade::{StyleEngine, Styles, USER_AGENT_CSS, flat_children, style_document};
pub use computed::{Computed, ComputedStyle};
pub use sheet::Stylesheet;
