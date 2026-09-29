//! Propriétés personnalisées (`--couleur: red`) et `var()` (spec CSS Custom
//! Properties for Cascading Variables, et CSS Values 5 pour la substitution).
//!
//! Une propriété personnalisée n'a pas de grammaire : sa valeur est le texte
//! d'origine (commentaires compris, espaces des bords retirés), comme le fait
//! Chromium. `var(--x)` est remplacé par ce texte ; si deux tokens voisins se
//! colleraient (`var(--a)px` avec `--a: 1`), on insère `/**/` entre eux pour
//! qu'ils restent deux tokens, exactement comme les navigateurs.
//!
//! Tout est vérifié contre Chromium (tests/oracle_variables.rs).
//!
//! ```
//! use lumen_css::variables::{CustomProperties, substitute};
//!
//! let parent = CustomProperties::default();
//! let vars = CustomProperties::compute([("--gap", "8px"), ("--double", "calc(var(--gap) * 2)")], &parent);
//! assert_eq!(vars.get("--double"), Some("calc(8px * 2)"));
//! assert_eq!(substitute("var(--gap) var(--absent, 0)", &vars).as_deref(), Some("8px 0"));
//! ```

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::iter;
use std::ops::Range;

use crate::tokenizer::{Token, Tokenizer};

/// Une déclaration avec le texte d'origine de sa valeur, telle qu'écrite dans
/// un attribut `style` ou un bloc de règle (sans règles imbriquées).
#[derive(Debug, Clone, PartialEq)]
pub struct RawDeclaration<'a> {
    /// Le nom de la propriété, tel qu'écrit.
    pub name: Cow<'a, str>,
    /// La valeur, espaces des bords et `!important` retirés.
    pub value: &'a str,
    /// La déclaration se terminait par `!important`.
    pub important: bool,
}

type Spanned<'a> = (Token<'a>, Range<usize>);

fn tokens(text: &str) -> Vec<Spanned<'_>> {
    let mut tokenizer = Tokenizer::new(text);
    iter::from_fn(|| tokenizer.next_with_span()).collect()
}

fn is_ws(t: &Token) -> bool {
    matches!(t, Token::Whitespace)
}

fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n'])
}

/// +1 pour ce qui ouvre un bloc, -1 pour ce qui le ferme.
fn nesting(t: &Token) -> i32 {
    match t {
        Token::Function(_) | Token::OpenParen | Token::OpenSquare | Token::OpenCurly => 1,
        Token::CloseParen | Token::CloseSquare | Token::CloseCurly => -1,
        _ => 0,
    }
}

/// Découpe une liste de déclarations (`a: 1; --b: x !important`) en gardant le
/// texte de chaque valeur. L'entrée doit être prétraitée ([`crate::preprocess`]).
/// Les morceaux qui ne sont pas des déclarations sont ignorés.
pub fn raw_declarations(input: &str) -> Vec<RawDeclaration<'_>> {
    let toks = tokens(input);
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for i in 0..=toks.len() {
        if i == toks.len() || (depth == 0 && matches!(toks[i].0, Token::Semicolon)) {
            out.extend(declaration(input, &toks[start..i]));
            start = i + 1;
        } else {
            depth = (depth + nesting(&toks[i].0)).max(0);
        }
    }
    out
}

fn declaration<'a>(input: &'a str, toks: &[Spanned<'a>]) -> Option<RawDeclaration<'a>> {
    let mut rest = toks.iter().filter(|t| !is_ws(&t.0)).peekable();
    let Some((Token::Ident(name), _)) = rest.next() else {
        return None;
    };
    let Some((Token::Colon, colon)) = rest.next() else {
        return None;
    };
    // Les tokens de la valeur, sans les espaces des bords.
    let mut value: Vec<&Spanned> = toks.iter().skip_while(|t| t.1.start < colon.end).collect();
    let trim_ws = |v: &mut Vec<&Spanned>| {
        while v.last().is_some_and(|t| is_ws(&t.0)) {
            v.pop();
        }
    };
    trim_ws(&mut value);
    let mut important = false;
    if let Some((Token::Ident(word), _)) = value.last().map(|t| &**t)
        && word.eq_ignore_ascii_case("important")
    {
        let mut before = value.len() - 1;
        while before > 0 && is_ws(&value[before - 1].0) {
            before -= 1;
        }
        if before > 0 && matches!(value[before - 1].0, Token::Delim('!')) {
            important = true;
            value.truncate(before - 1);
            trim_ws(&mut value);
        }
    }
    let first = value.iter().position(|t| !is_ws(&t.0));
    let text = match (first, value.last()) {
        (Some(first), Some(last)) => &input[value[first].1.start..last.1.end],
        _ => "",
    };
    Some(RawDeclaration {
        name: name.clone(),
        value: text,
        important,
    })
}

/// `--nom` : le nom d'une propriété personnalisée.
pub fn is_custom_property(name: &str) -> bool {
    name.len() > 2 && name.starts_with("--")
}

/// La valeur contient au moins un `var()`.
pub fn contains_var(value: &str) -> bool {
    tokens(value).iter().any(|t| is_var(&t.0))
}

fn is_var(t: &Token) -> bool {
    matches!(t, Token::Function(name) if name.eq_ignore_ascii_case("var"))
}

/// Les mots-clés valables pour toutes les propriétés (`initial`, `inherit`...),
/// si la valeur n'est que l'un d'eux.
pub fn css_wide_keyword(value: &str) -> Option<&'static str> {
    let toks = tokens(trim(value));
    let [(Token::Ident(word), _)] = &toks[..] else {
        return None;
    };
    ["initial", "inherit", "unset", "revert", "revert-layer"]
        .into_iter()
        .find(|k| word.eq_ignore_ascii_case(k))
}

