//! Interning des noms de balises.
//!
//! Le parser compare des noms de balises en permanence ("y a-t-il un `<p>` ouvert ?",
//! "suis-je dans un `<table>` ?"). Comparer des chaînes à chaque fois serait lent :
//! on transforme chaque nom en un petit entier, un `Atom`. Comparer deux atomes
//! revient à comparer deux `u32`.
//!
//! Les noms connus de la spec ont des atomes fixes (`atoms::DIV`, `atoms::P`...),
//! connus à la compilation. Les noms inconnus (`<mon-composant>`) reçoivent un
//! atome à la volée.

use std::collections::HashMap;

/// Un nom de balise interné : comparer deux atomes revient à comparer deux `u32`.
/// Le texte s'obtient avec [`Interner::name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Atom(u32);

macro_rules! static_atoms {
    ($($name:ident = $text:literal,)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum StaticIndex { $($name,)* }

        $(
            #[doc = concat!("L'atome de `", $text, "`.")]
            pub const $name: Atom = Atom(StaticIndex::$name as u32);
        )*

        const STATIC_ATOMS: &[&str] = &[$($text,)*];
    };
}

static_atoms! {
    A = "a",
    ADDRESS = "address",
    ANNOTATION_XML = "annotation-xml",
    APPLET = "applet",
    AREA = "area",
    ARTICLE = "article",
    ASIDE = "aside",
    B = "b",
    BASE = "base",
    BASEFONT = "basefont",
    BGSOUND = "bgsound",
    BIG = "big",
    BLOCKQUOTE = "blockquote",
    BODY = "body",
    BR = "br",
    BUTTON = "button",
    CAPTION = "caption",
    CENTER = "center",
    CODE = "code",
    COL = "col",
    COLGROUP = "colgroup",
    DD = "dd",
    DESC = "desc",
    DETAILS = "details",
    DIALOG = "dialog",
    DIR = "dir",
    DIV = "div",
    DL = "dl",
    DT = "dt",
    EM = "em",
    EMBED = "embed",
    FIELDSET = "fieldset",
    FIGCAPTION = "figcaption",
    FIGURE = "figure",
    FONT = "font",
    FOOTER = "footer",
    FOREIGN_OBJECT = "foreignObject",
    FORM = "form",
    FRAME = "frame",
    FRAMESET = "frameset",
    H1 = "h1",
    H2 = "h2",
    H3 = "h3",
    H4 = "h4",
    H5 = "h5",
    H6 = "h6",
    HEAD = "head",
    HEADER = "header",
    HGROUP = "hgroup",
    HR = "hr",
    HTML = "html",
    I = "i",
    IFRAME = "iframe",
    IMAGE = "image",
    IMG = "img",
    INPUT = "input",
    KEYGEN = "keygen",
    LI = "li",
    LINK = "link",
    LISTING = "listing",
    MAIN = "main",
    MALIGNMARK = "malignmark",
    MARQUEE = "marquee",
    MATH = "math",
    MENU = "menu",
    META = "meta",
    MGLYPH = "mglyph",
    MI = "mi",
    MN = "mn",
    MO = "mo",
    MS = "ms",
    MTEXT = "mtext",
    NAV = "nav",
    NOBR = "nobr",
    NOEMBED = "noembed",
    NOFRAMES = "noframes",
    NOSCRIPT = "noscript",
    OBJECT = "object",
    OL = "ol",
    OPTGROUP = "optgroup",
    OPTION = "option",
    P = "p",
    PARAM = "param",
    PLAINTEXT = "plaintext",
    PRE = "pre",
    RB = "rb",
    RP = "rp",
    RT = "rt",
    RTC = "rtc",
    RUBY = "ruby",
    S = "s",
    SCRIPT = "script",
    SEARCH = "search",
    SECTION = "section",
    SELECT = "select",
    SELECTEDCONTENT = "selectedcontent",
    SMALL = "small",
    SOURCE = "source",
    STRIKE = "strike",
    STRONG = "strong",
    STYLE = "style",
    SUMMARY = "summary",
    SVG = "svg",
    TABLE = "table",
    TBODY = "tbody",
    TD = "td",
    TEMPLATE = "template",
    TEXTAREA = "textarea",
    TFOOT = "tfoot",
    TH = "th",
    THEAD = "thead",
    TITLE = "title",
    TR = "tr",
    TRACK = "track",
    TT = "tt",
    U = "u",
    UL = "ul",
    WBR = "wbr",
    XMP = "xmp",
}

/// Table de correspondance nom <-> atome, propre à chaque document.
#[derive(Debug, Clone)]
pub struct Interner {
    ids: HashMap<Box<str>, Atom>,
    names: Vec<Box<str>>,
}

impl Default for Interner {
    fn default() -> Self {
        let mut interner = Interner {
            ids: HashMap::new(),
            names: Vec::new(),
        };
        for name in STATIC_ATOMS {
            interner.intern(name);
        }
        interner
    }
}

impl Interner {
    /// Renvoie l'atome de `name`, en le créant si c'est la première fois.
    pub fn intern(&mut self, name: &str) -> Atom {
        if let Some(&atom) = self.ids.get(name) {
            return atom;
        }
        let atom = Atom(self.names.len() as u32);
        self.names.push(name.into());
        self.ids.insert(name.into(), atom);
        atom
    }

    /// Le texte d'un atome.
    ///
    /// # Panics
    ///
    /// Si `atom` est un atome dynamique créé par l'`Interner` d'un autre document
    /// (les atomes statiques comme `atoms::DIV` sont valables partout).
    pub fn name(&self, atom: Atom) -> &str {
        &self.names[atom.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomes_statiques_et_dynamiques() {
        let mut atoms = Interner::default();
        assert_eq!(atoms.intern("div"), DIV);
        assert_eq!(atoms.intern("foreignObject"), FOREIGN_OBJECT);
        let custom = atoms.intern("mon-composant");
        assert_eq!(atoms.intern("mon-composant"), custom);
        assert_eq!(atoms.name(custom), "mon-composant");
        assert_eq!(atoms.name(TABLE), "table");
    }
}
