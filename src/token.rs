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
    /// Balise ouvrante : `<div class="x">`.
    StartTag(Tag<'a>),
    /// Balise fermante : `</div>`.
    EndTag(Tag<'a>),
    /// Commentaire : `<!-- ... -->`.
    Comment(String),
    /// `<?cible données?>` (ajouté à la spec HTML en 2026).
    ProcessingInstruction {
        /// La cible (`xml-stylesheet`...).
        target: String,
        /// Les données.
        data: String,
    },
    /// `<!DOCTYPE html>`.
    Doctype(Doctype),
    /// Fin de la page.
    Eof,
}

/// Une balise ouvrante ou fermante.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag<'a> {
    /// Nom, en minuscules.
    pub name: Cow<'a, str>,
    /// Écrite `<br/>` (le parser décide si ça a un sens).
    pub self_closing: bool,
    /// Attributs, sans doublons (le premier gagne).
    pub attributes: Vec<Attribute<'a>>,
}

/// Un attribut de balise.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attribute<'a> {
    /// Nom, en minuscules.
    pub name: Cow<'a, str>,
    /// Valeur, entités décodées.
    pub value: Cow<'a, str>,
}

/// Un `<!DOCTYPE ...>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doctype {
    /// Nom, en minuscules (`html`).
    pub name: Option<String>,
    /// Identifiant public (`-//W3C//DTD HTML 4.01//EN`...).
    pub public_id: Option<String>,
    /// Identifiant système (une URL de DTD).
    pub system_id: Option<String>,
    /// Doctype mal formé : la page passera en mode quirks.
    pub force_quirks: bool,
}
