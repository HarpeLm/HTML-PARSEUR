//! Construction de l'arbre DOM à partir des tokens (spec §13.2.6).
//!
//! Le parser est lui aussi une machine à états : le "mode d'insertion" (avant
//! <html>, dans <head>, dans <body>...) décide quoi faire de chaque token. Il
//! maintient deux structures centrales :
//! - la PILE des éléments ouverts (`open`) : le chemin depuis <html> jusqu'à
//!   l'élément où on insère ;
//! - la liste des éléments de FORMATAGE actifs (`formatting`) : les <b>, <i>, <a>...
//!   qu'il faut "rouvrir" quand le HTML est mal imbriqué.

use std::borrow::Cow;

use crate::atoms::{self, Atom};
use crate::dom::{AttrNamespace, Attribute, Document, Element, Namespace, NodeData, NodeId, QuirksMode};
use crate::token::{Doctype, Token};
use crate::tokenizer::{InitialState, Tokenizer};

/// Parse une page HTML complète et renvoie son DOM.
pub fn parse_document(html: &str) -> Document {
    let mut builder = TreeBuilder::default();
    let mut tokenizer = Tokenizer::new(html);
    while let Some(token) = tokenizer.next() {
        builder.process_token(token);
        // Après <title>, <script>, <textarea>... le parser change l'état du tokenizer.
        if let Some(state) = builder.tokenizer_state.take() {
            tokenizer.set_state(state);
        }
    }
    builder.doc
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    InHeadNoscript,
    AfterHead,
    InBody,
    Text,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}

/// Une balise telle que le parser la manipule : nom interné, attributs possédés.
#[derive(Debug, Clone)]
struct TagToken {
    name: Atom,
    attrs: Vec<Attribute>,
    self_closing: bool,
}

/// Token interne. Le texte reste emprunté : on peut le découper (espaces en tête,
/// reste...) et retraiter chaque morceau sans rien copier.
#[derive(Debug)]
enum Tok<'t> {
    Text(&'t str),
    Start(TagToken),
    End(TagToken),
    Comment(String),
    Doctype(Doctype),
    Eof,
}

#[derive(Debug, Clone)]
enum Formatting {
    Marker,
    Element(NodeId, TagToken),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Default,
    ListItem,
    Button,
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | '\r' | ' ')
}

/// Sépare "   texte" en ("   ", "texte").
fn split_leading_ws(s: &str) -> (&str, &str) {
    let n = s.find(|c| !is_ws(c)).unwrap_or(s.len());
    s.split_at(n)
}

const HEADINGS: &[Atom] = &[atoms::H1, atoms::H2, atoms::H3, atoms::H4, atoms::H5, atoms::H6];

pub struct TreeBuilder {
    doc: Document,
    mode: Mode,
    original_mode: Mode,
    open: Vec<NodeId>,
    formatting: Vec<Formatting>,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    /// Ignorer un '\n' juste après <pre>, <listing>, <textarea>.
    ignore_lf: bool,
    scripting: bool,
    /// Nouvel état demandé au tokenizer (lu par `parse_document`).
    tokenizer_state: Option<InitialState>,
}

impl Default for TreeBuilder {
    fn default() -> Self {
        TreeBuilder {
            doc: Document::default(),
            mode: Mode::Initial,
            original_mode: Mode::Initial,
            open: Vec::new(),
            formatting: Vec::new(),
            head: None,
            form: None,
            frameset_ok: true,
            ignore_lf: false,
            scripting: false,
            tokenizer_state: None,
        }
    }
}

impl TreeBuilder {
    // ───────────── Entrée : conversion des tokens du tokenizer ─────────────

    fn process_token(&mut self, token: Token<'_>) {
        let tok = match token {
            Token::Characters(text) => {
                let mut text: &str = &text;
                if std::mem::take(&mut self.ignore_lf) {
                    text = text.strip_prefix('\n').unwrap_or(text);
                    if text.is_empty() {
                        return;
                    }
                }
                // Le texte est emprunté au Cow : on le traite ici directement.
                return self.process(Tok::Text(text));
            }
            Token::StartTag(tag) => Tok::Start(self.convert_tag(tag.name, tag.attributes, tag.self_closing)),
            Token::EndTag(tag) => Tok::End(self.convert_tag(tag.name, tag.attributes, tag.self_closing)),
            Token::Comment(data) => Tok::Comment(data),
            Token::Doctype(d) => Tok::Doctype(d),
            Token::Eof => Tok::Eof,
        };
        self.ignore_lf = false;
        self.process(tok);
    }

    fn convert_tag(
        &mut self,
        name: Cow<'_, str>,
        attributes: Vec<crate::token::Attribute<'_>>,
        self_closing: bool,
    ) -> TagToken {
        TagToken {
            name: self.doc.atoms.intern(&name),
            attrs: attributes
                .into_iter()
                .map(|a| Attribute { ns: AttrNamespace::None, name: a.name.into_owned(), value: a.value.into_owned() })
                .collect(),
            self_closing,
        }
    }

