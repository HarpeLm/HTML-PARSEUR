use std::borrow::Cow;

/// Les 6 types de tokens définis par la spec WHATWG (section 13.2.5).
///
/// `'a` est la durée de vie de la page HTML d'entrée : quand c'est possible, les
/// textes et les noms sont des morceaux EMPRUNTÉS à la page (`Cow::Borrowed`),
/// sans aucune copie. On ne copie (`Cow::Owned`) que si le contenu a été
/// transformé : entité remplacée, majuscules passées en minuscules, \r\n...
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    /// Une suite de caractères consécutifs (la spec émet un token par caractère,
    /// on les regroupe : c'est beaucoup plus rapide).
    Characters(Cow<'a, str>),
    StartTag(Tag<'a>),
    EndTag(Tag<'a>),
    Comment(String),
    Doctype(Doctype),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag<'a> {
    pub name: Cow<'a, str>,
    pub self_closing: bool,
    pub attributes: Vec<Attribute<'a>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attribute<'a> {
    pub name: Cow<'a, str>,
    pub value: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}