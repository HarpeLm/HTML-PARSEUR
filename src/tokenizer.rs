use std::collections::VecDeque;

use crate::token::{Attribute, Doctype, Tag, Token};

/// Les états de la machine (spec §13.2.5.x). On en ajoutera à chaque étape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
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
}

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    state: State,
    current_tag: Tag,
    current_tag_is_end: bool,
    current_attr: Option<Attribute>,
    current_comment: String,
    current_doctype: Doctype,
    pending: VecDeque<Token>,
    done: bool,
}

impl Tokenizer {
    pub fn new(input: &str) -> Self {
        // Prétraitement du flux d'entrée (§13.2.3.5) : CRLF et CR deviennent LF.
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        Self {
            input: normalized.chars().collect(),
            pos: 0,
            state: State::Data,
            current_tag: Tag::default(),
            current_tag_is_end: false,
            current_attr: None,
            current_comment: String::new(),
            current_doctype: Doctype::default(),
            pending: VecDeque::new(),
            done: false,
        }
    }

    /// Lit le prochain caractère. `None` = fin de fichier (EOF).
    fn consume(&mut self) -> Option<char> {
        let c = self.input.get(self.pos).copied();
        self.pos += 1;
        c
    }

    /// "Reconsume" de la spec : on relira le même caractère dans le nouvel état.
    fn reconsume(&mut self) {
        self.pos -= 1;
    }

    /// Regarde (sans consommer) si l'entrée continue par `s`, casse ASCII ignorée.
    fn next_is_ignore_case(&self, s: &str) -> bool {
        let mut rest = self.input[self.pos.min(self.input.len())..].iter();
        s.chars().all(|expected| rest.next().is_some_and(|c| c.eq_ignore_ascii_case(&expected)))
    }

    /// Pareil, mais en respectant la casse.
    fn next_is(&self, s: &str) -> bool {
        let mut rest = self.input[self.pos.min(self.input.len())..].iter();
        s.chars().all(|expected| rest.next() == Some(&expected))
    }

    fn emit(&mut self, token: Token) {
        if matches!(token, Token::Eof) {
            self.done = true;
        }
        self.pending.push_back(token);
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
            self.emit(Token::StartTag(tag));
        }
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

    /// Exécute UNE transition de la machine à états.
    fn step(&mut self) {
        match self.state {
            // §13.2.5.1
            State::Data => match self.consume() {
                Some('&') => self.emit(Token::Character('&')), // TODO étape 4 : références de caractères
                Some('<') => self.state = State::TagOpen,
                Some(c) => self.emit(Token::Character(c)),
                None => self.emit(Token::Eof),
            },

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
                    self.emit(Token::Character('<'));
                    self.emit(Token::Eof);
                }
                Some(_) => {
                    self.emit(Token::Character('<'));
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
                    self.emit(Token::Character('<'));
                    self.emit(Token::Character('/'));
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
                Some('&') => self.push_attr_value('&'), // TODO étape 4
                Some('\0') => self.push_attr_value('\u{FFFD}'),
                Some(c) => self.push_attr_value(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.37
            State::AttributeValueSingleQuoted => match self.consume() {
                Some('\'') => self.state = State::AfterAttributeValueQuoted,
                Some('&') => self.push_attr_value('&'), // TODO étape 4
                Some('\0') => self.push_attr_value('\u{FFFD}'),
                Some(c) => self.push_attr_value(c),
                None => self.emit(Token::Eof),
            },

            // §13.2.5.38
            State::AttributeValueUnquoted => match self.consume() {
                Some('\t' | '\n' | '\x0C' | ' ') => self.state = State::BeforeAttributeName,
                Some('&') => self.push_attr_value('&'), // TODO étape 4
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
                } else {
                    // "[CDATA[" n'est valide qu'en SVG/MathML (décidé par le parser) :
                    // en HTML c'est un commentaire bogus, comme tout le reste.
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
        }
    }
}

impl Iterator for Tokenizer {
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
