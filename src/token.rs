/// Les 6 types de tokens définis par la spec WHATWG (section 13.2.5).
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Character(char),
    StartTag(Tag),
    EndTag(Tag),
    Comment(String),
    Doctype(Doctype),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag {
    pub name: String,
    pub self_closing: bool,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}