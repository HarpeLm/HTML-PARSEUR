use std::collections::VecDeque;

use crate::char_ref::{longest_named_match, numeric_reference_char};
use crate::token::{Attribute, Doctype, Tag, Token};

/// Les états dans lesquels le parser peut placer le tokenizer.
/// Par exemple, après `<title>` il passe en RCDATA, après `<script>` en ScriptData.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialState {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    CdataSection,
}

/// Les états de la machine (spec §13.2.5.x). On en ajoutera à chaque étape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThanSign,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    DecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

pub struct Tokenizer<'a> {
    /// L'entrée est empruntée telle quelle : aucune copie.
    input: &'a str,
    /// Position en OCTETS dans `input`.
    pos: usize,
    /// Taille en octets du dernier caractère lu (pour `reconsume`).
    last_len: usize,
    state: State,
    current_tag: Tag,
    current_tag_is_end: bool,
    current_attr: Option<Attribute>,
    current_comment: String,
    current_doctype: Doctype,
    /// État où revenir après une référence de caractère (Data ou valeur d'attribut).
    return_state: State,
    temp_buffer: String,
    char_ref_code: u32,
    /// Nom de la dernière balise ouvrante émise : sert à reconnaître la balise
    /// fermante "appropriée" (le `</script>` qui ferme vraiment le `<script>`).
    last_start_tag: Option<String>,
    /// `<![CDATA[` n'est une vraie section CDATA que dans du SVG/MathML.
    /// C'est le parser qui le sait et qui l'active.
    cdata_allowed: bool,
    pending: VecDeque<Token>,
    /// Texte accumulé, émis en un seul token avant le prochain token non-texte.
    pending_text: String,
    done: bool,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            last_len: 0,
            state: State::Data,
            current_tag: Tag::default(),
            current_tag_is_end: false,
            current_attr: None,
            current_comment: String::new(),
            current_doctype: Doctype::default(),
            return_state: State::Data,
            temp_buffer: String::new(),
            char_ref_code: 0,
            last_start_tag: None,
            cdata_allowed: false,
            pending: VecDeque::new(),
            pending_text: String::new(),
            done: false,
        }
    }

    /// Change l'état courant (utilisé par le parser et par les tests).
    pub fn set_state(&mut self, state: InitialState) {
        self.state = match state {
            InitialState::Data => State::Data,
            InitialState::Rcdata => State::Rcdata,
            InitialState::Rawtext => State::Rawtext,
            InitialState::ScriptData => State::ScriptData,
            InitialState::Plaintext => State::Plaintext,
            InitialState::CdataSection => State::CdataSection,
        };
    }

    pub fn set_last_start_tag(&mut self, name: &str) {
        self.last_start_tag = Some(name.to_string());
    }

    pub fn set_cdata_allowed(&mut self, allowed: bool) {
        self.cdata_allowed = allowed;
    }

    /// Lit le prochain caractère. `None` = fin de fichier (EOF).
    ///
    /// Fait aussi le prétraitement du flux d'entrée (§13.2.3.5) à la volée :
    /// CRLF et CR isolé deviennent LF, sans recopier toute la page.
    #[inline]
    fn consume(&mut self) -> Option<char> {
        let bytes = self.input.as_bytes();
        let Some(&b) = bytes.get(self.pos) else {
            self.pos += 1;
            self.last_len = 1;
            return None;
        };
        if b < 0x80 {
            // Cas rapide : caractère ASCII sur un seul octet (l'immense majorité du HTML).
            self.pos += 1;
            self.last_len = 1;
            if b == b'\r' {
                if bytes.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                    self.last_len = 2;
                }
                return Some('\n');
            }
            return Some(b as char);
        }
        // Caractère UTF-8 sur plusieurs octets (é, €, emoji...).
        let c = self.input[self.pos..].chars().next().unwrap();
        self.last_len = c.len_utf8();
        self.pos += self.last_len;
        Some(c)
    }

    /// "Reconsume" de la spec : on relira le même caractère dans le nouvel état.
    #[inline]
    fn reconsume(&mut self) {
        self.pos -= self.last_len;
    }

    /// Le reste de l'entrée, à partir de la position courante.
    fn rest(&self) -> &'a [u8] {
        &self.input.as_bytes()[self.pos.min(self.input.len())..]
    }

    /// Nombre d'octets avant le premier octet de `stops` (ou jusqu'à la fin).
    /// Les octets d'arrêt sont ASCII : couper là tombe toujours entre deux caractères.
    #[inline]
    fn plain_text_len(&self, stops: &[u8]) -> usize {
        let rest = self.rest();
        rest.iter().position(|b| stops.contains(b)).unwrap_or(rest.len())
    }

    /// Regarde (sans consommer) si l'entrée continue par `s`, casse ASCII ignorée.
    fn next_is_ignore_case(&self, s: &str) -> bool {
        let rest = self.rest();
        rest.len() >= s.len() && rest[..s.len()].eq_ignore_ascii_case(s.as_bytes())
    }

    /// Pareil, mais en respectant la casse.
    fn next_is(&self, s: &str) -> bool {
        self.rest().starts_with(s.as_bytes())
    }

    fn emit(&mut self, token: Token) {
        if matches!(token, Token::Eof) {
            self.done = true;
        }
        // Le texte accumulé passe avant le token qu'on émet.
        if !self.pending_text.is_empty() {
            let text = std::mem::take(&mut self.pending_text);
            self.pending.push_back(Token::Characters(text));
        }
        self.pending.push_back(token);
    }

    /// Un caractère de texte : on l'accumule au lieu de créer un token.
    fn emit_char(&mut self, c: char) {
        self.pending_text.push(c);
    }

    fn new_tag(&mut self, is_end: bool) {
        self.current_tag = Tag::default();
        self.current_tag_is_end = is_end;
        self.current_attr = None;
    }

    /// Commence un nouvel attribut (et range le précédent dans la balise).
    fn start_attribute(&mut self) {
        self.finish_attribute();
        self.current_attr = Some(Attribute::default());
    }

    /// Range l'attribut courant dans la balise, sauf si ce nom existe déjà :
    /// la spec dit de garder le premier et d'ignorer les doublons.
    fn finish_attribute(&mut self) {
        if let Some(attr) = self.current_attr.take() {
            let duplicate = self.current_tag.attributes.iter().any(|a| a.name == attr.name);
            if !duplicate {
                self.current_tag.attributes.push(attr);
            }
        }
    }

    fn push_attr_name(&mut self, c: char) {
        if let Some(attr) = &mut self.current_attr {
            attr.name.push(c);
        }
    }

    fn push_attr_value(&mut self, c: char) {
        if let Some(attr) = &mut self.current_attr {
            attr.value.push(c);
        }
    }

    fn emit_current_tag(&mut self) {
        self.finish_attribute();
        let tag = std::mem::take(&mut self.current_tag);
        if self.current_tag_is_end {
            self.emit(Token::EndTag(tag));
        } else {
            self.last_start_tag = Some(tag.name.clone());
            self.emit(Token::StartTag(tag));
        }
    }

    /// Balise fermante "appropriée" : même nom que la dernière balise ouvrante.
    fn is_appropriate_end_tag(&self) -> bool {
        self.current_tag_is_end && self.last_start_tag.as_deref() == Some(&self.current_tag.name)
    }

    fn emit_str(&mut self, s: &str) {
        self.pending_text.push_str(s);
    }

    /// Pas une balise fermante valide : on rend "</" + les lettres lues comme du texte.
    fn emit_less_than_slash_and_buffer(&mut self) {
        self.emit_str("</");
        let buffer = std::mem::take(&mut self.temp_buffer);
        self.emit_str(&buffer);
    }

    fn emit_comment(&mut self) {
        let data = std::mem::take(&mut self.current_comment);
        self.emit(Token::Comment(data));
    }

    fn new_doctype(&mut self) {
        self.current_doctype = Doctype::default();
    }

    fn emit_doctype(&mut self) {
        let doctype = std::mem::take(&mut self.current_doctype);
        self.emit(Token::Doctype(doctype));
    }

    /// Cas très fréquent dans les états DOCTYPE : EOF -> force-quirks, on émet le
    /// doctype puis EOF.
    fn emit_doctype_eof(&mut self) {
        self.current_doctype.force_quirks = true;
        self.emit_doctype();
        self.emit(Token::Eof);
    }

    /// Cas très fréquent aussi : '>' prématuré -> force-quirks, retour à Data.
    fn emit_doctype_quirks(&mut self) {
        self.current_doctype.force_quirks = true;
        self.state = State::Data;
        self.emit_doctype();
    }

    fn doctype_name_push(&mut self, c: char) {
        self.current_doctype.name.get_or_insert_with(String::new).push(c);
    }

    fn doctype_public_push(&mut self, c: char) {
        self.current_doctype.public_id.get_or_insert_with(String::new).push(c);
    }

    fn doctype_system_push(&mut self, c: char) {
        self.current_doctype.system_id.get_or_insert_with(String::new).push(c);
    }

    /// Démarre une référence de caractère (on vient de lire '&').
    fn start_char_ref(&mut self) {
        self.return_state = self.state;
        self.state = State::CharacterReference;
    }

    /// Vrai si la référence en cours se trouve dans une valeur d'attribut.
    fn in_attribute(&self) -> bool {
        matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }

    /// "Flush code points consumed as a character reference" : le tampon part dans
    /// la valeur d'attribut, ou devient des tokens Character.
    fn flush_temp_buffer(&mut self) {
        let buffer = std::mem::take(&mut self.temp_buffer);
        for c in buffer.chars() {
            if self.in_attribute() {
                self.push_attr_value(c);
            } else {
                self.emit_char(c);
            }
        }
    }

    /// Exécute UNE transition de la machine à états.
    fn step(&mut self) {
        match self.state {
            // §13.2.5.1
            State::Data => {
                // Chemin rapide : tout le texte jusqu'au prochain octet spécial est
                // copié d'un seul coup, sans passer par la machine à états.
                let n = self.plain_text_len(b"<&\r\0");
                if n > 0 {
                    self.pending_text.push_str(&self.input[self.pos..self.pos + n]);
                    self.pos += n;
                    return;
                }
                match self.consume() {
                    Some('&') => self.start_char_ref(),
                    Some('<') => self.state = State::TagOpen,
                    Some(c) => self.emit_char(c),
                    None => self.emit(Token::Eof),
                }
            }

            // §13.2.5.6
            State::TagOpen => match self.consume() {
                Some('!') => self.state = State::MarkupDeclarationOpen,
                Some('/') => self.state = State::EndTagOpen,
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(false);
                    self.reconsume();
                    self.state = State::TagName;
                }
                Some('?') => {
                    self.current_comment.clear();
                    self.reconsume();
                    self.state = State::BogusComment;
                }
                None => {
                    self.emit_char('<');
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.emit_char('<');
                    self.reconsume();
                    self.state = State::Data;
                }
            },

            // §13.2.5.7
            State::EndTagOpen => match self.consume() {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume();
                    self.state = State::TagName;
                }
                Some('>') => self.state = State::Data,
                None => {
                    self.emit_char('<');
                    self.emit_char('/');
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.current_comment.clear();
                    self.reconsume();
                    self.state = State::BogusComment;
                }
            },

            // §13.2.5.8
            State::TagName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                Some(c) if c.is_ascii_uppercase() => {
                    self.current_tag.name.push(c.to_ascii_lowercase())
                }
                Some('\0') => self.current_tag.name.push('\u{FFFD}'),
                Some(c) => self.current_tag.name.push(c),
                None => self.emit(Token::Eof),
            },

            // ───────────── Contenus spéciaux : <title>, <style>, <script>… ─────────────

            // §13.2.5.2 (<title>, <textarea> : le texte garde les &entités;)
            State::Rcdata => match self.consume() {
                Some('&') => self.start_char_ref(),
                Some('<') => self.state = State::RcdataLessThanSign,
                Some('\0') => self.emit_char('\u{FFFD}'),
                Some(c) => self.emit_char(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.3 et §13.2.5.4 (<style>, <script> : texte brut, pas d'entités)
            State::Rawtext | State::ScriptData => match self.consume() {
                Some('<') => {
                    self.state = if self.state == State::Rawtext {
                        State::RawtextLessThanSign
                    } else {
                        State::ScriptDataLessThanSign
                    }
                }
                Some('\0') => self.emit_char('\u{FFFD}'),
                Some(c) => self.emit_char(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.5 (<plaintext> : tout le reste du fichier est du texte)
            State::Plaintext => match self.consume() {
                Some('\0') => self.emit_char('\u{FFFD}'),
                Some(c) => self.emit_char(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.9, §13.2.5.12, §13.2.5.15
            State::RcdataLessThanSign | State::RawtextLessThanSign | State::ScriptDataLessThanSign => {
                let (text_state, end_tag_open) = match self.state {
                    State::RcdataLessThanSign => (State::Rcdata, State::RcdataEndTagOpen),
                    State::RawtextLessThanSign => (State::Rawtext, State::RawtextEndTagOpen),
                    _ => (State::ScriptData, State::ScriptDataEndTagOpen),
                };
                match self.consume() {
                    Some('/') => {
                        self.temp_buffer.clear();
                        self.state = end_tag_open;
                    }
                    Some('!') if text_state == State::ScriptData => {
                        self.emit_str("<!");
                        self.state = State::ScriptDataEscapeStart;
                    }
                    _ => {
                        self.emit_char('<');
                        self.reconsume();
                        self.state = text_state;
                    }
                }
            }

            // §13.2.5.10, §13.2.5.13, §13.2.5.16, §13.2.5.24
            State::RcdataEndTagOpen
            | State::RawtextEndTagOpen
            | State::ScriptDataEndTagOpen
            | State::ScriptDataEscapedEndTagOpen => {
                let (text_state, end_tag_name) = match self.state {
                    State::RcdataEndTagOpen => (State::Rcdata, State::RcdataEndTagName),
                    State::RawtextEndTagOpen => (State::Rawtext, State::RawtextEndTagName),
                    State::ScriptDataEndTagOpen => (State::ScriptData, State::ScriptDataEndTagName),
                    _ => (State::ScriptDataEscaped, State::ScriptDataEscapedEndTagName),
                };
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.new_tag(true);
                        self.reconsume();
                        self.state = end_tag_name;
                    }
                    _ => {
                        self.emit_str("</");
                        self.reconsume();
                        self.state = text_state;
                    }
                }
            }

            // §13.2.5.11, §13.2.5.14, §13.2.5.17, §13.2.5.25
            // On lit "</xxx". Si xxx == dernière balise ouvrante, c'est une vraie
            // balise fermante ; sinon, "</xxx" est simplement du texte.
            State::RcdataEndTagName
            | State::RawtextEndTagName
            | State::ScriptDataEndTagName
            | State::ScriptDataEscapedEndTagName => {
                let text_state = match self.state {
                    State::RcdataEndTagName => State::Rcdata,
                    State::RawtextEndTagName => State::Rawtext,
                    State::ScriptDataEndTagName => State::ScriptData,
                    _ => State::ScriptDataEscaped,
                };
                match self.consume() {
                    Some('\t' | '\n' | '\x0C' | ' ') if self.is_appropriate_end_tag() => {
                        self.state = State::BeforeAttributeName;
                    }
                    Some('/') if self.is_appropriate_end_tag() => {
                        self.state = State::SelfClosingStartTag;
                    }
                    Some('>') if self.is_appropriate_end_tag() => {
                        self.state = State::Data;
                        self.emit_current_tag();
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.current_tag.name.push(c.to_ascii_lowercase());
                        self.temp_buffer.push(c);
                    }
                    _ => {
                        self.emit_less_than_slash_and_buffer();
                        self.reconsume();
                        self.state = text_state;
                    }
                }
            }

            // §13.2.5.18 : "<!-" dans un script
            State::ScriptDataEscapeStart => match self.consume() {
                Some('-') => {
                    self.emit_char('-');
                    self.state = State::ScriptDataEscapeStartDash;
                }
                _ => {
                    self.reconsume();
                    self.state = State::ScriptData;
                }
            },

            // §13.2.5.19 : "<!--" dans un script
            State::ScriptDataEscapeStartDash => match self.consume() {
                Some('-') => {
                    self.emit_char('-');
                    self.state = State::ScriptDataEscapedDashDash;
                }
                _ => {
                    self.reconsume();
                    self.state = State::ScriptData;
                }
            },

            // §13.2.5.20, §13.2.5.21, §13.2.5.22 : dans "<!-- ... -->" d'un script.
            // Les trois états ne diffèrent que par le nombre de '-' déjà vus.
            State::ScriptDataEscaped | State::ScriptDataEscapedDash | State::ScriptDataEscapedDashDash => {
                let dashes = match self.state {
                    State::ScriptDataEscaped => 0,
                    State::ScriptDataEscapedDash => 1,
                    _ => 2,
                };
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = match dashes {
                            0 => State::ScriptDataEscapedDash,
                            _ => State::ScriptDataEscapedDashDash,
                        };
                    }
                    Some('<') => self.state = State::ScriptDataEscapedLessThanSign,
                    Some('>') if dashes == 2 => {
                        self.emit_char('>');
                        self.state = State::ScriptData;
                    }
                    Some('\0') => {
                        self.emit_char('\u{FFFD}');
                        self.state = State::ScriptDataEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = State::ScriptDataEscaped;
                    }
                    None => self.emit(Token::Eof),
                }
            }

            // §13.2.5.23
            State::ScriptDataEscapedLessThanSign => match self.consume() {
                Some('/') => {
                    self.temp_buffer.clear();
                    self.state = State::ScriptDataEscapedEndTagOpen;
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    self.temp_buffer.clear();
                    self.emit_char('<');
                    self.reconsume();
                    self.state = State::ScriptDataDoubleEscapeStart;
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume();
                    self.state = State::ScriptDataEscaped;
                }
            },

            // §13.2.5.26 et §13.2.5.31 : "<script" ou "</script" à l'intérieur d'un
            // "<!--" de script fait entrer / sortir du mode "double échappé".
            State::ScriptDataDoubleEscapeStart | State::ScriptDataDoubleEscapeEnd => {
                let (if_script, otherwise) = if self.state == State::ScriptDataDoubleEscapeStart {
                    (State::ScriptDataDoubleEscaped, State::ScriptDataEscaped)
                } else {
                    (State::ScriptDataEscaped, State::ScriptDataDoubleEscaped)
                };
                match self.consume() {
                    Some(c @ ('\t' | '\n' | '\x0C' | ' ' | '/' | '>')) => {
                        self.state = if self.temp_buffer == "script" { if_script } else { otherwise };
                        self.emit_char(c);
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.temp_buffer.push(c.to_ascii_lowercase());
                        self.emit_char(c);
                    }
                    _ => {
                        self.reconsume();
                        self.state = otherwise;
                    }
                }
            }

            // §13.2.5.27, §13.2.5.28, §13.2.5.29
            State::ScriptDataDoubleEscaped
            | State::ScriptDataDoubleEscapedDash
            | State::ScriptDataDoubleEscapedDashDash => {
                let dashes = match self.state {
                    State::ScriptDataDoubleEscaped => 0,
                    State::ScriptDataDoubleEscapedDash => 1,
                    _ => 2,
                };
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = match dashes {
                            0 => State::ScriptDataDoubleEscapedDash,
                            _ => State::ScriptDataDoubleEscapedDashDash,
                        };
                    }
                    Some('<') => {
                        self.emit_char('<');
                        self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    }
                    Some('>') if dashes == 2 => {
                        self.emit_char('>');
                        self.state = State::ScriptData;
                    }
                    Some('\0') => {
                        self.emit_char('\u{FFFD}');
                        self.state = State::ScriptDataDoubleEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = State::ScriptDataDoubleEscaped;
                    }
                    None => self.emit(Token::Eof),
                }
            }

            // §13.2.5.30
            State::ScriptDataDoubleEscapedLessThanSign => match self.consume() {
                Some('/') => {
                    self.temp_buffer.clear();
                    self.emit_char('/');
                    self.state = State::ScriptDataDoubleEscapeEnd;
                }
                _ => {
                    self.reconsume();
                    self.state = State::ScriptDataDoubleEscaped;
                }
            },

            // §13.2.5.32
            State::BeforeAttributeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('/' | '>') | None => {
                    self.reconsume();
                    self.state = State::AfterAttributeName;
                }
                Some('=') => {
                    self.start_attribute();
                    self.push_attr_name('=');
                    self.state = State::AttributeName;
                }
                Some(_) => {
                    self.start_attribute();
                    self.reconsume();
                    self.state = State::AttributeName;
                }
            },

            // §13.2.5.33
            State::AttributeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ' | '/' | '>') | None => {
                    self.reconsume();
                    self.state = State::AfterAttributeName;
                }
                Some('=') => self.state = State::BeforeAttributeValue,
                Some(c) if c.is_ascii_uppercase() => self.push_attr_name(c.to_ascii_lowercase()),
                Some('\0') => self.push_attr_name('\u{FFFD}'),
                Some(c) => self.push_attr_name(c),
            },

            // §13.2.5.34
            State::AfterAttributeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('=') => self.state = State::BeforeAttributeValue,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                None => self.emit(Token::Eof),
                Some(_) => {
                    self.start_attribute();
                    self.reconsume();
                    self.state = State::AttributeName;
                }
            },

            // §13.2.5.35
            State::BeforeAttributeValue => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('"') => self.state = State::AttributeValueDoubleQuoted,
                Some('\'') => self.state = State::AttributeValueSingleQuoted,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                _ => {
                    self.reconsume();
                    self.state = State::AttributeValueUnquoted;
                }
            },

            // §13.2.5.36
            State::AttributeValueDoubleQuoted => match self.consume() {
                Some('"') => self.state = State::AfterAttributeValueQuoted,
                Some('&') => self.start_char_ref(),
                Some('\0') => self.push_attr_value('\u{FFFD}'),
                Some(c) => self.push_attr_value(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.37
            State::AttributeValueSingleQuoted => match self.consume() {
                Some('\'') => self.state = State::AfterAttributeValueQuoted,
                Some('&') => self.start_char_ref(),
                Some('\0') => self.push_attr_value('\u{FFFD}'),
                Some(c) => self.push_attr_value(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.38
            State::AttributeValueUnquoted => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::BeforeAttributeName,
                Some('&') => self.start_char_ref(),
                Some('>') => {
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                Some('\0') => self.push_attr_value('\u{FFFD}'),
                Some(c) => self.push_attr_value(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.39
            State::AfterAttributeValueQuoted => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                None => self.emit(Token::Eof),
                Some(_) => {
                    self.reconsume();
                    self.state = State::BeforeAttributeName;
                }
            },

            // §13.2.5.40
            State::SelfClosingStartTag => match self.consume() {
                Some('>') => {
                    self.current_tag.self_closing = true;
                    self.state = State::Data;
                    self.emit_current_tag();
                }
                None => self.emit(Token::Eof),
                Some(_) => {
                    self.reconsume();
                    self.state = State::BeforeAttributeName;
                }
            },

            // ───────────── Commentaires ─────────────

            // §13.2.5.41
            State::BogusComment => match self.consume() {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
                Some('\0') => self.current_comment.push('\u{FFFD}'),
                Some(c) => self.current_comment.push(c),
            },

            // §13.2.5.42 : on vient de lire "<!", on regarde ce qui suit.
            State::MarkupDeclarationOpen => {
                self.current_comment.clear();
                if self.next_is("--") {
                    self.pos += 2;
                    self.state = State::CommentStart;
                } else if self.next_is_ignore_case("DOCTYPE") {
                    self.pos += 7;
                    self.state = State::Doctype;
                } else if self.cdata_allowed && self.next_is("[CDATA[") {
                    self.pos += 7;
                    self.state = State::CdataSection;
                } else {
                    // En HTML normal, "<![CDATA[" est un commentaire bogus, comme le reste.
                    self.state = State::BogusComment;
                }
            }

            // §13.2.5.43
            State::CommentStart => match self.consume() {
                Some('-') => self.state = State::CommentStartDash,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                _ => {
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.44
            State::CommentStartDash => match self.consume() {
                Some('-') => self.state = State::CommentEnd,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.current_comment.push('-');
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.45
            State::Comment => match self.consume() {
                Some('<') => {
                    self.current_comment.push('<');
                    self.state = State::CommentLessThanSign;
                }
                Some('-') => self.state = State::CommentEndDash,
                Some('\0') => self.current_comment.push('\u{FFFD}'),
                Some(c) => self.current_comment.push(c),
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
            },

            // §13.2.5.46
            State::CommentLessThanSign => match self.consume() {
                Some('!') => {
                    self.current_comment.push('!');
                    self.state = State::CommentLessThanSignBang;
                }
                Some('<') => self.current_comment.push('<'),
                _ => {
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.47
            State::CommentLessThanSignBang => match self.consume() {
                Some('-') => self.state = State::CommentLessThanSignBangDash,
                _ => {
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.48
            State::CommentLessThanSignBangDash => match self.consume() {
                Some('-') => self.state = State::CommentLessThanSignBangDashDash,
                _ => {
                    self.reconsume();
                    self.state = State::CommentEndDash;
                }
            },

            // §13.2.5.49 ("<!--" imbriqué : erreur, mais aucun effet sur le résultat)
            State::CommentLessThanSignBangDashDash => {
                self.consume();
                self.reconsume();
                self.state = State::CommentEnd;
            }

            // §13.2.5.50
            State::CommentEndDash => match self.consume() {
                Some('-') => self.state = State::CommentEnd,
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.current_comment.push('-');
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.51
            State::CommentEnd => match self.consume() {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                Some('!') => self.state = State::CommentEndBang,
                Some('-') => self.current_comment.push('-'),
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.current_comment.push_str("--");
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // §13.2.5.52
            State::CommentEndBang => match self.consume() {
                Some('-') => {
                    self.current_comment.push_str("--!");
                    self.state = State::CommentEndDash;
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.emit_comment();
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.current_comment.push_str("--!");
                    self.reconsume();
                    self.state = State::Comment;
                }
            },

            // ───────────── DOCTYPE ─────────────

            // §13.2.5.53
            State::Doctype => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::BeforeDoctypeName,
                None => {
                    self.new_doctype();
                    self.emit_doctype_eof();
                }
                Some(_) => {
                    self.reconsume();
                    self.state = State::BeforeDoctypeName;
                }
            },

            // §13.2.5.54
            State::BeforeDoctypeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('>') => {
                    self.new_doctype();
                    self.emit_doctype_quirks();
                }
                None => {
                    self.new_doctype();
                    self.emit_doctype_eof();
                }
                Some(c) => {
                    self.new_doctype();
                    let c = match c {
                        '\0' => '\u{FFFD}',
                        c => c.to_ascii_lowercase(),
                    };
                    self.doctype_name_push(c);
                    self.state = State::DoctypeName;
                }
            },

            // §13.2.5.55
            State::DoctypeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::AfterDoctypeName,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some('\0') => self.doctype_name_push('\u{FFFD}'),
                Some(c) => self.doctype_name_push(c.to_ascii_lowercase()),
                None => self.emit_doctype_eof(),
            },

            // §13.2.5.56
            State::AfterDoctypeName => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => self.emit_doctype_eof(),
                Some(_) => {
                    self.reconsume();
                    if self.next_is_ignore_case("PUBLIC") {
                        self.pos += 6;
                        self.state = State::AfterDoctypePublicKeyword;
                    } else if self.next_is_ignore_case("SYSTEM") {
                        self.pos += 6;
                        self.state = State::AfterDoctypeSystemKeyword;
                    } else {
                        self.current_doctype.force_quirks = true;
                        self.state = State::BogusDoctype;
                    }
                }
            },

            // §13.2.5.57 et §13.2.5.58 (presque identiques)
            State::AfterDoctypePublicKeyword | State::BeforeDoctypePublicIdentifier => {
                match self.consume() {
                    Some('\t' | '\n' | '\x0C' | ' ') => {
                        if self.state == State::AfterDoctypePublicKeyword {
                            self.state = State::BeforeDoctypePublicIdentifier;
                        }
                    }
                    Some('"') => {
                        self.current_doctype.public_id = Some(String::new());
                        self.state = State::DoctypePublicIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.current_doctype.public_id = Some(String::new());
                        self.state = State::DoctypePublicIdentifierSingleQuoted;
                    }
                    Some('>') => self.emit_doctype_quirks(),
                    None => self.emit_doctype_eof(),
                    Some(_) => {
                        self.current_doctype.force_quirks = true;
                        self.reconsume();
                        self.state = State::BogusDoctype;
                    }
                }
            }

            // §13.2.5.59 et §13.2.5.60
            State::DoctypePublicIdentifierDoubleQuoted
            | State::DoctypePublicIdentifierSingleQuoted => {
                let quote = if self.state == State::DoctypePublicIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match self.consume() {
                    Some(c) if c == quote => self.state = State::AfterDoctypePublicIdentifier,
                    Some('\0') => self.doctype_public_push('\u{FFFD}'),
                    Some('>') => self.emit_doctype_quirks(),
                    Some(c) => self.doctype_public_push(c),
                    None => self.emit_doctype_eof(),
                }
            }

            // §13.2.5.61 et §13.2.5.62
            State::AfterDoctypePublicIdentifier
            | State::BetweenDoctypePublicAndSystemIdentifiers => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {
                    self.state = State::BetweenDoctypePublicAndSystemIdentifiers
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some('"') => {
                    self.current_doctype.system_id = Some(String::new());
                    self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                }
                Some('\'') => {
                    self.current_doctype.system_id = Some(String::new());
                    self.state = State::DoctypeSystemIdentifierSingleQuoted;
                }
                None => self.emit_doctype_eof(),
                Some(_) => {
                    self.current_doctype.force_quirks = true;
                    self.reconsume();
                    self.state = State::BogusDoctype;
                }
            },

            // §13.2.5.63 et §13.2.5.64
            State::AfterDoctypeSystemKeyword | State::BeforeDoctypeSystemIdentifier => {
                match self.consume() {
                    Some('\t' | '\n' | '\x0C' | ' ') => {
                        if self.state == State::AfterDoctypeSystemKeyword {
                            self.state = State::BeforeDoctypeSystemIdentifier;
                        }
                    }
                    Some('"') => {
                        self.current_doctype.system_id = Some(String::new());
                        self.state = State::DoctypeSystemIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.current_doctype.system_id = Some(String::new());
                        self.state = State::DoctypeSystemIdentifierSingleQuoted;
                    }
                    Some('>') => self.emit_doctype_quirks(),
                    None => self.emit_doctype_eof(),
                    Some(_) => {
                        self.current_doctype.force_quirks = true;
                        self.reconsume();
                        self.state = State::BogusDoctype;
                    }
                }
            }

            // §13.2.5.65 et §13.2.5.66
            State::DoctypeSystemIdentifierDoubleQuoted
            | State::DoctypeSystemIdentifierSingleQuoted => {
                let quote = if self.state == State::DoctypeSystemIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match self.consume() {
                    Some(c) if c == quote => self.state = State::AfterDoctypeSystemIdentifier,
                    Some('\0') => self.doctype_system_push('\u{FFFD}'),
                    Some('>') => self.emit_doctype_quirks(),
                    Some(c) => self.doctype_system_push(c),
                    None => self.emit_doctype_eof(),
                }
            }

            // §13.2.5.67
            State::AfterDoctypeSystemIdentifier => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => {}
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => self.emit_doctype_eof(),
                Some(_) => {
                    // Erreur, mais ici PAS de force-quirks.
                    self.reconsume();
                    self.state = State::BogusDoctype;
                }
            },

            // §13.2.5.68
            State::BogusDoctype => match self.consume() {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => {
                    self.emit_doctype();
                    self.emit(Token::Eof);
                }
                Some(_) => {}
            },

            // ───────────── Sections CDATA (SVG/MathML) ─────────────

            // §13.2.5.69
            State::CdataSection => match self.consume() {
                Some(']') => self.state = State::CdataSectionBracket,
                Some(c) => self.emit_char(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.70
            State::CdataSectionBracket => match self.consume() {
                Some(']') => self.state = State::CdataSectionEnd,
                _ => {
                    self.emit_char(']');
                    self.reconsume();
                    self.state = State::CdataSection;
                }
            },

            // §13.2.5.71
            State::CdataSectionEnd => match self.consume() {
                Some(']') => self.emit_char(']'),
                Some('>') => self.state = State::Data,
                _ => {
                    self.emit_str("]]");
                    self.reconsume();
                    self.state = State::CdataSection;
                }
            },

            // ───────────── Références de caractères ─────────────

            // §13.2.5.72
            State::CharacterReference => {
                self.temp_buffer.clear();
                self.temp_buffer.push('&');
                match self.consume() {
                    Some(c) if c.is_ascii_alphanumeric() => {
                        self.reconsume();
                        self.state = State::NamedCharacterReference;
                    }
                    Some('#') => {
                        self.temp_buffer.push('#');
                        self.state = State::NumericCharacterReference;
                    }
                    _ => {
                        self.flush_temp_buffer();
                        self.reconsume();
                        self.state = self.return_state;
                    }
                }
            }

            // §13.2.5.73
            State::NamedCharacterReference => {
                match longest_named_match(self.rest()) {
                    Some((len, value)) => {
                        // Les noms d'entités sont en ASCII : len octets = len caractères.
                        let matched = &self.input[self.pos..self.pos + len];
                        let ends_with_semicolon = matched.ends_with(';');
                        let next = self.input.as_bytes().get(self.pos + len).map(|&b| b as char);
                        self.temp_buffer.push_str(matched);
                        self.pos += len;

                        // Exception historique : dans un attribut, "?a=1&copy=2" doit
                        // rester tel quel (c'est une URL, pas un ©).
                        let keep_as_is = self.in_attribute()
                            && !ends_with_semicolon
                            && next.is_some_and(|c| c == '=' || c.is_ascii_alphanumeric());
                        if !keep_as_is {
                            self.temp_buffer.clear();
                            self.temp_buffer.push_str(value);
                        }
                        self.flush_temp_buffer();
                        self.state = self.return_state;
                    }
                    None => {
                        self.flush_temp_buffer();
                        self.state = State::AmbiguousAmpersand;
                    }
                }
            }

            // §13.2.5.74
            State::AmbiguousAmpersand => match self.consume() {
                Some(c) if c.is_ascii_alphanumeric() => {
                    if self.in_attribute() {
                        self.push_attr_value(c);
                    } else {
                        self.emit_char(c);
                    }
                }
                _ => {
                    self.reconsume();
                    self.state = self.return_state;
                }
            },

            // §13.2.5.75
            State::NumericCharacterReference => {
                self.char_ref_code = 0;
                match self.consume() {
                    Some(c @ ('x' | 'X')) => {
                        self.temp_buffer.push(c);
                        self.state = State::HexadecimalCharacterReferenceStart;
                    }
                    _ => {
                        self.reconsume();
                        self.state = State::DecimalCharacterReferenceStart;
                    }
                }
            }

            // §13.2.5.76
            State::HexadecimalCharacterReferenceStart => match self.consume() {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.reconsume();
                    self.state = State::HexadecimalCharacterReference;
                }
                _ => {
                    // "&#x" sans chiffre : on rend le texte tel quel.
                    self.flush_temp_buffer();
                    self.reconsume();
                    self.state = self.return_state;
                }
            },

            // §13.2.5.77
            State::DecimalCharacterReferenceStart => match self.consume() {
                Some(c) if c.is_ascii_digit() => {
                    self.reconsume();
                    self.state = State::DecimalCharacterReference;
                }
                _ => {
                    self.flush_temp_buffer();
                    self.reconsume();
                    self.state = self.return_state;
                }
            },

            // §13.2.5.78 et §13.2.5.79
            State::HexadecimalCharacterReference | State::DecimalCharacterReference => {
                let radix = if self.state == State::HexadecimalCharacterReference { 16 } else { 10 };
                match self.consume() {
                    Some(c) if c.is_digit(radix) => {
                        // saturating : "&#99999999999;" ne doit pas faire déborder le u32.
                        self.char_ref_code = self
                            .char_ref_code
                            .saturating_mul(radix)
                            .saturating_add(c.to_digit(radix).unwrap());
                    }
                    Some(';') => self.state = State::NumericCharacterReferenceEnd,
                    _ => {
                        self.reconsume();
                        self.state = State::NumericCharacterReferenceEnd;
                    }
                }
            }

            // §13.2.5.80
            State::NumericCharacterReferenceEnd => {
                self.temp_buffer.clear();
                self.temp_buffer.push(numeric_reference_char(self.char_ref_code));
                self.flush_temp_buffer();
                self.state = self.return_state;
            }
        }
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        while self.pending.is_empty() {
            if self.done {
                return None;
            }
            self.step();
        }
        self.pending.pop_front()
    }
}
