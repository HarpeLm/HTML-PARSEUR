//! Sélecteurs CSS (spec Selectors Level 4) : parsing, spécificité et
//! correspondance avec les éléments d'un DOM.
//!
//! La correspondance passe par le trait [`Element`] : n'importe quel DOM peut
//! l'implémenter, la brique CSS ne dépend d'aucun DOM en particulier.

use crate::an_plus_b::parse_an_plus_b;
use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;

/// Ce qu'un DOM doit fournir pour que les sélecteurs s'y appliquent.
pub trait Element: Copy {
    /// Le nom de la balise (`div`), en minuscules pour le HTML.
    fn local_name(&self) -> &str;
    /// Vrai pour un élément HTML dans un document HTML (noms insensibles à la casse).
    fn is_html(&self) -> bool;
    /// La valeur d'un attribut.
    fn attribute(&self, name: &str) -> Option<&str>;
    /// L'élément parent (pas le document).
    fn parent_element(&self) -> Option<Self>;
    /// L'élément frère précédent (textes et commentaires ignorés).
    fn prev_sibling_element(&self) -> Option<Self>;
    /// L'élément frère suivant.
    fn next_sibling_element(&self) -> Option<Self>;
    /// Aucun enfant, hormis des commentaires (`:empty`).
    fn is_empty(&self) -> bool;
    /// Vrai si les deux désignent le même élément.
    fn same_as(&self, other: &Self) -> bool;
    /// L'élément racine du document (`<html>`).
    fn is_root(&self) -> bool {
        self.parent_element().is_none()
    }
    /// L'attribut `id`.
    fn id(&self) -> Option<&str> {
        self.attribute("id")
    }
    /// L'attribut `class` contient-il `name` ?
    fn has_class(&self, name: &str) -> bool {
        self.attribute("class")
            .is_some_and(|c| c.split_ascii_whitespace().any(|x| x == name))
    }
}

/// Une liste de sélecteurs séparés par des virgules : `h1, h2.titre`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectorList(pub Vec<Selector>);

/// Un sélecteur complexe : `div > p.titre`.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// Le sujet (la partie la plus à droite).
    pub subject: Compound,
    /// Les parties précédentes, de droite à gauche, avec le combinateur qui
    /// les relie à la partie à leur droite.
    pub ancestors: Vec<(Combinator, Compound)>,
}

/// Comment deux parties d'un sélecteur sont reliées.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    /// ` ` (espace) : descendant.
    Descendant,
    /// `>` : enfant direct.
    Child,
    /// `+` : frère immédiatement précédent.
    NextSibling,
    /// `~` : un frère précédent.
    SubsequentSibling,
}

/// Un sélecteur composé, sans combinateur : `p.titre[lang]:first-child`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compound {
    /// Les sélecteurs simples.
    pub simple: Vec<Simple>,
    /// Un pseudo-élément final (`::before`).
    pub pseudo_element: Option<String>,
}

/// Opérateur d'un sélecteur d'attribut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `[a=v]`
    Equals,
    /// `[a~=v]` : un des mots.
    Includes,
    /// `[a|=v]` : `v` ou `v-...`.
    DashMatch,
    /// `[a^=v]`
    Prefix,
    /// `[a$=v]`
    Suffix,
    /// `[a*=v]`
    Substring,
}

/// Un sélecteur simple.
#[derive(Debug, Clone, PartialEq)]
pub enum Simple {
    /// `*`
    Universal,
    /// `div`
    Type(String),
    /// `#id`
    Id(String),
    /// `.classe`
    Class(String),
    /// `[attr]`, `[attr=valeur i]`...
    Attribute {
        /// Le nom de l'attribut.
        name: String,
        /// L'opérateur et la valeur (`None` : présence seule).
        matcher: Option<(AttrOp, String)>,
        /// Drapeau `i` (insensible) ou `s` (sensible), s'il est donné.
        case_insensitive: Option<bool>,
    },
    /// `:first-child`, `:not(...)`...
    PseudoClass(PseudoClass),
}