/// La valeur est acceptable au parsing pour une propriété personnalisée, ou pour
/// une propriété qui contient `var()` (vérifiée seulement après substitution) :
/// blocs bien fermés, pas de `!` isolé, pas de chaîne ou d'url cassée, et des
/// `var()` bien formés.
pub fn is_valid_value(value: &str) -> bool {
    let toks = tokens(value);
    let mut closers = Vec::new();
    for (i, (t, _)) in toks.iter().enumerate() {
        match t {
            Token::BadString | Token::BadUrl | Token::Semicolon => return false,
            Token::Delim('!') if closers.is_empty() => return false,
            Token::Function(_) | Token::OpenParen => closers.push(')'),
            Token::OpenSquare => closers.push(']'),
            Token::OpenCurly => closers.push('}'),
            Token::CloseParen | Token::CloseSquare | Token::CloseCurly => {
                let c = match t {
                    Token::CloseParen => ')',
                    Token::CloseSquare => ']',
                    _ => '}',
                };
                if closers.pop() != Some(c) {
                    return false;
                }
            }
            _ => {}
        }
        if is_var(t) {
            let close = matching_close(&toks, i);
            if var_arguments(value, &toks[i + 1..close], 0).is_none() {
                return false;
            }
        }
    }
    true
}

/// L'indice du token qui ferme le bloc ouvert en `open` (ou la fin de la liste).
fn matching_close(toks: &[Spanned], open: usize) -> usize {
    let mut depth = 0;
    for (i, (t, _)) in toks.iter().enumerate().skip(open) {
        depth += nesting(t);
        if depth == 0 {
            return i;
        }
    }
    toks.len()
}

/// Les arguments de `var( --nom [, repli]? )` : le nom et le texte du repli.
/// `end` est la position de la parenthèse fermante (la fin du texte sinon).
fn var_arguments<'t>(
    text: &'t str,
    args: &[Spanned<'t>],
    end: usize,
) -> Option<(Cow<'t, str>, Option<&'t str>)> {
    let mut rest = args.iter().filter(|t| !is_ws(&t.0));
    let Some((Token::Ident(name), _)) = rest.next() else {
        return None;
    };
    if !is_custom_property(name) {
        return None;
    }
    match rest.next() {
        None => Some((name.clone(), None)),
        Some((Token::Comma, comma)) => {
            let end = end.max(comma.end);
            Some((name.clone(), Some(trim(&text[comma.end..end]))))
        }
        Some(_) => None,
    }
}

// ───────────── Sortie : texte substitué, avec `/**/` si besoin ─────────────

/// Le genre d'un token, pour savoir si deux tokens voisins se colleraient.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Ident,
    Function,
    Url,
    AtKeyword,
    Hash,
    Dimension,
    Number,
    Percentage,
    Cdc,
    OpenParen,
    Delim(char),
    Other,
}

fn kind(t: &Token) -> Kind {
    match t {
        Token::Ident(_) => Kind::Ident,
        Token::Function(_) => Kind::Function,
        Token::Url(_) | Token::BadUrl => Kind::Url,
        Token::AtKeyword(_) => Kind::AtKeyword,
        Token::Hash { .. } => Kind::Hash,
        Token::Dimension { .. } => Kind::Dimension,
        Token::Number(_) => Kind::Number,
        Token::Percentage(_) => Kind::Percentage,
        Token::Cdc => Kind::Cdc,
        Token::OpenParen => Kind::OpenParen,
        Token::Delim(c) => Kind::Delim(*c),
        _ => Kind::Other,
    }
}