    /// Traite un token ; un mode peut demander à le retraiter (souvent après avoir
    /// changé de mode) en le renvoyant.
    fn process<'t>(&mut self, tok: Tok<'t>) {
        let mut tok = tok;
        while let Some(again) = self.dispatch(tok) {
            tok = again;
        }
    }

    fn dispatch<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match self.mode {
            Mode::Initial => self.initial(tok),
            Mode::BeforeHtml => self.before_html(tok),
            Mode::BeforeHead => self.before_head(tok),
            Mode::InHead => self.in_head(tok),
            Mode::InHeadNoscript => self.in_head_noscript(tok),
            Mode::AfterHead => self.after_head(tok),
            Mode::InBody => self.in_body(tok),
            Mode::Text => self.text(tok),
            Mode::AfterBody => self.after_body(tok),
            Mode::InFrameset => self.in_frameset(tok),
            Mode::AfterFrameset => self.after_frameset(tok),
            Mode::AfterAfterBody => self.after_after_body(tok),
            Mode::AfterAfterFrameset => self.after_after_frameset(tok),
        }
    }

    // ───────────── Outils sur la pile et le DOM ─────────────

    fn current(&self) -> NodeId {
        *self.open.last().expect("pile des éléments ouverts vide")
    }

    fn is_html(&self, id: NodeId, name: Atom) -> bool {
        self.doc.element(id).is_some_and(|e| e.ns == Namespace::Html && e.name == name)
    }

    fn is_html_any(&self, id: NodeId, names: &[Atom]) -> bool {
        self.doc.element(id).is_some_and(|e| e.ns == Namespace::Html && names.contains(&e.name))
    }

    /// Catégorie "special" de la spec (§13.2.4.2).
    fn is_special(&self, id: NodeId) -> bool {
        use atoms::*;
        let Some(e) = self.doc.element(id) else { return false };
        match e.ns {
            Namespace::Html => [
                ADDRESS, APPLET, AREA, ARTICLE, ASIDE, BASE, BASEFONT, BGSOUND, BLOCKQUOTE, BODY, BR,
                BUTTON, CAPTION, CENTER, COL, COLGROUP, DD, DETAILS, DIR, DIV, DL, DT, EMBED,
                FIELDSET, FIGCAPTION, FIGURE, FOOTER, FORM, FRAME, FRAMESET, H1, H2, H3, H4, H5, H6,
                HEAD, HEADER, HGROUP, HR, HTML, IFRAME, IMG, INPUT, KEYGEN, LI, LINK, LISTING, MAIN,
                MARQUEE, MENU, META, NAV, NOEMBED, NOFRAMES, NOSCRIPT, OBJECT, OL, P, PARAM,
                PLAINTEXT, PRE, SCRIPT, SEARCH, SECTION, SELECT, SOURCE, STYLE, SUMMARY, TABLE,
                TBODY, TD, TEMPLATE, TEXTAREA, TFOOT, TH, THEAD, TITLE, TR, TRACK, UL, WBR, XMP,
            ]
            .contains(&e.name),
            Namespace::MathMl => [MI, MO, MN, MS, MTEXT, ANNOTATION_XML].contains(&e.name),
            Namespace::Svg => [FOREIGN_OBJECT, DESC, TITLE].contains(&e.name),
        }
    }

    /// Les éléments qui "arrêtent" la recherche dans une portée (§13.2.4.2).
    fn is_scope_boundary(&self, id: NodeId, scope: Scope) -> bool {
        use atoms::*;
        let Some(e) = self.doc.element(id) else { return false };
        let default = match e.ns {
            Namespace::Html => {
                [APPLET, CAPTION, HTML, TABLE, TD, TH, MARQUEE, OBJECT, TEMPLATE].contains(&e.name)
            }
            Namespace::MathMl => [MI, MO, MN, MS, MTEXT, ANNOTATION_XML].contains(&e.name),
            Namespace::Svg => [FOREIGN_OBJECT, DESC, TITLE].contains(&e.name),
        };
        default
            || match scope {
                Scope::Default => false,
                Scope::ListItem => self.is_html_any(id, &[OL, UL]),
                Scope::Button => self.is_html(id, BUTTON),
            }
    }

    /// "Has an element in scope" : cherche un élément HTML de ce nom en descendant
    /// la pile, jusqu'à tomber sur une frontière de portée.
    fn in_scope_any(&self, names: &[Atom], scope: Scope) -> bool {
        for &node in self.open.iter().rev() {
            if self.is_html_any(node, names) {
                return true;
            }
            if self.is_scope_boundary(node, scope) {
                return false;
            }
        }
        false
    }

    fn in_scope(&self, name: Atom, scope: Scope) -> bool {
        self.in_scope_any(&[name], scope)
    }

    fn node_in_scope(&self, target: NodeId) -> bool {
        for &node in self.open.iter().rev() {
            if node == target {
                return true;
            }
            if self.is_scope_boundary(node, Scope::Default) {
                return false;
            }
        }
        false
    }

    /// Dépile jusqu'à avoir retiré un élément HTML dont le nom est dans `names`.
    fn pop_until_any(&mut self, names: &[Atom]) {
        while let Some(node) = self.open.pop() {
            if self.is_html_any(node, names) {
                break;
            }
        }
    }

    fn pop_until(&mut self, name: Atom) {
        self.pop_until_any(&[name]);
    }

    /// §13.2.6.3 : ferme les éléments dont la balise fermante est implicite.
    fn generate_implied_end_tags(&mut self, except: Option<Atom>) {
        use atoms::*;
        while let Some(&node) = self.open.last() {
            let implied = self.is_html_any(node, &[DD, DT, LI, OPTGROUP, OPTION, P, RB, RP, RT, RTC]);
            if !implied || except.is_some_and(|name| self.is_html(node, name)) {
                break;
            }
            self.open.pop();
        }
    }

    fn close_p(&mut self) {
        self.generate_implied_end_tags(Some(atoms::P));
        self.pop_until(atoms::P);
    }

    fn close_p_if_in_button_scope(&mut self) {
        if self.in_scope(atoms::P, Scope::Button) {
            self.close_p();
        }
    }

    fn template_on_stack(&self) -> bool {
        self.open.iter().any(|&n| self.is_html(n, atoms::TEMPLATE))
    }

    /// L'endroit où insérer un nouveau nœud (§13.2.6.1) : (parent, avant quel nœud).
    fn insertion_place(&self, target: Option<NodeId>) -> (NodeId, Option<NodeId>) {
        let target = target.unwrap_or_else(|| self.current());
        if let Some(contents) = self.doc.element(target).and_then(|e| e.template_contents) {
            return (contents, None);
        }
        (target, None)
    }

    fn create_element(&mut self, tag: &TagToken, ns: Namespace) -> NodeId {
        let template_contents = if ns == Namespace::Html && tag.name == atoms::TEMPLATE {
            Some(self.doc.create(NodeData::DocumentFragment))
        } else {
            None
        };
        self.doc.create(NodeData::Element(Element {
            ns,
            name: tag.name,
            attrs: tag.attrs.clone(),
            template_contents,
        }))
    }

    /// Crée l'élément, l'insère à l'endroit approprié et l'empile.
    fn insert_element(&mut self, tag: &TagToken, ns: Namespace) -> NodeId {
        let node = self.create_element(tag, ns);
        let (parent, before) = self.insertion_place(None);
        self.doc.insert_before(parent, node, before);
        self.open.push(node);
        node
    }

    fn insert_html(&mut self, tag: &TagToken) -> NodeId {
        self.insert_element(tag, Namespace::Html)
    }

    /// Élément vide (<br>, <img>...) : inséré puis immédiatement dépilé.
    fn insert_void(&mut self, tag: &TagToken) {
        self.insert_html(tag);
        self.open.pop();
    }

    fn insert_text(&mut self, text: &str) {
        let (parent, before) = self.insertion_place(None);
        if parent == NodeId::DOCUMENT {
            return;
        }
        self.doc.insert_text(parent, before, text);
    }

    fn insert_comment(&mut self, data: String, parent: Option<NodeId>) {
        let (parent, before) = match parent {
            Some(p) => (p, None),
            None => self.insertion_place(None),
        };
        let node = self.doc.create(NodeData::Comment(data));
        self.doc.insert_before(parent, node, before);
    }

    /// Ajoute à l'élément `target` les attributs qu'il n'a pas encore.
    fn merge_attributes(&mut self, target: NodeId, tag: &TagToken) {
        if let Some(e) = self.doc.element_mut(target) {
            for attr in &tag.attrs {
                if !e.attrs.iter().any(|a| a.name == attr.name) {
                    e.attrs.push(attr.clone());
                }
            }
        }
    }

    /// Algorithmes génériques pour <title>/<textarea> (RCDATA) et <style>... (RAWTEXT).
    fn parse_raw_text(&mut self, tag: &TagToken, state: InitialState) {
        self.insert_html(tag);
        self.tokenizer_state = Some(state);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    // ───────────── Éléments de formatage actifs (§13.2.4.3) ─────────────

    fn push_formatting(&mut self, node: NodeId, tag: &TagToken) {
        // Clause "de l'arche de Noé" : au plus 3 éléments identiques depuis le dernier
        // marqueur. Sinon, on oublie le plus ancien.
        let same = |entry: &Formatting| match entry {
            Formatting::Element(_, t) => {
                t.name == tag.name && t.attrs.len() == tag.attrs.len()
                    && t.attrs.iter().all(|a| tag.attrs.contains(a))
            }
            Formatting::Marker => false,
        };
        let start = self.last_marker_index().map_or(0, |i| i + 1);
        let matches: Vec<usize> = (start..self.formatting.len()).filter(|&i| same(&self.formatting[i])).collect();
        if matches.len() >= 3 {
            self.formatting.remove(matches[0]);
        }
        self.formatting.push(Formatting::Element(node, tag.clone()));
    }

    fn last_marker_index(&self) -> Option<usize> {
        self.formatting.iter().rposition(|f| matches!(f, Formatting::Marker))
    }

    fn clear_formatting_to_last_marker(&mut self) {
        while let Some(entry) = self.formatting.pop() {
            if matches!(entry, Formatting::Marker) {
                break;
            }
        }
    }

    /// Rouvre les éléments de formatage fermés trop tôt : dans `<b>1<p>2`, le "2"
    /// doit être en gras, donc on recrée un <b> dans le <p>.
    fn reconstruct_formatting(&mut self) {
        let is_open_or_marker = |this: &Self, entry: &Formatting| match entry {
            Formatting::Marker => true,
            Formatting::Element(node, _) => this.open.contains(node),
        };
        let Some(last) = self.formatting.last() else { return };
        if is_open_or_marker(self, last) {
            return;
        }
        // Remonter jusqu'au premier élément à rouvrir...
        let mut i = self.formatting.len() - 1;
        while i > 0 && !is_open_or_marker(self, &self.formatting[i - 1]) {
            i -= 1;
        }
        // ...puis les recréer dans l'ordre.
        for j in i..self.formatting.len() {
            let Formatting::Element(_, tag) = self.formatting[j].clone() else { continue };
            let node = self.insert_html(&tag);
            self.formatting[j] = Formatting::Element(node, tag);
        }
    }

    fn remove_from_formatting(&mut self, node: NodeId) {
        self.formatting.retain(|f| !matches!(f, Formatting::Element(n, _) if *n == node));
    }

    // ───────────── Modes d'insertion ─────────────

    // §13.2.6.4.1
    fn initial<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Text(s) => {
                let (_, rest) = split_leading_ws(s);
                if rest.is_empty() {
                    return None;
                }
                self.initial_anything_else(Tok::Text(rest))
            }
            Tok::Comment(data) => {
                self.insert_comment(data, Some(NodeId::DOCUMENT));
                None
            }
            Tok::Doctype(d) => {
                self.doc.quirks_mode = quirks_mode_for(&d);
                let node = self.doc.create(NodeData::Doctype {
                    name: d.name.unwrap_or_default(),
                    public_id: d.public_id.unwrap_or_default(),
                    system_id: d.system_id.unwrap_or_default(),
                });
                self.doc.append(NodeId::DOCUMENT, node);
                self.mode = Mode::BeforeHtml;
                None
            }
            tok => self.initial_anything_else(tok),
        }
    }

    fn initial_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        self.doc.quirks_mode = QuirksMode::Quirks;
        self.mode = Mode::BeforeHtml;
        Some(tok)
    }

    // §13.2.6.4.2
    fn before_html<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Doctype(_) => None,
            Tok::Comment(data) => {
                self.insert_comment(data, Some(NodeId::DOCUMENT));
                None
            }
            Tok::Text(s) => {
                let (_, rest) = split_leading_ws(s);
                if rest.is_empty() {
                    return None;
                }
                self.before_html_anything_else(Tok::Text(rest))
            }
            Tok::Start(tag) if tag.name == atoms::HTML => {
                let node = self.create_element(&tag, Namespace::Html);
                self.doc.append(NodeId::DOCUMENT, node);
                self.open.push(node);
                self.mode = Mode::BeforeHead;
                None
            }
            Tok::End(ref tag) if ![atoms::HEAD, atoms::BODY, atoms::HTML, atoms::BR].contains(&tag.name) => None,
            tok => self.before_html_anything_else(tok),
        }
    }

    fn before_html_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        let tag = TagToken { name: atoms::HTML, attrs: Vec::new(), self_closing: false };
        let node = self.create_element(&tag, Namespace::Html);
        self.doc.append(NodeId::DOCUMENT, node);
        self.open.push(node);
        self.mode = Mode::BeforeHead;
        Some(tok)
    }

    // §13.2.6.4.3
    fn before_head<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Text(s) => {
                let (_, rest) = split_leading_ws(s);
                if rest.is_empty() {
                    return None;
                }
                self.before_head_anything_else(Tok::Text(rest))
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(ref tag) if tag.name == atoms::HTML => self.in_body(tok),
            Tok::Start(tag) if tag.name == atoms::HEAD => {
                self.head = Some(self.insert_html(&tag));
                self.mode = Mode::InHead;
                None
            }
            Tok::End(ref tag) if ![atoms::HEAD, atoms::BODY, atoms::HTML, atoms::BR].contains(&tag.name) => None,
            tok => self.before_head_anything_else(tok),
        }
    }

    fn before_head_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        let tag = TagToken { name: atoms::HEAD, attrs: Vec::new(), self_closing: false };
        self.head = Some(self.insert_html(&tag));
        self.mode = Mode::InHead;
        Some(tok)
    }

    // §13.2.6.4.4
    fn in_head<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        use atoms::*;
        match tok {
            Tok::Text(s) => {
                let (ws, rest) = split_leading_ws(s);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if rest.is_empty() {
                    return None;
                }
                self.in_head_anything_else(Tok::Text(rest))
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(ref tag) if tag.name == HTML => self.in_body(tok),
            Tok::Start(tag) if [BASE, BASEFONT, BGSOUND, LINK, META].contains(&tag.name) => {
                self.insert_void(&tag);
                None
            }
            Tok::Start(tag) if tag.name == TITLE => {
                self.parse_raw_text(&tag, InitialState::Rcdata);
                None
            }
            Tok::Start(tag)
                if tag.name == NOFRAMES || tag.name == STYLE || (tag.name == NOSCRIPT && self.scripting) =>
            {
                self.parse_raw_text(&tag, InitialState::Rawtext);
                None
            }
            Tok::Start(tag) if tag.name == NOSCRIPT => {
                self.insert_html(&tag);
                self.mode = Mode::InHeadNoscript;
                None
            }
            Tok::Start(tag) if tag.name == SCRIPT => {
                self.parse_raw_text(&tag, InitialState::ScriptData);
                None
            }
            Tok::End(tag) if tag.name == HEAD => {
                self.open.pop();
                self.mode = Mode::AfterHead;
                None
            }
            Tok::Start(ref tag) if tag.name == TEMPLATE => {
                // Templates : étape suivante. Pour l'instant, un élément normal.
                let Tok::Start(tag) = tok else { unreachable!() };
                self.insert_html(&tag);
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
                None
            }
            Tok::End(tag) if tag.name == TEMPLATE => {
                if !self.template_on_stack() {
                    return None;
                }
                self.generate_implied_end_tags(None);
                self.pop_until(TEMPLATE);
                self.clear_formatting_to_last_marker();
                None
            }
            Tok::Start(ref tag) if tag.name == HEAD => None,
            Tok::End(ref tag) if ![BODY, HTML, BR].contains(&tag.name) => None,
            tok => self.in_head_anything_else(tok),
        }
    }

    fn in_head_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        self.open.pop();
        self.mode = Mode::AfterHead;
        Some(tok)
    }

    // §13.2.6.4.5
    fn in_head_noscript<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        use atoms::*;
        match tok {
            Tok::Doctype(_) => None,
            Tok::Start(ref tag) if tag.name == HTML => self.in_body(tok),
            Tok::End(ref tag) if tag.name == NOSCRIPT => {
                self.open.pop();
                self.mode = Mode::InHead;
                None
            }
            Tok::Text(s) => {
                let (ws, rest) = split_leading_ws(s);
                if !ws.is_empty() {
                    self.in_head(Tok::Text(ws));
                }
                if rest.is_empty() {
                    return None;
                }
                self.in_head_noscript_anything_else(Tok::Text(rest))
            }
            Tok::Comment(_) => self.in_head(tok),
            Tok::Start(ref tag) if [BASEFONT, BGSOUND, LINK, META, NOFRAMES, STYLE].contains(&tag.name) => {
                self.in_head(tok)
            }
            Tok::Start(ref tag) if tag.name == HEAD || tag.name == NOSCRIPT => None,
            Tok::End(ref tag) if tag.name != BR => None,
            tok => self.in_head_noscript_anything_else(tok),
        }
    }

    fn in_head_noscript_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        self.open.pop();
        self.mode = Mode::InHead;
        Some(tok)
    }

    // §13.2.6.4.6
    fn after_head<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        use atoms::*;
        match tok {
            Tok::Text(s) => {
                let (ws, rest) = split_leading_ws(s);
                if !ws.is_empty() {
                    self.insert_text(ws);
                }
                if rest.is_empty() {
                    return None;
                }
                self.after_head_anything_else(Tok::Text(rest))
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(ref tag) if tag.name == HTML => self.in_body(tok),
            Tok::Start(tag) if tag.name == BODY => {
                self.insert_html(&tag);
                self.frameset_ok = false;
                self.mode = Mode::InBody;
                None
            }
            Tok::Start(tag) if tag.name == FRAMESET => {
                self.insert_html(&tag);
                self.mode = Mode::InFrameset;
                None
            }
            Tok::Start(ref tag)
                if [BASE, BASEFONT, BGSOUND, LINK, META, NOFRAMES, SCRIPT, STYLE, TEMPLATE, TITLE]
                    .contains(&tag.name) =>
            {
                // Élément de <head> trouvé après </head> : on le range quand même dans <head>.
                let head = self.head.expect("head");
                self.open.push(head);
                let result = self.in_head(tok);
                if let Some(pos) = self.open.iter().rposition(|&n| n == head) {
                    self.open.remove(pos);
                }
                result
            }
            Tok::End(ref tag) if tag.name == TEMPLATE => self.in_head(tok),
            Tok::Start(ref tag) if tag.name == HEAD => None,
            Tok::End(ref tag) if ![BODY, HTML, BR].contains(&tag.name) => None,
            tok => self.after_head_anything_else(tok),
        }
    }

    fn after_head_anything_else<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        let tag = TagToken { name: atoms::BODY, attrs: Vec::new(), self_closing: false };
        self.insert_html(&tag);
        self.mode = Mode::InBody;
        Some(tok)
    }

    // §13.2.6.4.7 : le gros morceau.
    fn in_body<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Text(s) => {
                let text: Cow<str> = if s.contains('\0') { Cow::Owned(s.replace('\0', "")) } else { Cow::Borrowed(s) };
                if text.is_empty() {
                    return None;
                }
                self.reconstruct_formatting();
                self.insert_text(&text);
                if !text.chars().all(is_ws) {
                    self.frameset_ok = false;
                }
                None
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(tag) => self.in_body_start_tag(tag),
            Tok::End(tag) => self.in_body_end_tag(tag),
            Tok::Eof => None, // fin du document : on arrête.
        }
    }

    fn in_body_start_tag<'t>(&mut self, tag: TagToken) -> Option<Tok<'t>> {
        use atoms::*;
        let name = tag.name;
        match name {
            HTML => {
                if !self.template_on_stack() {
                    let html = self.open[0];
                    self.merge_attributes(html, &tag);
                }
            }
            BASE | BASEFONT | BGSOUND | LINK | META | NOFRAMES | SCRIPT | STYLE | TEMPLATE | TITLE => {
                return self.in_head(Tok::Start(tag));
            }
            BODY => {
                if self.open.len() > 1 && self.is_html(self.open[1], BODY) && !self.template_on_stack() {
                    self.frameset_ok = false;
                    let body = self.open[1];
                    self.merge_attributes(body, &tag);
                }
            }
            FRAMESET => {
                if self.open.len() > 1 && self.is_html(self.open[1], BODY) && self.frameset_ok {
                    let body = self.open[1];
                    self.doc.detach(body);
                    self.open.truncate(1);
                    self.insert_html(&tag);
                    self.mode = Mode::InFrameset;
                }
            }
            ADDRESS | ARTICLE | ASIDE | BLOCKQUOTE | CENTER | DETAILS | DIALOG | DIR | DIV | DL
            | FIELDSET | FIGCAPTION | FIGURE | FOOTER | HEADER | HGROUP | MAIN | MENU | NAV | OL | P
            | SEARCH | SECTION | SUMMARY | UL => {
                self.close_p_if_in_button_scope();
                self.insert_html(&tag);
            }
            H1 | H2 | H3 | H4 | H5 | H6 => {
                self.close_p_if_in_button_scope();
                if self.is_html_any(self.current(), HEADINGS) {
                    self.open.pop();
                }
                self.insert_html(&tag);
            }
            PRE | LISTING => {
                self.close_p_if_in_button_scope();
                self.insert_html(&tag);
                self.ignore_lf = true;
                self.frameset_ok = false;
            }
            FORM => {
                if self.form.is_some() && !self.template_on_stack() {
                    return None;
                }
                self.close_p_if_in_button_scope();
                let node = self.insert_html(&tag);
                if !self.template_on_stack() {
                    self.form = Some(node);
                }
            }
            LI | DD | DT => {
                self.frameset_ok = false;
                // Un <li> ferme le <li> précédent (idem <dd>/<dt>).
                let closes: &[Atom] = if name == LI { &[LI] } else { &[DD, DT] };
                for i in (0..self.open.len()).rev() {
                    let node = self.open[i];
                    if self.is_html_any(node, closes) {
                        let node_name = self.doc.element(node).unwrap().name;
                        self.generate_implied_end_tags(Some(node_name));
                        self.pop_until(node_name);
                        break;
                    }
                    if self.is_special(node) && !self.is_html_any(node, &[ADDRESS, DIV, P]) {
                        break;
                    }
                }
                self.close_p_if_in_button_scope();
                self.insert_html(&tag);
            }
            PLAINTEXT => {
                self.close_p_if_in_button_scope();
                self.insert_html(&tag);
                self.tokenizer_state = Some(InitialState::Plaintext);
            }
            BUTTON => {
                if self.in_scope(BUTTON, Scope::Default) {
                    self.generate_implied_end_tags(None);
                    self.pop_until(BUTTON);
                }
                self.reconstruct_formatting();
                self.insert_html(&tag);
                self.frameset_ok = false;
            }
            A => {
                // Un <a> ouvert ne peut pas contenir un autre <a>.
                let start = self.last_marker_index().map_or(0, |i| i + 1);
                let existing = self.formatting[start..].iter().find_map(|f| match f {
                    Formatting::Element(n, t) if t.name == A => Some(*n),
                    _ => None,
                });
                if let Some(old) = existing {
                    self.adoption_agency(A);
                    self.remove_from_formatting(old);
                    self.open.retain(|&n| n != old);
                }
                self.reconstruct_formatting();
                let node = self.insert_html(&tag);
                self.push_formatting(node, &tag);
            }
            B | BIG | CODE | EM | FONT | I | S | SMALL | STRIKE | STRONG | TT | U => {
                self.reconstruct_formatting();
                let node = self.insert_html(&tag);
                self.push_formatting(node, &tag);
            }
            NOBR => {
                self.reconstruct_formatting();
                if self.in_scope(NOBR, Scope::Default) {
                    self.adoption_agency(NOBR);
                    self.reconstruct_formatting();
                }
                let node = self.insert_html(&tag);
                self.push_formatting(node, &tag);
            }
            APPLET | MARQUEE | OBJECT => {
                self.reconstruct_formatting();
                self.insert_html(&tag);
                self.formatting.push(Formatting::Marker);
                self.frameset_ok = false;
            }
            AREA | BR | EMBED | IMG | KEYGEN | WBR => {
                self.reconstruct_formatting();
                self.insert_void(&tag);
                self.frameset_ok = false;
            }
            INPUT => {
                self.reconstruct_formatting();
                self.insert_void(&tag);
                let hidden = tag
                    .attrs
                    .iter()
                    .any(|a| a.name == "type" && a.value.eq_ignore_ascii_case("hidden"));
                if !hidden {
                    self.frameset_ok = false;
                }
            }
            PARAM | SOURCE | TRACK => self.insert_void(&tag),
            HR => {
                self.close_p_if_in_button_scope();
                self.insert_void(&tag);
                self.frameset_ok = false;
            }
            IMAGE => {
                // "<image>" est une vieille faute de frappe pour "<img>".
                return Some(Tok::Start(TagToken { name: IMG, ..tag }));
            }
            TEXTAREA => {
                self.insert_html(&tag);
                self.ignore_lf = true;
                self.tokenizer_state = Some(InitialState::Rcdata);
                self.original_mode = self.mode;
                self.frameset_ok = false;
                self.mode = Mode::Text;
            }
            XMP => {
                self.close_p_if_in_button_scope();
                self.reconstruct_formatting();
                self.frameset_ok = false;
                self.parse_raw_text(&tag, InitialState::Rawtext);
            }
            IFRAME => {
                self.frameset_ok = false;
                self.parse_raw_text(&tag, InitialState::Rawtext);
            }
            NOEMBED => self.parse_raw_text(&tag, InitialState::Rawtext),
            NOSCRIPT if self.scripting => self.parse_raw_text(&tag, InitialState::Rawtext),
            SELECT => {
                self.reconstruct_formatting();
                self.insert_html(&tag);
                self.frameset_ok = false;
            }
            OPTGROUP | OPTION => {
                if self.is_html(self.current(), OPTION) {
                    self.open.pop();
                }
                self.reconstruct_formatting();
                self.insert_html(&tag);
            }
            RB | RTC => {
                if self.in_scope(RUBY, Scope::Default) {
                    self.generate_implied_end_tags(None);
                }
                self.insert_html(&tag);
            }
            RP | RT => {
                if self.in_scope(RUBY, Scope::Default) {
                    self.generate_implied_end_tags(Some(RTC));
                }
                self.insert_html(&tag);
            }
            MATH | SVG => {
                // SVG/MathML complets : étape suivante.
                self.reconstruct_formatting();
                let ns = if name == MATH { Namespace::MathMl } else { Namespace::Svg };
                self.insert_element(&tag, ns);
                if tag.self_closing {
                    self.open.pop();
                }
            }
            CAPTION | COL | COLGROUP | FRAME | HEAD | TBODY | TD | TFOOT | TH | THEAD | TR => {}
            _ => {
                self.reconstruct_formatting();
                self.insert_html(&tag);
            }
        }
        None
    }

    fn in_body_end_tag<'t>(&mut self, tag: TagToken) -> Option<Tok<'t>> {
        use atoms::*;
        let name = tag.name;
        match name {
            TEMPLATE => return self.in_head(Tok::End(tag)),
            BODY => {
                if self.in_scope(BODY, Scope::Default) {
                    self.mode = Mode::AfterBody;
                }
            }
            HTML => {
                if self.in_scope(BODY, Scope::Default) {
                    self.mode = Mode::AfterBody;
                    return Some(Tok::End(tag));
                }
            }
            ADDRESS | ARTICLE | ASIDE | BLOCKQUOTE | BUTTON | CENTER | DETAILS | DIALOG | DIR | DIV
            | DL | FIELDSET | FIGCAPTION | FIGURE | FOOTER | HEADER | HGROUP | LISTING | MAIN | MENU
            | NAV | OL | PRE | SEARCH | SECTION | SUMMARY | UL => {
                if self.in_scope(name, Scope::Default) {
                    self.generate_implied_end_tags(None);
                    self.pop_until(name);
                }
            }
            FORM => {
                if self.template_on_stack() {
                    if self.in_scope(FORM, Scope::Default) {
                        self.generate_implied_end_tags(None);
                        self.pop_until(FORM);
                    }
                } else {
                    let node = self.form.take();
                    if let Some(node) = node.filter(|&n| self.node_in_scope(n)) {
                        self.generate_implied_end_tags(None);
                        self.open.retain(|&n| n != node);
                    }
                }
            }
            P => {
                if !self.in_scope(P, Scope::Button) {
                    // </p> sans <p> ouvert : on crée un <p> vide.
                    self.insert_html(&TagToken { name: P, attrs: Vec::new(), self_closing: false });
                }
                self.close_p();
            }
            LI => {
                if self.in_scope(LI, Scope::ListItem) {
                    self.generate_implied_end_tags(Some(LI));
                    self.pop_until(LI);
                }
            }
            DD | DT => {
                if self.in_scope(name, Scope::Default) {
                    self.generate_implied_end_tags(Some(name));
                    self.pop_until(name);
                }
            }
            H1 | H2 | H3 | H4 | H5 | H6 => {
                if self.in_scope_any(HEADINGS, Scope::Default) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_any(HEADINGS);
                }
            }
            A | B | BIG | CODE | EM | FONT | I | NOBR | S | SMALL | STRIKE | STRONG | TT | U => {
                self.adoption_agency(name);
            }
            APPLET | MARQUEE | OBJECT => {
                if self.in_scope(name, Scope::Default) {
                    self.generate_implied_end_tags(None);
                    self.pop_until(name);
                    self.clear_formatting_to_last_marker();
                }
            }
            BR => {
                // </br> est traité comme <br>.
                return self.in_body_start_tag(TagToken { name: BR, attrs: Vec::new(), self_closing: false });
            }
            _ => self.any_other_end_tag(name),
        }
        None
    }

    /// "Any other end tag" (§13.2.6.4.7) : ferme l'élément du même nom le plus
    /// proche, sauf si un élément "special" se trouve entre les deux.
    fn any_other_end_tag(&mut self, name: Atom) {
        for i in (0..self.open.len()).rev() {
            let node = self.open[i];
            if self.is_html(node, name) {
                self.generate_implied_end_tags(Some(name));
                self.open.truncate(i);
                return;
            }
            if self.is_special(node) {
                return;
            }
        }
    }

    fn formatting_index(&self, node: NodeId) -> Option<usize> {
        self.formatting.iter().position(|f| matches!(f, Formatting::Element(n, _) if *n == node))
    }

    /// L'"adoption agency algorithm" (§13.2.6.4.7) gère le formatage mal imbriqué.
    ///
    /// Exemple : `<b>1<p>2</b>3</p>`. Quand arrive `</b>`, le <p> (le "furthest
    /// block") est encore ouvert DANS le <b>. L'algorithme sort le <p> du <b>, et
    /// crée un nouveau <b> à l'intérieur du <p> pour que "2" reste en gras :
    /// `<b>1</b><p><b>2</b>3</p>`.
    fn adoption_agency(&mut self, subject: Atom) {
        // Étape 2 : cas simple, l'élément courant est celui qu'on ferme.
        let current = self.current();
        if self.is_html(current, subject) && self.formatting_index(current).is_none() {
            self.open.pop();
            return;
        }

        // Étapes 3-4 : boucle externe, au plus 8 tours.
        for _ in 0..8 {
            // 4.3 : l'élément de formatage (le dernier de ce nom depuis le dernier marqueur).
            let start = self.last_marker_index().map_or(0, |i| i + 1);
            let found = (start..self.formatting.len()).rev().find_map(|i| match &self.formatting[i] {
                Formatting::Element(n, t) if t.name == subject => Some((i, *n, t.clone())),
                _ => None,
            });
            let Some((fe_index, formatting_element, fe_tag)) = found else {
                return self.any_other_end_tag(subject);
            };

            // 4.4 : plus dans la pile -> on l'oublie.
            let Some(fe_stack) = self.open.iter().position(|&n| n == formatting_element) else {
                self.formatting.remove(fe_index);
                return;
            };
            // 4.5 : pas dans la portée -> on ignore la balise.
            if !self.node_in_scope(formatting_element) {
                return;
            }

            // 4.7 : le "furthest block", premier élément special AU-DESSUS de lui dans la pile.
            let Some(fb_stack) = (fe_stack + 1..self.open.len()).find(|&i| self.is_special(self.open[i])) else {
                // 4.8 : pas de furthest block : on ferme simplement jusqu'à l'élément.
                self.open.truncate(fe_stack);
                self.formatting.remove(fe_index);
                return;
            };
            let furthest_block = self.open[fb_stack];

            // 4.9 à 4.12
            let common_ancestor = self.open[fe_stack - 1];
            let mut bookmark = fe_index;
            let mut node_stack = fb_stack;
            let mut last_node = furthest_block;

            // 4.13 : boucle interne. On remonte la pile du furthest block vers
            // l'élément de formatage, en recréant les éléments de formatage croisés.
            let mut inner = 0;
            loop {
                inner += 1;
                node_stack -= 1;
                let node = self.open[node_stack];
                if node == formatting_element {
                    break;
                }
                let mut node_fmt = self.formatting_index(node);
                if inner > 3 {
                    if let Some(i) = node_fmt {
                        self.formatting.remove(i);
                        if i < bookmark {
                            bookmark -= 1;
                        }
                        node_fmt = None;
                    }
                }
                let Some(i) = node_fmt else {
                    self.open.remove(node_stack);
                    continue;
                };
                let Formatting::Element(_, tag) = self.formatting[i].clone() else { unreachable!() };
                let new_node = self.create_element(&tag, Namespace::Html);
                self.formatting[i] = Formatting::Element(new_node, tag);
                self.open[node_stack] = new_node;
                if last_node == furthest_block {
                    bookmark = i + 1;
                }
                self.doc.append(new_node, last_node);
                last_node = new_node;
            }

            // 4.14 : on accroche la chaîne reconstruite sous l'ancêtre commun.
            let (parent, before) = self.insertion_place(Some(common_ancestor));
            self.doc.insert_before(parent, last_node, before);

            // 4.15 à 4.17 : un nouvel élément de formatage prend les enfants du furthest block.
            let new_fe = self.create_element(&fe_tag, Namespace::Html);
            self.doc.reparent_children(furthest_block, new_fe);
            self.doc.append(furthest_block, new_fe);

            // 4.18 : il remplace l'ancien dans la liste, à l'emplacement du marque-page.
            let old = self.formatting_index(formatting_element).unwrap();
            self.formatting.remove(old);
            if old < bookmark {
                bookmark -= 1;
            }
            self.formatting.insert(bookmark, Formatting::Element(new_fe, fe_tag));

            // 4.19 : et dans la pile, juste au-dessus du furthest block.
            self.open.retain(|&n| n != formatting_element);
            let fb = self.open.iter().position(|&n| n == furthest_block).unwrap();
            self.open.insert(fb + 1, new_fe);
        }
    }

    // §13.2.6.4.8
    fn text<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Text(s) => {
                self.insert_text(s);
                None
            }
            Tok::Eof => {
                self.open.pop();
                self.mode = self.original_mode;
                Some(Tok::Eof)
            }
            Tok::End(_) => {
                self.open.pop();
                self.mode = self.original_mode;
                None
            }
            _ => None,
        }
    }

    // §13.2.6.4.19
    fn after_body<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Text(s) => {
                let (ws, rest) = split_leading_ws(s);
                if !ws.is_empty() {
                    self.in_body(Tok::Text(ws));
                }
                if rest.is_empty() {
                    return None;
                }
                self.mode = Mode::InBody;
                Some(Tok::Text(rest))
            }
            Tok::Comment(data) => {
                let html = self.open[0];
                self.insert_comment(data, Some(html));
                None
            }
            Tok::Doctype(_) => None,
            Tok::Start(ref tag) if tag.name == atoms::HTML => self.in_body(tok),
            Tok::End(ref tag) if tag.name == atoms::HTML => {
                self.mode = Mode::AfterAfterBody;
                None
            }
            Tok::Eof => None,
            tok => {
                self.mode = Mode::InBody;
                Some(tok)
            }
        }
    }

    // §13.2.6.4.20
    fn in_frameset<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        use atoms::*;
        match tok {
            Tok::Text(s) => {
                // Seuls les espaces sont gardés, le reste est ignoré.
                let ws: String = s.chars().filter(|&c| is_ws(c)).collect();
                if !ws.is_empty() {
                    self.insert_text(&ws);
                }
                None
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Start(ref tag) if tag.name == HTML => self.in_body(tok),
            Tok::Start(tag) if tag.name == FRAMESET => {
                self.insert_html(&tag);
                None
            }
            Tok::End(tag) if tag.name == FRAMESET => {
                if self.open.len() > 1 {
                    self.open.pop();
                    if !self.is_html(self.current(), FRAMESET) {
                        self.mode = Mode::AfterFrameset;
                    }
                }
                None
            }
            Tok::Start(tag) if tag.name == FRAME => {
                self.insert_void(&tag);
                None
            }
            Tok::Start(ref tag) if tag.name == NOFRAMES => self.in_head(tok),
            _ => None,
        }
    }

    // §13.2.6.4.21
    fn after_frameset<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        use atoms::*;
        match tok {
            Tok::Text(s) => {
                let ws: String = s.chars().filter(|&c| is_ws(c)).collect();
                if !ws.is_empty() {
                    self.insert_text(&ws);
                }
                None
            }
            Tok::Comment(data) => {
                self.insert_comment(data, None);
                None
            }
            Tok::Start(ref tag) if tag.name == HTML => self.in_body(tok),
            Tok::End(ref tag) if tag.name == HTML => {
                self.mode = Mode::AfterAfterFrameset;
                None
            }
            Tok::Start(ref tag) if tag.name == NOFRAMES => self.in_head(tok),
            _ => None,
        }
    }

    // §13.2.6.4.22
    fn after_after_body<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Comment(data) => {
                self.insert_comment(data, Some(NodeId::DOCUMENT));
                None
            }
            Tok::Doctype(_) => self.in_body(tok),
            Tok::Start(ref tag) if tag.name == atoms::HTML => self.in_body(tok),
            Tok::Text(s) => {
                let (ws, rest) = split_leading_ws(s);
                if !ws.is_empty() {
                    self.in_body(Tok::Text(ws));
                }
                if rest.is_empty() {
                    return None;
                }
                self.mode = Mode::InBody;
                Some(Tok::Text(rest))
            }
            Tok::Eof => None,
            tok => {
                self.mode = Mode::InBody;
                Some(tok)
            }
        }
    }

    // §13.2.6.4.23
    fn after_after_frameset<'t>(&mut self, tok: Tok<'t>) -> Option<Tok<'t>> {
        match tok {
            Tok::Comment(data) => {
                self.insert_comment(data, Some(NodeId::DOCUMENT));
                None
            }
            Tok::Doctype(_) => self.in_body(tok),
            Tok::Start(ref tag) if tag.name == atoms::HTML => self.in_body(tok),
            Tok::Text(s) => {
                let ws: String = s.chars().filter(|&c| is_ws(c)).collect();
                if !ws.is_empty() {
                    self.in_body(Tok::Text(&ws));
                }
                None
            }
            Tok::Start(ref tag) if tag.name == atoms::NOFRAMES => self.in_head(tok),
            _ => None,
        }
    }
}