/// Les pseudo-classes prises en charge.
#[derive(Debug, Clone, PartialEq)]
pub enum PseudoClass {
    /// `:root`
    Root,
    /// `:empty`
    Empty,
    /// `:first-child`
    FirstChild,
    /// `:last-child`
    LastChild,
    /// `:only-child`
    OnlyChild,
    /// `:first-of-type`
    FirstOfType,
    /// `:last-of-type`
    LastOfType,
    /// `:only-of-type`
    OnlyOfType,
    /// `:nth-child(An+B [of S])`, `:nth-last-child(...)`, `:nth-of-type(...)`...
    Nth {
        /// A et B.
        a: i32,
        /// B.
        b: i32,
        /// Compter depuis la fin.
        from_end: bool,
        /// Compter seulement les frères du même type.
        of_type: bool,
        /// `of S` : compter seulement les frères qui correspondent à S.
        of_selector: Option<Box<SelectorList>>,
    },
    /// `:not(...)`
    Not(Box<SelectorList>),
    /// `:is(...)`
    Is(Box<SelectorList>),
    /// `:where(...)` (spécificité nulle).
    Where(Box<SelectorList>),
    /// `:hover`, `:focus`, `:visited`... : états dynamiques, jamais vrais pour
    /// un document statique.
    Never(String),
}

/// La spécificité (§17) : (ids, classes/attributs/pseudo-classes, types).
pub type Specificity = (u32, u32, u32);

// ───────────── Parsing ─────────────

fn is_ws(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

fn trim<'v, 'a>(values: &'v [ComponentValue<'a>]) -> &'v [ComponentValue<'a>] {
    let start = values
        .iter()
        .position(|v| !is_ws(v))
        .unwrap_or(values.len());
    let end = values
        .iter()
        .rposition(|v| !is_ws(v))
        .map_or(start, |i| i + 1);
    &values[start..end]
}

/// Parse une liste de sélecteurs (le prélude d'une règle de style).
/// `None` si un des sélecteurs est invalide : la règle entière est alors ignorée.
pub fn parse_selector_list(values: &[ComponentValue]) -> Option<SelectorList> {
    let mut list = Vec::new();
    for part in values.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        list.push(parse_selector(trim(part))?);
    }
    Some(SelectorList(list))
}

/// Liste "tolérante" de `:is()` et `:where()` : les sélecteurs invalides sont
/// simplement ignorés.
fn parse_forgiving_list(values: &[ComponentValue]) -> SelectorList {
    SelectorList(
        values
            .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
            .filter_map(|part| parse_selector(trim(part)))
            .collect(),
    )
}

fn parse_selector(values: &[ComponentValue]) -> Option<Selector> {
    let mut pos = 0;
    let mut compounds = vec![parse_compound(values, &mut pos)?];
    let mut combinators = Vec::new();
    loop {
        let had_space = values.get(pos).is_some_and(is_ws);
        while values.get(pos).is_some_and(is_ws) {
            pos += 1;
        }
        if pos >= values.len() {
            break;
        }
        let combinator = match &values[pos] {
            ComponentValue::Token(Token::Delim('>')) => Some(Combinator::Child),
            ComponentValue::Token(Token::Delim('+')) => Some(Combinator::NextSibling),
            ComponentValue::Token(Token::Delim('~')) => Some(Combinator::SubsequentSibling),
            _ if had_space => None,
            _ => return None,
        };
        let combinator = match combinator {
            Some(c) => {
                pos += 1;
                while values.get(pos).is_some_and(is_ws) {
                    pos += 1;
                }
                c
            }
            None => Combinator::Descendant,
        };
        // Un pseudo-élément ne peut être que dans la dernière partie.
        if compounds
            .last()
            .is_some_and(|c: &Compound| c.pseudo_element.is_some())
        {
            return None;
        }
        combinators.push(combinator);
        compounds.push(parse_compound(values, &mut pos)?);
    }
    let subject = compounds.pop()?;
    let ancestors = combinators
        .into_iter()
        .rev()
        .zip(compounds.into_iter().rev())
        .collect();
    Some(Selector { subject, ancestors })
}

fn parse_compound(values: &[ComponentValue], pos: &mut usize) -> Option<Compound> {
    let mut compound = Compound::default();
    match values.get(*pos) {
        Some(ComponentValue::Token(Token::Ident(name))) => {
            compound.simple.push(Simple::Type(name.to_string()));
            *pos += 1;
        }
        Some(ComponentValue::Token(Token::Delim('*'))) => {
            compound.simple.push(Simple::Universal);
            *pos += 1;
        }
        _ => {}
    }
    // Espaces de noms (`svg|rect`) : pas pris en charge.
    if matches!(
        values.get(*pos),
        Some(ComponentValue::Token(Token::Delim('|')))
    ) {
        return None;
    }
    loop {
        if compound.pseudo_element.is_some() {
            // Rien d'autre (dans cette version) après un pseudo-élément.
            break;
        }
        match values.get(*pos) {
            Some(ComponentValue::Token(Token::Hash { value, is_id: true })) => {
                compound.simple.push(Simple::Id(value.to_string()));
                *pos += 1;
            }
            Some(ComponentValue::Token(Token::Delim('.'))) => {
                let Some(ComponentValue::Token(Token::Ident(name))) = values.get(*pos + 1) else {
                    return None;
                };
                compound.simple.push(Simple::Class(name.to_string()));
                *pos += 2;
            }
            Some(ComponentValue::Block {
                kind: BlockKind::Square,
                contents,
            }) => {
                compound.simple.push(parse_attribute(contents)?);
                *pos += 1;
            }
            Some(ComponentValue::Token(Token::Colon)) => {
                *pos += 1;
                parse_pseudo(values, pos, &mut compound)?;
            }
            _ => break,
        }
    }
    if compound.simple.is_empty() && compound.pseudo_element.is_none() {
        return None;
    }
    Some(compound)
}