/// La table de la spec CSS Syntax (§9, sérialisation) : faut-il un commentaire
/// entre `a` et `b` ? Vérifiée paire par paire contre Chromium.
fn needs_comment(a: Kind, b: Kind) -> bool {
    use Kind::*;
    let word_or_number = matches!(
        b,
        Ident | Function | Url | Delim('-') | Number | Percentage | Dimension | Cdc
    );
    match a {
        Ident => word_or_number || b == OpenParen,
        AtKeyword | Hash | Dimension | Delim('#') | Delim('-') => word_or_number,
        Number => matches!(
            b,
            Ident | Function | Url | Number | Percentage | Dimension | Cdc | Delim('%')
        ),
        Delim('@') => matches!(b, Ident | Function | Url | Delim('-') | Cdc),
        Delim('.') | Delim('+') => matches!(b, Number | Percentage | Dimension),
        Delim('/') => b == Delim('*'),
        _ => false,
    }
}

#[derive(Default)]
struct Output {
    text: String,
    /// Le dernier token écrit (`None` s'il est suivi d'un commentaire).
    last: Option<Kind>,
}

impl Output {
    fn push(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let toks = tokens(s);
        let first = toks.first().filter(|t| t.1.start == 0).map(|t| kind(&t.0));
        if let (Some(a), Some(b)) = (self.last, first)
            && needs_comment(a, b)
        {
            self.text.push_str("/**/");
        }
        self.text.push_str(s);
        self.last = toks
            .last()
            .filter(|t| t.1.end == s.len())
            .map(|t| kind(&t.0));
    }
}

// ───────────── Substitution ─────────────

/// Où trouver la valeur d'une variable.
trait Lookup {
    fn lookup(&mut self, name: &str) -> Option<String>;
}

impl Lookup for &CustomProperties {
    fn lookup(&mut self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

/// Taille maximale d'une valeur substituée. Sans limite, 30 variables qui
/// doublent chacune la précédente (`--b: var(--a) var(--a)`...) produiraient
/// des gigaoctets. Au-delà, la valeur est invalide, comme dans Chromium (qui
/// garde 1,4 Mo et refuse 2,9 Mo).
pub const MAX_SUBSTITUTION_BYTES: usize = 2 * 1024 * 1024;

/// Remplace les `var()` de `text`. `None` : un `var()` sans valeur ni repli (la
/// valeur est "invalide au moment du calcul"), ou un résultat trop grand.
fn substitute_with(text: &str, lookup: &mut impl Lookup) -> Option<String> {
    let toks = tokens(text);
    let mut out = Output::default();
    let mut copied = 0; // octets de `text` déjà recopiés
    let mut i = 0;
    while i < toks.len() {
        if !is_var(&toks[i].0) {
            i += 1;
            continue;
        }
        let close = matching_close(&toks, i);
        let end = toks.get(close).map_or(text.len(), |t| t.1.start);
        out.push(&text[copied..toks[i].1.start]);
        let (name, fallback) = var_arguments(text, &toks[i + 1..close], end)?;
        let value = match lookup.lookup(&name) {
            Some(value) => value,
            None => substitute_with(fallback?, lookup)?,
        };
        out.push(&value);
        if out.text.len() > MAX_SUBSTITUTION_BYTES {
            return None;
        }
        copied = toks.get(close).map_or(text.len(), |t| t.1.end);
        i = close + 1;
    }
    out.push(&text[copied..]);
    (out.text.len() <= MAX_SUBSTITUTION_BYTES).then_some(out.text)
}

/// Remplace les `var()` d'une valeur par les propriétés personnalisées de
/// l'élément. `None` si un `var()` n'a ni valeur ni repli : la déclaration est
/// alors "invalide au moment du calcul" (la propriété se comporte comme `unset`).
pub fn substitute(value: &str, vars: &CustomProperties) -> Option<String> {
    substitute_with(value, &mut &*vars).map(|s| trim(&s).to_string())
}

/// Les propriétés personnalisées d'un élément, une fois calculées : les `var()`
/// sont remplacés, les cycles et les valeurs invalides retirés.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomProperties(HashMap<String, String>);

/// Calcule les propriétés dont la valeur contient `var()`, à la demande (un
/// repli non utilisé ne crée pas de dépendance, comme dans Chromium).
struct Resolver<'a> {
    pending: HashMap<&'a str, &'a str>,
    values: HashMap<String, String>,
    stack: Vec<&'a str>,
    cyclic: HashSet<&'a str>,
}

impl<'a> Resolver<'a> {
    fn resolve(&mut self, name: &'a str, text: &'a str) -> Option<String> {
        self.stack.push(name);
        let result = substitute_with(text, self);
        self.stack.pop();
        match result {
            Some(value) if !self.cyclic.contains(name) => {
                let value = trim(&value).to_string();
                self.values.insert(name.to_string(), value.clone());
                Some(value)
            }
            // Cycle ou var() sans repli : la valeur "invalide garantie" (vide).
            _ => {
                self.values.remove(name);
                None
            }
        }
    }
}

impl Lookup for Resolver<'_> {
    fn lookup(&mut self, name: &str) -> Option<String> {
        if let Some(pos) = self.stack.iter().position(|n| *n == name) {
            // Toutes les propriétés entre les deux apparitions forment un cycle.
            self.cyclic.extend(&self.stack[pos..]);
            return None;
        }
        match self.pending.remove_entry(name) {
            Some((name, text)) => self.resolve(name, text),
            None => self.values.get(name).cloned(),
        }
    }
}