/// Décide du mode de rendu d'après le DOCTYPE (§13.2.6.4.1).
fn quirks_mode_for(d: &Doctype) -> QuirksMode {
    const QUIRKY_PREFIXES: &[&str] = &[
        "+//silmaril//dtd html pro v0r11 19970101//",
        "-//as//dtd html 3.0 aswedit + extensions//",
        "-//advasoft ltd//dtd html 3.0 aswedit + extensions//",
        "-//ietf//dtd html 2.0 level 1//",
        "-//ietf//dtd html 2.0 level 2//",
        "-//ietf//dtd html 2.0 strict level 1//",
        "-//ietf//dtd html 2.0 strict level 2//",
        "-//ietf//dtd html 2.0 strict//",
        "-//ietf//dtd html 2.0//",
        "-//ietf//dtd html 2.1e//",
        "-//ietf//dtd html 3.0//",
        "-//ietf//dtd html 3.2 final//",
        "-//ietf//dtd html 3.2//",
        "-//ietf//dtd html 3//",
        "-//ietf//dtd html level 0//",
        "-//ietf//dtd html level 1//",
        "-//ietf//dtd html level 2//",
        "-//ietf//dtd html level 3//",
        "-//ietf//dtd html strict level 0//",
        "-//ietf//dtd html strict level 1//",
        "-//ietf//dtd html strict level 2//",
        "-//ietf//dtd html strict level 3//",
        "-//ietf//dtd html strict//",
        "-//ietf//dtd html//",
        "-//metrius//dtd metrius presentational//",
        "-//microsoft//dtd internet explorer 2.0 html strict//",
        "-//microsoft//dtd internet explorer 2.0 html//",
        "-//microsoft//dtd internet explorer 2.0 tables//",
        "-//microsoft//dtd internet explorer 3.0 html strict//",
        "-//microsoft//dtd internet explorer 3.0 html//",
        "-//microsoft//dtd internet explorer 3.0 tables//",
        "-//netscape comm. corp.//dtd html//",
        "-//netscape comm. corp.//dtd strict html//",
        "-//o'reilly and associates//dtd html 2.0//",
        "-//o'reilly and associates//dtd html extended 1.0//",
        "-//o'reilly and associates//dtd html extended relaxed 1.0//",
        "-//sq//dtd html 2.0 hotmetal + extensions//",
        "-//softquad software//dtd hotmetal pro 6.0::19990601::extensions to html 4.0//",
        "-//softquad//dtd hotmetal pro 4.0::19971010::extensions to html 4.0//",
        "-//spyglass//dtd html 2.0 extended//",
        "-//sun microsystems corp.//dtd hotjava html//",
        "-//sun microsystems corp.//dtd hotjava strict html//",
        "-//w3c//dtd html 3 1995-03-24//",
        "-//w3c//dtd html 3.2 draft//",
        "-//w3c//dtd html 3.2 final//",
        "-//w3c//dtd html 3.2//",
        "-//w3c//dtd html 3.2s draft//",
        "-//w3c//dtd html 4.0 frameset//",
        "-//w3c//dtd html 4.0 transitional//",
        "-//w3c//dtd html experimental 19960712//",
        "-//w3c//dtd html experimental 970421//",
        "-//w3c//dtd w3 html//",
        "-//w3o//dtd w3 html 3.0//",
        "-//webtechs//dtd mozilla html 2.0//",
        "-//webtechs//dtd mozilla html//",
    ];
    let public = d.public_id.as_deref().map(str::to_ascii_lowercase);
    let system = d.system_id.as_deref().map(str::to_ascii_lowercase);
    let pub_starts = |prefix: &str| public.as_deref().is_some_and(|p| p.starts_with(prefix));

    if d.force_quirks
        || d.name.as_deref() != Some("html")
        || matches!(
            public.as_deref(),
            Some("-//w3o//dtd w3 html strict 3.0//en//" | "-/w3c/dtd html 4.0 transitional/en" | "html")
        )
        || system.as_deref() == Some("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
        || QUIRKY_PREFIXES.iter().any(|p| pub_starts(p))
        || (system.is_none()
            && (pub_starts("-//w3c//dtd html 4.01 frameset//")
                || pub_starts("-//w3c//dtd html 4.01 transitional//")))
    {
        QuirksMode::Quirks
    } else if pub_starts("-//w3c//dtd xhtml 1.0 frameset//")
        || pub_starts("-//w3c//dtd xhtml 1.0 transitional//")
        || (system.is_some()
            && (pub_starts("-//w3c//dtd html 4.01 frameset//")
                || pub_starts("-//w3c//dtd html 4.01 transitional//")))
    {
        QuirksMode::LimitedQuirks
    } else {
        QuirksMode::NoQuirks
    }
}