fn parse_attribute(contents: &[ComponentValue]) -> Option<Simple> {
    let items: Vec<&ComponentValue> = contents.iter().filter(|v| !is_ws(v)).collect();
    let Some(ComponentValue::Token(Token::Ident(name))) = items.first() else {
        return None;
    };
    let name = name.to_string();
    if items.len() == 1 {
        return Some(Simple::Attribute {
            name,
            matcher: None,
            case_insensitive: None,
        });
    }
    let op = match items[1] {
        ComponentValue::Token(Token::Delim('=')) => AttrOp::Equals,
        ComponentValue::Token(Token::IncludeMatch) => AttrOp::Includes,
        ComponentValue::Token(Token::DashMatch) => AttrOp::DashMatch,
        ComponentValue::Token(Token::PrefixMatch) => AttrOp::Prefix,
        ComponentValue::Token(Token::SuffixMatch) => AttrOp::Suffix,
        ComponentValue::Token(Token::SubstringMatch) => AttrOp::Substring,
        _ => return None,
    };
    let value = match items.get(2)? {
        ComponentValue::Token(Token::Ident(v) | Token::String(v)) => v.to_string(),
        _ => return None,
    };
    let case_insensitive = match items.get(3) {
        None => None,
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("i") => {
            Some(true)
        }
        Some(ComponentValue::Token(Token::Ident(flag))) if flag.eq_ignore_ascii_case("s") => {
            Some(false)
        }
        _ => return None,
    };
    if items.len() > 4 {
        return None;
    }
    Some(Simple::Attribute {
        name,
        matcher: Some((op, value)),
        case_insensitive,
    })
}

/// Après un `:` : pseudo-classe, ou pseudo-élément (`::x`, ou `:before` historique).
fn parse_pseudo(values: &[ComponentValue], pos: &mut usize, compound: &mut Compound) -> Option<()> {
    let element = matches!(values.get(*pos), Some(ComponentValue::Token(Token::Colon)));
    if element {
        *pos += 1;
    }
    match values.get(*pos)? {
        ComponentValue::Token(Token::Ident(name)) => {
            *pos += 1;
            let lower = name.to_ascii_lowercase();
            let legacy_element = matches!(
                lower.as_str(),
                "before" | "after" | "first-line" | "first-letter"
            );
            if element || legacy_element {
                compound.pseudo_element = Some(lower);
                return Some(());
            }
            let pc = match lower.as_str() {
                "root" => PseudoClass::Root,
                "empty" => PseudoClass::Empty,
                "first-child" => PseudoClass::FirstChild,
                "last-child" => PseudoClass::LastChild,
                "only-child" => PseudoClass::OnlyChild,
                "first-of-type" => PseudoClass::FirstOfType,
                "last-of-type" => PseudoClass::LastOfType,
                "only-of-type" => PseudoClass::OnlyOfType,
                "hover" | "active" | "focus" | "focus-within" | "focus-visible" | "visited"
                | "target" => PseudoClass::Never(lower),
                _ => return None,
            };
            compound.simple.push(Simple::PseudoClass(pc));
        }
        ComponentValue::Function { name, arguments } if !element => {
            *pos += 1;
            let lower = name.to_ascii_lowercase();
            let pc = match lower.as_str() {
                "not" => PseudoClass::Not(Box::new(parse_selector_list(trim(arguments))?)),
                "is" => PseudoClass::Is(Box::new(parse_forgiving_list(arguments))),
                "where" => PseudoClass::Where(Box::new(parse_forgiving_list(arguments))),
                "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                    let of_type = lower.ends_with("of-type");
                    // `An+B of S` : seulement pour nth-child et nth-last-child.
                    let of = arguments.iter().position(
                        |v| matches!(v, ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case("of")),
                    );
                    let (anb, of_selector) = match of {
                        Some(i) if !of_type => {
                            let list = parse_selector_list(trim(&arguments[i + 1..]))?;
                            (&arguments[..i], Some(Box::new(list)))
                        }
                        Some(_) => return None,
                        None => (&arguments[..], None),
                    };
                    let (a, b) = parse_an_plus_b(anb)?;
                    PseudoClass::Nth {
                        a,
                        b,
                        from_end: lower.starts_with("nth-last"),
                        of_type,
                        of_selector,
                    }
                }
                _ => return None,
            };
            compound.simple.push(Simple::PseudoClass(pc));
        }
        _ => return None,
    }
    Some(())
}