impl CustomProperties {
    /// La valeur calculée de `--nom`, si elle existe.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// Le nombre de propriétés définies.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Aucune propriété définie.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Calcule les propriétés personnalisées d'un élément à partir de ses
    /// déclarations `(--nom, valeur)`, dans l'ordre de la cascade (la dernière
    /// gagne), et de celles de son parent (elles sont héritées).
    /// Les valeurs invalides au parsing sont ignorées.
    pub fn compute<'a>(
        declarations: impl IntoIterator<Item = (&'a str, &'a str)>,
        parent: &CustomProperties,
    ) -> CustomProperties {
        let mut declared: HashMap<&str, &str> = HashMap::new();
        for (name, value) in declarations {
            if is_custom_property(name) && is_valid_value(value) {
                declared.insert(name, trim(value));
            }
        }
        let mut resolver = Resolver {
            pending: HashMap::new(),
            values: parent.0.clone(),
            stack: Vec::new(),
            cyclic: HashSet::new(),
        };
        for (name, value) in declared {
            match css_wide_keyword(value) {
                Some("initial") => {
                    resolver.values.remove(name);
                }
                // inherit, unset, revert : la valeur du parent, déjà là.
                Some(_) => {}
                None if contains_var(value) => {
                    resolver.values.remove(name);
                    resolver.pending.insert(name, value);
                }
                None => {
                    resolver.values.insert(name.to_string(), value.to_string());
                }
            }
        }
        while let Some((&name, &text)) = resolver.pending.iter().next() {
            resolver.pending.remove(name);
            resolver.resolve(name, text);
        }
        CustomProperties(resolver.values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_brutes() {
        let d = raw_declarations("a: 1 ; --b:  x /*c*/ y  !IMPORTANT; ; c: f(;) ; nope");
        assert_eq!(d.len(), 3);
        assert_eq!(
            (d[0].name.as_ref(), d[0].value, d[0].important),
            ("a", "1", false)
        );
        assert_eq!(
            (d[1].name.as_ref(), d[1].value, d[1].important),
            ("--b", "x /*c*/ y", true)
        );
        assert_eq!(d[2].value, "f(;)");
    }

    #[test]
    fn cycle_et_repli() {
        let vars = CustomProperties::compute(
            [
                ("--a", "var(--b)"),
                ("--b", "var(--a)"),
                ("--c", "var(--a, ok)"),
            ],
            &CustomProperties::default(),
        );
        assert_eq!(vars.get("--a"), None);
        assert_eq!(vars.get("--b"), None);
        assert_eq!(vars.get("--c"), Some("ok"));
    }

    #[test]
    fn croissance_exponentielle_bornee() {
        // Chaque variable double la précédente : 10 × 2^n octets environ.
        let names: Vec<String> = (0..=30).map(|i| format!("--v{i}")).collect();
        let values: Vec<String> = (0..=30)
            .map(|i| match i {
                0 => "xxxxxxxxxx".to_string(),
                _ => format!("var(--v{}) var(--v{})", i - 1, i - 1),
            })
            .collect();
        let vars = CustomProperties::compute(
            names
                .iter()
                .zip(&values)
                .map(|(n, v)| (n.as_str(), v.as_str())),
            &CustomProperties::default(),
        );
        // Mêmes tailles que Chromium : 1 441 791 octets gardés, la suite refusée.
        assert_eq!(vars.get("--v17").map(str::len), Some(1_441_791));
        assert_eq!(vars.get("--v18"), None);
        assert_eq!(vars.get("--v30"), None);
    }
}