// ───────────── Spécificité ─────────────

impl SelectorList {
    /// La plus grande spécificité de la liste (utile pour `:is()` et `:not()`).
    fn max_specificity(&self) -> Specificity {
        self.0
            .iter()
            .map(Selector::specificity)
            .max()
            .unwrap_or((0, 0, 0))
    }
}

impl Selector {
    /// La spécificité du sélecteur (Selectors 4, §17).
    pub fn specificity(&self) -> Specificity {
        let mut total = (0, 0, 0);
        for compound in std::iter::once(&self.subject).chain(self.ancestors.iter().map(|(_, c)| c))
        {
            for simple in &compound.simple {
                let s = match simple {
                    Simple::Universal => (0, 0, 0),
                    Simple::Type(_) => (0, 0, 1),
                    Simple::Id(_) => (1, 0, 0),
                    Simple::Class(_) | Simple::Attribute { .. } => (0, 1, 0),
                    Simple::PseudoClass(pc) => match pc {
                        PseudoClass::Not(list) | PseudoClass::Is(list) => list.max_specificity(),
                        PseudoClass::Where(_) => (0, 0, 0),
                        PseudoClass::Nth {
                            of_selector: Some(list),
                            ..
                        } => {
                            let (a, b, c) = list.max_specificity();
                            (a, b + 1, c)
                        }
                        _ => (0, 1, 0),
                    },
                };
                total = (total.0 + s.0, total.1 + s.1, total.2 + s.2);
            }
            if compound.pseudo_element.is_some() {
                total.2 += 1;
            }
        }
        total
    }
}

// ───────────── Correspondance ─────────────

/// Attributs HTML dont la valeur se compare sans tenir compte de la casse
/// (spec HTML, "case-sensitivity of selectors").
const CASE_INSENSITIVE_ATTRIBUTES: &[&str] = &[
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

impl SelectorList {
    /// L'élément correspond-il à au moins un des sélecteurs ?
    pub fn matches<E: Element>(&self, element: E) -> bool {
        self.0.iter().any(|s| s.matches(element))
    }
}

impl Selector {
    /// L'élément correspond-il au sélecteur ? (Un sélecteur avec pseudo-élément
    /// ne correspond jamais à l'élément lui-même.)
    pub fn matches<E: Element>(&self, element: E) -> bool {
        self.subject.pseudo_element.is_none()
            && matches_compound(&self.subject, element)
            && matches_ancestors(&self.ancestors, element)
    }
}

/// Vérifie les parties à gauche, avec retour en arrière pour ` ` et `~`.
fn matches_ancestors<E: Element>(parts: &[(Combinator, Compound)], element: E) -> bool {
    let Some(((combinator, compound), rest)) = parts.split_first() else {
        return true;
    };
    match combinator {
        Combinator::Child => element
            .parent_element()
            .is_some_and(|p| matches_compound(compound, p) && matches_ancestors(rest, p)),
        Combinator::NextSibling => element
            .prev_sibling_element()
            .is_some_and(|s| matches_compound(compound, s) && matches_ancestors(rest, s)),
        Combinator::Descendant => {
            let mut current = element.parent_element();
            while let Some(p) = current {
                if matches_compound(compound, p) && matches_ancestors(rest, p) {
                    return true;
                }
                current = p.parent_element();
            }
            false
        }
        Combinator::SubsequentSibling => {
            let mut current = element.prev_sibling_element();
            while let Some(s) = current {
                if matches_compound(compound, s) && matches_ancestors(rest, s) {
                    return true;
                }
                current = s.prev_sibling_element();
            }
            false
        }
    }
}

fn matches_compound<E: Element>(compound: &Compound, element: E) -> bool {
    compound.simple.iter().all(|s| matches_simple(s, element))
}

fn matches_simple<E: Element>(simple: &Simple, e: E) -> bool {
    match simple {
        Simple::Universal => true,
        Simple::Type(name) => {
            if e.is_html() {
                e.local_name().eq_ignore_ascii_case(name)
            } else {
                e.local_name() == name
            }
        }
        Simple::Id(id) => e.id() == Some(id.as_str()),
        Simple::Class(class) => e.has_class(class),
        Simple::Attribute {
            name,
            matcher,
            case_insensitive,
        } => {
            let name = if e.is_html() {
                name.to_ascii_lowercase()
            } else {
                name.clone()
            };
            let Some(actual) = e.attribute(&name) else {
                return false;
            };
            let Some((op, expected)) = matcher else {
                return true;
            };
            let insensitive = case_insensitive.unwrap_or_else(|| {
                e.is_html() && CASE_INSENSITIVE_ATTRIBUTES.contains(&name.as_str())
            });
            matches_attribute(*op, actual, expected, insensitive)
        }
        Simple::PseudoClass(pc) => matches_pseudo_class(pc, e),
    }
}

fn matches_attribute(op: AttrOp, actual: &str, expected: &str, insensitive: bool) -> bool {
    let (actual, expected) = if insensitive {
        (actual.to_ascii_lowercase(), expected.to_ascii_lowercase())
    } else {
        (actual.to_string(), expected.to_string())
    };
    match op {
        AttrOp::Equals => actual == expected,
        AttrOp::Includes => {
            !expected.is_empty()
                && !expected.contains(|c: char| c.is_ascii_whitespace())
                && actual.split_ascii_whitespace().any(|w| w == expected)
        }
        AttrOp::DashMatch => {
            actual == expected
                || actual
                    .strip_prefix(expected.as_str())
                    .is_some_and(|r| r.starts_with('-'))
        }
        AttrOp::Prefix => !expected.is_empty() && actual.starts_with(&expected),
        AttrOp::Suffix => !expected.is_empty() && actual.ends_with(&expected),
        AttrOp::Substring => !expected.is_empty() && actual.contains(&expected),
    }
}

fn same_type<E: Element>(a: &E, b: &E) -> bool {
    a.local_name() == b.local_name() && a.is_html() == b.is_html()
}

/// Position (à partir de 1) parmi les frères qui passent le filtre.
fn nth_index<E: Element>(e: E, from_end: bool, keep: impl Fn(&E) -> bool) -> i32 {
    let mut index = 1;
    let mut current = if from_end {
        e.next_sibling_element()
    } else {
        e.prev_sibling_element()
    };
    while let Some(s) = current {
        if keep(&s) {
            index += 1;
        }
        current = if from_end {
            s.next_sibling_element()
        } else {
            s.prev_sibling_element()
        };
    }
    index
}

/// L'index `n` est-il de la forme `a*k + b` avec `k >= 0` ?
fn matches_an_plus_b(a: i32, b: i32, n: i32) -> bool {
    let (a, b, n) = (a as i64, b as i64, n as i64);
    if a == 0 {
        n == b
    } else {
        let diff = n - b;
        diff % a == 0 && diff / a >= 0
    }
}

fn matches_pseudo_class<E: Element>(pc: &PseudoClass, e: E) -> bool {
    match pc {
        PseudoClass::Root => e.is_root(),
        PseudoClass::Empty => e.is_empty(),
        PseudoClass::FirstChild => e.prev_sibling_element().is_none(),
        PseudoClass::LastChild => e.next_sibling_element().is_none(),
        PseudoClass::OnlyChild => {
            e.prev_sibling_element().is_none() && e.next_sibling_element().is_none()
        }
        PseudoClass::FirstOfType => nth_index(e, false, |s| same_type(s, &e)) == 1,
        PseudoClass::LastOfType => nth_index(e, true, |s| same_type(s, &e)) == 1,
        PseudoClass::OnlyOfType => {
            nth_index(e, false, |s| same_type(s, &e)) == 1
                && nth_index(e, true, |s| same_type(s, &e)) == 1
        }
        PseudoClass::Nth {
            a,
            b,
            from_end,
            of_type,
            of_selector,
        } => {
            if let Some(list) = of_selector {
                if !list.matches(e) {
                    return false;
                }
                matches_an_plus_b(*a, *b, nth_index(e, *from_end, |s| list.matches(*s)))
            } else if *of_type {
                matches_an_plus_b(*a, *b, nth_index(e, *from_end, |s| same_type(s, &e)))
            } else {
                matches_an_plus_b(*a, *b, nth_index(e, *from_end, |_| true))
            }
        }
        PseudoClass::Not(list) => !list.matches(e),
        PseudoClass::Is(list) | PseudoClass::Where(list) => list.matches(e),
        PseudoClass::Never(_) => false,
    }
}
