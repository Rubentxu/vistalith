//! LikeC4 round-trip (SPK-008): the C4 lens speaks the DSL that LikeC4
//! renders, but identity never lives in LikeC4. Every exported element
//! carries its `SubjectRef` in a `metadata { vistalith "ns:kind:id" }` entry,
//! so exporting, hand-editing and re-importing the same model is an
//! identity-preserving no-op — the same guarantee SUBJECT-REF.md demands of
//! every lens.
//!
//! The parser accepts a pragmatic subset of the DSL: `specification`,
//! `model` (nested elements, typed/untyped relationships, `this`, `extend`,
//! metadata, tags, triple-quoted strings) and skips `views`, `deployment`,
//! `global` and comments. It is deliberately not a full LikeC4 front-end:
//! anything outside the architecture-model surface the round-trip needs is
//! an explicit error, not a silent guess.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use vistalith_domain::{
    Actor, AuthorityClass, EventPayload, Namespace, Provenance, RelationFact, RelationDeclared,
    RelationKind, RelationRef, SubjectDefined, SubjectDeprecated, SubjectKind, SubjectRef,
    SubjectUpdated, VEvent,
};

use crate::c4::{C4Element, C4Level, C4Relationship, C4View, c4_view};
use crate::diff::PropertyChange;
use crate::graph::SemanticWorldGraph;
use crate::store::{GraphStore, StoreError};

/// The LikeC4 element kinds the import maps onto architecture subjects.
const SUPPORTED_ELEMENT_KINDS: &[(&str, SubjectKind)] = &[
    ("system", SubjectKind::System),
    ("container", SubjectKind::Container),
    ("component", SubjectKind::Component),
    ("interface", SubjectKind::Interface),
    ("datastore", SubjectKind::DataStore),
    ("deploymentnode", SubjectKind::DeploymentNode),
];

/// Untyped LikeC4 relationships land on this generic structural verb so the
/// import never invents a specific semantic it was not given.
const DEFAULT_RELATIONSHIP_KIND: &str = "uses";

// --- Export -----------------------------------------------------------------

/// Serializes the C4 view of `graph` as a LikeC4 model source. Deterministic:
/// the same graph revision always yields byte-identical output.
pub fn likec4_source(graph: &SemanticWorldGraph) -> String {
    let view = c4_view(graph);
    let mut out = String::new();

    out.push_str("specification {\n");
    out.push_str("  element system\n");
    out.push_str("  element container\n");
    out.push_str("  element component\n");
    let mut kinds = std::collections::BTreeSet::new();
    for relationship in &view.relationships {
        kinds.insert(dsl_ident(&relationship.kind));
    }
    for kind in &kinds {
        out.push_str(&format!("  relationship {kind}\n"));
    }
    if view.all_elements().any(|e| e.deprecated) {
        out.push_str("  tag deprecated\n");
    }
    out.push_str("}\n\n");

    out.push_str("model {\n");
    let mut used_ids = std::collections::HashSet::new();
    let mut element_ids: HashMap<String, String> = HashMap::new();
    for element in view.all_elements() {
        let id = fresh_ident(&dsl_ident(id_of(&element.identity)), &mut used_ids);
        element_ids.insert(element.identity.clone(), id.clone());
        out.push_str(&format!(
            "  {} {} \"{}\" {{\n",
            level_name(level_of(&element.identity, &view)),
            id,
            escape(&element.name)
        ));
        if let Some(description) = &element.description {
            out.push_str(&format!("    description '{}'\n", escape(description)));
        }
        if element.deprecated {
            out.push_str("    #deprecated\n");
        }
        out.push_str("    metadata {\n");
        out.push_str(&format!(
            "      vistalith '{}'\n",
            escape(&element.identity)
        ));
        out.push_str("    }\n");
        out.push_str("  }\n");
    }
    for relationship in &view.relationships {
        let source = &element_ids[&relationship.source];
        let target = &element_ids[&relationship.target];
        out.push_str(&format!(
            "  {} -[{}]-> {}\n",
            source,
            dsl_ident(&relationship.kind),
            target
        ));
    }
    out.push_str("}\n\n");

    out.push_str("views {\n");
    out.push_str("  view overview {\n");
    out.push_str("    title 'Vistalith architecture'\n");
    out.push_str("    include *\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn level_of(identity: &str, view: &C4View) -> C4Level {
    if view.systems.iter().any(|e| e.identity == identity) {
        return C4Level::System;
    }
    if view.containers.iter().any(|e| e.identity == identity) {
        return C4Level::Container;
    }
    C4Level::Component
}

fn level_name(level: C4Level) -> &'static str {
    match level {
        C4Level::System => "system",
        C4Level::Container => "container",
        C4Level::Component => "component",
    }
}

/// Maps an arbitrary string onto a LikeC4 identifier: letters, digits,
/// hyphens, underscores; never empty, never starting with a digit.
fn dsl_ident(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    if out.is_empty() {
        out.push('e');
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 'e');
    }
    out
}

fn fresh_ident(base: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut candidate = base.to_owned();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// `identity` is `ns:kind:id`; the id is everything after the second colon.
fn id_of(identity: &str) -> &str {
    identity.splitn(3, ':').nth(2).unwrap_or(identity)
}

fn escape(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('\'', "\\'")
}

// --- Parsed model -----------------------------------------------------------

/// A LikeC4 model flattened to FQN-addressed elements and resolved
/// relationships — the shape the import needs, independent of nesting.
#[derive(Debug, Clone, PartialEq)]
pub struct LikeC4Model {
    pub elements: Vec<LikeC4Element>,
    pub relationships: Vec<LikeC4Relationship>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LikeC4Element {
    pub fqn: String,
    pub kind: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LikeC4Relationship {
    pub source: String,
    pub target: String,
    pub kind: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LikeC4Error {
    Parse(String),
    UnsupportedElementKind(String),
    UnknownElement(String),
    AmbiguousElement(String),
    InvalidIdentity(String),
    Store(String),
}

impl std::fmt::Display for LikeC4Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LikeC4Error::Parse(message) => write!(f, "likec4 parse error: {message}"),
            LikeC4Error::UnsupportedElementKind(kind) => {
                write!(f, "unsupported likec4 element kind `{kind}`")
            }
            LikeC4Error::UnknownElement(fqn) => write!(f, "unknown element `{fqn}`"),
            LikeC4Error::AmbiguousElement(fqn) => {
                write!(f, "ambiguous element reference `{fqn}`")
            }
            LikeC4Error::InvalidIdentity(raw) => write!(f, "invalid vistalith identity `{raw}`"),
            LikeC4Error::Store(message) => write!(f, "store error during import: {message}"),
        }
    }
}

impl std::error::Error for LikeC4Error {}

// --- Lexer ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Arrow,
    Minus,
    Hash,
    Dot,
    /// Wildcards (`*`, `**`) inside skipped blocks such as `views`.
    Star,
    /// Named assignments (`primary = instanceOf ...`) in skipped blocks.
    Eq,
}

fn lex(source: &str) -> Result<Vec<Tok>, LikeC4Error> {
    let mut toks = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        let rest = &source[i..];
        match ch {
            '/' if rest.starts_with("//") => {
                for (_, c) in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if rest.starts_with("/*") => {
                loop {
                    match chars.next() {
                        None => {
                            return Err(LikeC4Error::Parse(
                                "unterminated block comment".into(),
                            ));
                        }
                        Some((j, _)) => {
                            if source[j..].starts_with("*/") {
                                chars.next(); // consume '/'
                                break;
                            }
                        }
                    }
                }
            }
            ' ' | '\t' | '\n' | '\r' => {}
            '{' => toks.push(Tok::LBrace),
            '}' => toks.push(Tok::RBrace),
            '[' => toks.push(Tok::LBracket),
            ']' => toks.push(Tok::RBracket),
            '#' => toks.push(Tok::Hash),
            '.' => toks.push(Tok::Dot),
            '*' => toks.push(Tok::Star),
            '=' => toks.push(Tok::Eq),
            '-' if rest.starts_with("->") => {
                toks.push(Tok::Arrow);
                let _ = chars.next(); // '>'
            }
            '-' => toks.push(Tok::Minus),
            '\'' | '"' => {
                let quote = ch;
                let triple = rest.starts_with(&quote.to_string().repeat(3));
                if triple {
                    chars.next();
                    chars.next();
                }
                let mut value = String::new();
                loop {
                    match chars.next() {
                        None => return Err(LikeC4Error::Parse("unterminated string".into())),
                        Some((j, c)) => {
                            if triple && source[j..].starts_with(&quote.to_string().repeat(3)) {
                                chars.next();
                                chars.next();
                                break;
                            }
                            if !triple && c == quote {
                                break;
                            }
                            if c == '\\' {
                                match chars.next() {
                                    Some((_, 'n')) => value.push('\n'),
                                    Some((_, 't')) => value.push('\t'),
                                    Some((_, other)) => value.push(other),
                                    None => {
                                        return Err(LikeC4Error::Parse(
                                            "unterminated escape".into(),
                                        ));
                                    }
                                }
                            } else {
                                value.push(c);
                            }
                        }
                    }
                }
                toks.push(Tok::Str(value));
            }
            c if c.is_ascii_alphanumeric() || c == '_' => {
                let start = i;
                let mut end = i + c.len_utf8();
                while let Some((j, c)) = chars.peek() {
                    if c.is_ascii_alphanumeric() || *c == '-' || *c == '_' {
                        end = j + c.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push(Tok::Ident(source[start..end].to_owned()));
            }
            other => {
                return Err(LikeC4Error::Parse(format!("unexpected character `{other}`")));
            }
        }
    }
    Ok(toks)
}

// --- Parser -----------------------------------------------------------------

#[derive(Debug, Clone)]
struct PendingRelationship {
    source: String,
    target: String,
    kind: Option<String>,
    title: Option<String>,
    scope: Vec<String>,
}

#[derive(Debug, Clone)]
struct PendingExtend {
    target: String,
    name: Option<String>,
    description: Option<String>,
    metadata: BTreeMap<String, String>,
    tags: Vec<String>,
    scope: Vec<String>,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    element_kinds: Vec<String>,
    elements: Vec<LikeC4Element>,
    /// Relationships still expressed as in-source references plus the scope
    /// they were written in; resolved once every element is known.
    pending: Vec<PendingRelationship>,
    extends: Vec<PendingExtend>,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let tok = self.toks.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }
    fn expect(&mut self, tok: &Tok) -> Result<(), LikeC4Error> {
        match self.next() {
            Some(got) if &got == tok => Ok(()),
            Some(got) => Err(LikeC4Error::Parse(format!("expected {tok:?}, found {got:?}"))),
            None => Err(LikeC4Error::Parse(format!(
                "expected {tok:?}, found end of input"
            ))),
        }
    }
    fn expect_ident(&mut self) -> Result<String, LikeC4Error> {
        match self.next() {
            Some(Tok::Ident(id)) => Ok(id),
            Some(got) => {
                Err(LikeC4Error::Parse(format!("expected identifier, found {got:?}")))
            }
            None => Err(LikeC4Error::Parse(
                "expected identifier, found end of input".into(),
            )),
        }
    }
    fn eat_str(&mut self) -> Result<Option<String>, LikeC4Error> {
        match self.next() {
            Some(Tok::Str(value)) => Ok(Some(value)),
            Some(_) => {
                self.pos -= 1;
                Ok(None)
            }
            None => Ok(None),
        }
    }
    fn skip_balanced(&mut self) -> Result<(), LikeC4Error> {
        let mut depth = 0usize;
        loop {
            match self.next() {
                Some(Tok::LBrace) => depth += 1,
                Some(Tok::RBrace) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(_) => {}
                None => return Err(LikeC4Error::Parse("unbalanced braces".into())),
            }
        }
    }

    fn is_element_kind(&self, id: &str) -> bool {
        self.element_kinds.iter().any(|k| k == id)
    }

    fn parse_document(mut self) -> Result<LikeC4Model, LikeC4Error> {
        loop {
            match self.next() {
                None => break,
                Some(Tok::Ident(block)) => match block.as_str() {
                    "import" => {
                        if self.peek() == Some(&Tok::LBrace) {
                            self.skip_balanced()?;
                        }
                        if self.peek() == Some(&Tok::Ident("from".to_owned())) {
                            let _ = self.next();
                            let _ = self.eat_str()?;
                        }
                    }
                    "specification" => self.parse_specification()?,
                    "model" => {
                        self.expect(&Tok::LBrace)?;
                        self.parse_model_body(Vec::new())?;
                    }
                    "views" | "deployment" | "global" => {
                        let _ = self.eat_str()?;
                        if self.peek() == Some(&Tok::LBrace) {
                            self.skip_balanced()?;
                        }
                    }
                    other => {
                        return Err(LikeC4Error::Parse(format!(
                            "unexpected top-level block `{other}`"
                        )));
                    }
                },
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "expected top-level block, found {got:?}"
                    )));
                }
            }
        }

        // `extend` merges first, in document order, so relationships resolve
        // against the fully merged element set.
        let extends = std::mem::take(&mut self.extends);
        for ext in extends {
            let fqn = self.resolve(&ext.target, &ext.scope)?;
            let Some(element) = self.elements.iter_mut().find(|e| e.fqn == fqn) else {
                return Err(LikeC4Error::UnknownElement(fqn));
            };
            if ext.name.is_some() {
                element.name = ext.name.clone();
            }
            if ext.description.is_some() {
                element.description = ext.description.clone();
            }
            for (key, value) in ext.metadata {
                element.metadata.insert(key, value);
            }
            for tag in ext.tags {
                if !element.tags.contains(&tag) {
                    element.tags.push(tag);
                }
            }
        }
        let mut relationships = Vec::new();
        for pending in &self.pending {
            let source = self.resolve(&pending.source, &pending.scope)?;
            let target = self.resolve(&pending.target, &pending.scope)?;
            relationships.push(LikeC4Relationship {
                source,
                target,
                kind: pending.kind.clone(),
                title: pending.title.clone(),
            });
        }
        Ok(LikeC4Model {
            elements: std::mem::take(&mut self.elements),
            relationships,
        })
    }

    fn parse_specification(&mut self) -> Result<(), LikeC4Error> {
        self.expect(&Tok::LBrace)?;
        loop {
            match self.peek() {
                Some(Tok::RBrace) => {
                    let _ = self.next();
                    return Ok(());
                }
                Some(Tok::Ident(word)) => {
                    let word = word.clone();
                    let _ = self.next();
                    match word.as_str() {
                        "element" => {
                            let name = self.expect_ident()?;
                            if !self.is_element_kind(&name) {
                                self.element_kinds.push(name);
                            }
                        }
                        "relationship" | "tag" => {
                            let _ = self.expect_ident()?;
                        }
                        "color" => {
                            let _ = self.expect_ident()?;
                            if self.peek() == Some(&Tok::Hash) {
                                let _ = self.next();
                                let _ = self.expect_ident()?;
                            }
                        }
                        other => {
                            return Err(LikeC4Error::Parse(format!(
                                "unexpected specification entry `{other}`"
                            )));
                        }
                    }
                }
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "unexpected token in specification: {got:?}"
                    )));
                }
                None => return Err(LikeC4Error::Parse("unterminated specification".into())),
            }
        }
    }

    fn parse_model_body(&mut self, scope: Vec<String>) -> Result<(), LikeC4Error> {
        loop {
            match self.peek().cloned() {
                Some(Tok::RBrace) => {
                    let _ = self.next();
                    return Ok(());
                }
                Some(Tok::Ident(word)) => {
                    if word == "extend" {
                        let _ = self.next();
                        let target = self.parse_ref()?;
                        let name = self.eat_str()?;
                        let mut ext = PendingExtend {
                            target,
                            name,
                            description: None,
                            metadata: BTreeMap::new(),
                            tags: Vec::new(),
                            scope: scope.clone(),
                        };
                        if self.peek() == Some(&Tok::LBrace) {
                            let _ = self.next();
                            self.parse_extend_body(&mut ext)?;
                        }
                        self.extends.push(ext);
                    } else if self.is_element_kind(&word)
                        && matches!(self.toks.get(self.pos + 1), Some(Tok::Ident(_)))
                    {
                        let _ = self.next();
                        self.parse_element_definition(&word, &scope)?;
                    } else {
                        self.parse_relationship(scope.clone())?;
                    }
                }
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "unexpected token in model: {got:?}"
                    )));
                }
                None => return Err(LikeC4Error::Parse("unterminated model block".into())),
            }
        }
    }

    fn parse_element_definition(
        &mut self,
        kind: &str,
        scope: &[String],
    ) -> Result<(), LikeC4Error> {
        let local = self.parse_ref()?;
        let name = self.eat_str()?;
        let fqn = if scope.is_empty() {
            local.clone()
        } else {
            format!("{}.{}", scope.join("."), local)
        };
        if self.peek() == Some(&Tok::LBrace) {
            let _ = self.next();
            let mut inner = scope.to_vec();
            inner.push(local);
            self.parse_element_body(kind, &fqn, name, &inner)
        } else {
            self.elements.push(LikeC4Element {
                fqn,
                kind: kind.to_owned(),
                name,
                description: None,
                metadata: BTreeMap::new(),
                tags: Vec::new(),
            });
            Ok(())
        }
    }

    fn parse_element_body(
        &mut self,
        kind: &str,
        fqn: &str,
        name: Option<String>,
        inner_scope: &[String],
    ) -> Result<(), LikeC4Error> {
        let mut element = LikeC4Element {
            fqn: fqn.to_owned(),
            kind: kind.to_owned(),
            name,
            description: None,
            metadata: BTreeMap::new(),
            tags: Vec::new(),
        };
        loop {
            match self.peek().cloned() {
                Some(Tok::RBrace) => {
                    let _ = self.next();
                    break;
                }
                Some(Tok::Hash) => {
                    let _ = self.next();
                    element.tags.push(self.expect_ident()?);
                }
                Some(Tok::Ident(word)) => {
                    match word.as_str() {
                        "title" => {
                            let _ = self.next();
                            element.name = self.eat_str()?;
                        }
                        "description" | "summary" => {
                            let _ = self.next();
                            element.description = self.eat_str()?;
                        }
                        "technology" | "link" | "icon" | "notation" => {
                            let _ = self.next();
                            if let Some(value) = self.eat_str()? {
                                element.metadata.insert(word.clone(), value);
                            }
                        }
                        "metadata" => {
                            let _ = self.next();
                            self.expect(&Tok::LBrace)?;
                            self.parse_metadata(&mut element.metadata)?;
                        }
                        "tag" => {
                            let _ = self.next();
                            self.expect(&Tok::Hash)?;
                            element.tags.push(self.expect_ident()?);
                        }
                        "style" => {
                            let _ = self.next();
                            if self.peek() == Some(&Tok::LBrace) {
                                let _ = self.next();
                                self.skip_balanced()?;
                            } else {
                                let _ = self.expect_ident()?;
                            }
                        }
                        "navigateTo" => {
                            let _ = self.next();
                            let _ = self.expect_ident()?;
                        }
                        _ if self.is_element_kind(&word)
                            && matches!(self.toks.get(self.pos + 1), Some(Tok::Ident(_))) =>
                        {
                            let _ = self.next();
                            self.parse_element_definition(&word, inner_scope)?;
                        }
                        _ => {
                            // the source reference is still unconsumed here
                            let _ = self.next();
                            self.parse_relationship_from(word, fqn, inner_scope)?;
                        }
                    }
                }
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "unexpected token in element body: {got:?}"
                    )));
                }
                None => return Err(LikeC4Error::Parse("unterminated element body".into())),
            }
        }
        self.elements.push(element);
        Ok(())
    }

    fn parse_metadata(&mut self, into: &mut BTreeMap<String, String>) -> Result<(), LikeC4Error> {
        loop {
            match self.peek() {
                Some(Tok::RBrace) => {
                    let _ = self.next();
                    return Ok(());
                }
                Some(Tok::Ident(_)) => {
                    let key = self.expect_ident()?;
                    let value = self.eat_str()?.ok_or_else(|| {
                        LikeC4Error::Parse(format!("metadata `{key}` needs a string value"))
                    })?;
                    into.insert(key, value);
                }
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "unexpected token in metadata: {got:?}"
                    )));
                }
                None => return Err(LikeC4Error::Parse("unterminated metadata".into())),
            }
        }
    }

    fn parse_extend_body(&mut self, ext: &mut PendingExtend) -> Result<(), LikeC4Error> {
        loop {
            match self.peek().cloned() {
                Some(Tok::RBrace) => {
                    let _ = self.next();
                    return Ok(());
                }
                Some(Tok::Hash) => {
                    let _ = self.next();
                    ext.tags.push(self.expect_ident()?);
                }
                Some(Tok::Ident(word)) => match word.as_str() {
                    "title" => {
                        let _ = self.next();
                        ext.name = self.eat_str()?;
                    }
                    "description" | "summary" => {
                        let _ = self.next();
                        ext.description = self.eat_str()?;
                    }
                    "metadata" => {
                        let _ = self.next();
                        self.expect(&Tok::LBrace)?;
                        self.parse_metadata(&mut ext.metadata)?;
                    }
                    "tag" => {
                        let _ = self.next();
                        self.expect(&Tok::Hash)?;
                        ext.tags.push(self.expect_ident()?);
                    }
                    "style" => {
                        let _ = self.next();
                        if self.peek() == Some(&Tok::LBrace) {
                            let _ = self.next();
                            self.skip_balanced()?;
                        }
                    }
                    other => {
                        return Err(LikeC4Error::Parse(format!(
                            "unsupported extend entry `{other}`"
                        )));
                    }
                },
                Some(got) => {
                    return Err(LikeC4Error::Parse(format!(
                        "unexpected token in extend body: {got:?}"
                    )));
                }
                None => return Err(LikeC4Error::Parse("unterminated extend body".into())),
            }
        }
    }

    fn parse_relationship(&mut self, scope: Vec<String>) -> Result<(), LikeC4Error> {
        let source = self.parse_ref()?;
        self.parse_relationship_from(source, "", &scope)
    }

    /// Parses `-[kind]-> target "title"` after the source reference was
    /// consumed (`source`/`this`/`it`); `default_source` resolves `this`.
    fn parse_relationship_from(
        &mut self,
        source: String,
        default_source: &str,
        scope: &[String],
    ) -> Result<(), LikeC4Error> {
        let source = match source.as_str() {
            "this" | "it" => default_source.to_owned(),
            _ => source,
        };
        let kind = if self.peek() == Some(&Tok::Minus) {
            let _ = self.next();
            self.expect(&Tok::LBracket)?;
            let kind = self.expect_ident()?;
            self.expect(&Tok::RBracket)?;
            Some(kind)
        } else {
            None
        };
        self.expect(&Tok::Arrow)?;
        let target = self.parse_ref()?;
        let title = self.eat_str()?;
        self.pending.push(PendingRelationship {
            source,
            target,
            kind,
            title,
            scope: scope.to_vec(),
        });
        Ok(())
    }

    /// A dotted reference path: `a.b.c` or a single `this`/`it`.
    fn parse_ref(&mut self) -> Result<String, LikeC4Error> {
        let mut path = self.expect_ident()?;
        while self.peek() == Some(&Tok::Dot) {
            let _ = self.next();
            path.push('.');
            path.push_str(&self.expect_ident()?);
        }
        Ok(path)
    }

    /// Lexical scoping first (longest scope prefix wins), then a unique FQN
    /// suffix match, mirroring how LikeC4 resolves references.
    fn resolve(&self, reference: &str, scope: &[String]) -> Result<String, LikeC4Error> {
        for prefix_len in (0..=scope.len()).rev() {
            let candidate = if prefix_len == 0 {
                reference.to_owned()
            } else {
                format!("{}.{}", scope[..prefix_len].join("."), reference)
            };
            if self.elements.iter().any(|e| e.fqn == candidate) {
                return Ok(candidate);
            }
        }
        let suffixes: Vec<&LikeC4Element> = self
            .elements
            .iter()
            .filter(|e| e.fqn == reference || e.fqn.ends_with(&format!(".{reference}")))
            .collect();
        match suffixes.len() {
            1 => Ok(suffixes[0].fqn.clone()),
            0 => Err(LikeC4Error::UnknownElement(reference.to_owned())),
            _ => Err(LikeC4Error::AmbiguousElement(reference.to_owned())),
        }
    }
}

/// Parses the supported LikeC4 DSL subset into a flattened model.
pub fn parse_likec4(source: &str) -> Result<LikeC4Model, LikeC4Error> {
    let parser = Parser {
        toks: lex(source)?,
        pos: 0,
        element_kinds: SUPPORTED_ELEMENT_KINDS
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect(),
        elements: Vec::new(),
        pending: Vec::new(),
        extends: Vec::new(),
    };
    parser.parse_document()
}

// --- Import -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct LikeC4ImportReport {
    pub defined_subjects: Vec<SubjectRef>,
    pub updated_subjects: Vec<SubjectRef>,
    pub unchanged_subjects: Vec<SubjectRef>,
    pub deprecated_subjects: Vec<SubjectRef>,
    pub declared_relations: Vec<RelationRef>,
    pub skipped_relations: Vec<RelationRef>,
}

/// Imports a parsed LikeC4 model into the store as durable events.
///
/// Identity: an element whose `metadata.vistalith` parses to a `SubjectRef`
/// reuses that identity (round-trip); everything else gets a fresh `arch`
/// subject whose id is the LikeC4 FQN. Already-present subjects are updated
/// only when a property actually differs, so re-importing an exported model
/// appends nothing and the graph diff stays empty.
pub fn import_likec4(
    store: &mut GraphStore,
    model: &LikeC4Model,
    actor: &Actor,
) -> Result<LikeC4ImportReport, LikeC4Error> {
    let mut report = LikeC4ImportReport::default();
    let mut provenance = Provenance::new(actor.as_str())
        .map_err(|e| LikeC4Error::Store(e.to_string()))?;
    provenance.note = Some("likec4 model import".to_owned());

    // Elements first: relationships validate against existing endpoints.
    let mut identities: Vec<(String, SubjectRef)> = Vec::new();
    for element in &model.elements {
        let kind_name = normalize_kind(&element.kind);
        let subject_kind = SUPPORTED_ELEMENT_KINDS
            .iter()
            .find(|(name, _)| *name == kind_name)
            .map(|(_, kind)| kind.clone())
            .ok_or_else(|| LikeC4Error::UnsupportedElementKind(element.kind.clone()))?;
        let subject = match element.metadata.get("vistalith") {
            Some(raw) => Some(
                SubjectRef::parse(raw).map_err(|_| LikeC4Error::InvalidIdentity(raw.clone()))?,
            ),
            None => None,
        };
        let subject = subject.unwrap_or_else(|| {
            SubjectRef::new(Namespace::Arch, subject_kind, element.fqn.clone())
                .expect("fqn without ':' or '@' is a valid subject id")
        });

        let leaf = element.fqn.rsplit('.').next().unwrap_or(&element.fqn);
        let mut properties = BTreeMap::from([(
            "name".to_owned(),
            serde_json::json!(element.name.clone().unwrap_or_else(|| leaf.to_owned())),
        )]);
        if let Some(description) = &element.description {
            properties.insert("description".to_owned(), serde_json::json!(description));
        }
        // `#deprecated` is the one tag with SWG meaning: it maps onto the
        // first-class deprecation fact. Everything else is bookkeeping.
        let wants_deprecated = element.tags.iter().any(|tag| tag == "deprecated");
        let other_tags: Vec<String> = element
            .tags
            .iter()
            .filter(|tag| tag.as_str() != "deprecated")
            .cloned()
            .collect();
        if !other_tags.is_empty() {
            properties.insert("likec4_tags".to_owned(), serde_json::json!(other_tags));
        }
        for (key, value) in &element.metadata {
            if key != "vistalith" {
                properties.insert(format!("likec4_{key}"), serde_json::json!(value));
            }
        }

        match store.graph().subject(&subject) {
            None => {
                let event = import_event(
                    actor,
                    EventPayload::SubjectDefined(SubjectDefined {
                        subject: subject.clone(),
                        authority: AuthorityClass::Authoritative,
                        provenance: provenance.clone(),
                        properties: properties.clone(),
                    }),
                    vec![subject.clone()],
                );
                store.append(event).map_err(store_error)?;
                report.defined_subjects.push(subject.clone());
            }
            Some(node) => {
                let changed: BTreeMap<String, serde_json::Value> = properties
                    .iter()
                    .filter(|(key, value)| node.properties.get(key.as_str()) != Some(value))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                if changed.is_empty() {
                    report.unchanged_subjects.push(subject.clone());
                } else {
                    let event = import_event(
                        actor,
                        EventPayload::SubjectUpdated(SubjectUpdated {
                            subject: subject.clone(),
                            properties: changed,
                        }),
                        vec![subject.clone()],
                    );
                    store.append(event).map_err(store_error)?;
                    report.updated_subjects.push(subject.clone());
                }
            }
        }
        if wants_deprecated && !store.graph().subject(&subject).is_some_and(|n| n.deprecated) {
            let event = import_event(
                actor,
                EventPayload::SubjectDeprecated(SubjectDeprecated {
                    subject: subject.clone(),
                    reason: Some("likec4 #deprecated tag".to_owned()),
                }),
                vec![subject.clone()],
            );
            store.append(event).map_err(store_error)?;
            report.deprecated_subjects.push(subject.clone());
        }
        identities.push((element.fqn.clone(), subject));
    }

    for relationship in &model.relationships {
        let resolve = |fqn: &str| -> Result<SubjectRef, LikeC4Error> {
            identities
                .iter()
                .find(|(element_fqn, _)| element_fqn == fqn)
                .map(|(_, subject)| subject.clone())
                .ok_or_else(|| LikeC4Error::UnknownElement(fqn.to_owned()))
        };
        let from = resolve(&relationship.source)?;
        let to = resolve(&relationship.target)?;
        let kind_name = relationship
            .kind
            .as_deref()
            .map(normalize_relation_kind)
            .unwrap_or_else(|| DEFAULT_RELATIONSHIP_KIND.to_owned());
        let kind =
            RelationKind::parse(&kind_name).map_err(|e| LikeC4Error::Store(e.to_string()))?;
        let relation =
            RelationRef::new(from, kind, to).map_err(|e| LikeC4Error::Store(e.to_string()))?;
        let fact = RelationFact {
            relation: relation.clone(),
            authority: AuthorityClass::Authoritative,
            provenance: provenance.clone(),
        };
        // Imports are additive: a relation that already exists under the same
        // identity (endpoints + kind) is left untouched, provenance included.
        if store.graph().relation(&fact.relation).is_some() {
            report.skipped_relations.push(relation);
            continue;
        }
        let event = import_event(
            actor,
            EventPayload::RelationDeclared(RelationDeclared {
                fact: fact.clone(),
            }),
            vec![relation.from.clone(), relation.to.clone()],
        );
        store.append(event).map_err(store_error)?;
        report.declared_relations.push(relation);
    }

    Ok(report)
}

fn normalize_kind(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn normalize_relation_kind(raw: &str) -> String {
    if raw.chars().all(|c| c == '_' || c.is_ascii_lowercase()) {
        return raw.to_owned();
    }
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_uppercase() {
            out.push('_');
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    out
}

fn store_error(err: StoreError) -> LikeC4Error {
    LikeC4Error::Store(err.to_string())
}

fn import_event(actor: &Actor, payload: EventPayload, subjects: Vec<SubjectRef>) -> VEvent {
    VEvent {
        event_id: uuid::Uuid::now_v7(),
        actor: actor.clone(),
        timestamp: time::OffsetDateTime::now_utc(),
        subjects,
        correlation_id: uuid::Uuid::now_v7(),
        causation_id: None,
        trace_id: None,
        payload,
    }
}

// --- Architecture revision diff ----------------------------------------------

/// Architecture-flavored diff (SPK-008): what changed in the C4 projection
/// between two graph states. Element-level changes are property-level detail
/// on stable SubjectRef identities, so a rename shows up as a `name` change —
/// never as remove+add.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4ElementChange {
    pub identity: String,
    pub level: C4Level,
    pub changes: Vec<PropertyChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4RelationshipChange {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub changes: Vec<PropertyChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C4Diff {
    pub from_revision: u64,
    pub to_revision: u64,
    pub added_elements: Vec<C4Element>,
    pub removed_elements: Vec<C4Element>,
    pub changed_elements: Vec<C4ElementChange>,
    pub added_relationships: Vec<C4Relationship>,
    pub removed_relationships: Vec<C4Relationship>,
    pub changed_relationships: Vec<C4RelationshipChange>,
}

impl C4Diff {
    pub fn is_empty(&self) -> bool {
        self.added_elements.is_empty()
            && self.removed_elements.is_empty()
            && self.changed_elements.is_empty()
            && self.added_relationships.is_empty()
            && self.removed_relationships.is_empty()
            && self.changed_relationships.is_empty()
    }
}

fn relationship_key(r: &C4Relationship) -> String {
    format!("{}|{}|{}", r.source, r.kind, r.target)
}

pub fn c4_diff(from: &SemanticWorldGraph, to: &SemanticWorldGraph) -> C4Diff {
    let from_view = c4_view(from);
    let to_view = c4_view(to);
    let from_elements: HashMap<String, &C4Element> = from_view
        .all_elements()
        .map(|element| (element.identity.clone(), element))
        .collect();
    let to_elements: HashMap<String, &C4Element> = to_view
        .all_elements()
        .map(|element| (element.identity.clone(), element))
        .collect();

    let mut added_elements = Vec::new();
    let mut changed_elements = Vec::new();
    for element in to_view.all_elements() {
        match from_elements.get(&element.identity) {
            None => added_elements.push(element.clone()),
            Some(previous) => {
                let changes = element_changes(previous, element);
                if !changes.is_empty() {
                    changed_elements.push(C4ElementChange {
                        identity: element.identity.clone(),
                        level: level_of(&element.identity, &to_view),
                        changes,
                    });
                }
            }
        }
    }
    let removed_elements: Vec<C4Element> = from_view
        .all_elements()
        .filter(|element| !to_elements.contains_key(&element.identity))
        .cloned()
        .collect();

    let from_rels: HashMap<String, &C4Relationship> = from_view
        .relationships
        .iter()
        .map(|r| (relationship_key(r), r))
        .collect();
    let to_rels: HashMap<String, &C4Relationship> = to_view
        .relationships
        .iter()
        .map(|r| (relationship_key(r), r))
        .collect();
    let added_relationships: Vec<C4Relationship> = to_view
        .relationships
        .iter()
        .filter(|r| !from_rels.contains_key(&relationship_key(r)))
        .cloned()
        .collect();
    let removed_relationships: Vec<C4Relationship> = from_view
        .relationships
        .iter()
        .filter(|r| !to_rels.contains_key(&relationship_key(r)))
        .cloned()
        .collect();
    let changed_relationships = to_view
        .relationships
        .iter()
        .filter_map(|r| {
            let previous = from_rels.get(&relationship_key(r))?;
            if previous.authority == r.authority {
                return None;
            }
            Some(C4RelationshipChange {
                source: r.source.clone(),
                target: r.target.clone(),
                kind: r.kind.clone(),
                changes: vec![PropertyChange {
                    key: "authority".to_owned(),
                    from: Some(serde_json::json!(previous.authority)),
                    to: Some(serde_json::json!(r.authority)),
                }],
            })
        })
        .collect();

    C4Diff {
        from_revision: from.revision(),
        to_revision: to.revision(),
        added_elements,
        removed_elements,
        changed_elements,
        added_relationships,
        removed_relationships,
        changed_relationships,
    }
}

fn element_changes(previous: &C4Element, current: &C4Element) -> Vec<PropertyChange> {
    let mut changes = Vec::new();
    if previous.name != current.name {
        changes.push(PropertyChange {
            key: "name".to_owned(),
            from: Some(serde_json::json!(previous.name)),
            to: Some(serde_json::json!(current.name)),
        });
    }
    if previous.description != current.description {
        changes.push(PropertyChange {
            key: "description".to_owned(),
            from: previous.description.as_ref().map(|v| serde_json::json!(v)),
            to: current.description.as_ref().map(|v| serde_json::json!(v)),
        });
    }
    if previous.deprecated != current.deprecated {
        changes.push(PropertyChange {
            key: "deprecated".to_owned(),
            from: Some(serde_json::json!(previous.deprecated)),
            to: Some(serde_json::json!(current.deprecated)),
        });
    }
    if previous.authority != current.authority {
        changes.push(PropertyChange {
            key: "authority".to_owned(),
            from: Some(serde_json::json!(previous.authority)),
            to: Some(serde_json::json!(current.authority)),
        });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use vistalith_domain::Provenance;

    fn actor() -> Actor {
        Actor::new("user:ruben").expect("static actor")
    }

    fn arch(id: &str, kind: SubjectKind) -> SubjectRef {
        SubjectRef::new(Namespace::Arch, kind, id.to_owned()).expect("valid id")
    }

    fn props(entries: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    fn defined(
        store: &mut GraphStore,
        subject: SubjectRef,
        properties: BTreeMap<String, serde_json::Value>,
    ) {
        store
            .append(import_event(
                &actor(),
                EventPayload::SubjectDefined(SubjectDefined {
                    subject: subject.clone(),
                    authority: AuthorityClass::Authoritative,
                    provenance: Provenance::new("user:ruben").unwrap(),
                    properties,
                }),
                vec![subject],
            ))
            .unwrap();
    }

    fn declared(store: &mut GraphStore, from: SubjectRef, kind: &str, to: SubjectRef) {
        let fact = RelationFact {
            relation: RelationRef::new(
                from,
                RelationKind::parse(kind).unwrap(),
                to,
            )
            .unwrap(),
            authority: AuthorityClass::Authoritative,
            provenance: Provenance::new("user:ruben").unwrap(),
        };
        store
            .append(import_event(
                &actor(),
                EventPayload::RelationDeclared(RelationDeclared { fact }),
                vec![],
            ))
            .unwrap();
    }

    fn sample_store() -> GraphStore {
        let mut store = GraphStore::new();
        let checkout = arch("checkout", SubjectKind::System);
        let payments = arch("payments", SubjectKind::Container);
        defined(
            &mut store,
            checkout,
            props(&[
                ("name", serde_json::json!("Checkout")),
                ("description", serde_json::json!("Checkout system")),
            ]),
        );
        defined(&mut store, payments, props(&[("name", serde_json::json!("Payments"))]));
        declared(
            &mut store,
            arch("checkout", SubjectKind::System),
            "calls",
            arch("payments", SubjectKind::Container),
        );
        store
    }

    #[test]
    fn export_is_deterministic_and_embeds_identity() {
        let store = sample_store();
        let first = likec4_source(store.graph());
        let second = likec4_source(store.graph());
        assert_eq!(first, second);
        assert!(first.contains("vistalith 'arch:system:checkout'"));
        assert!(first.contains("vistalith 'arch:container:payments'"));
        assert!(first.contains("-[calls]->"));
        assert!(first.contains("relationship calls"));
    }

    #[test]
    fn round_trip_preserves_identity_and_structure() {
        let store = sample_store();
        let model = parse_likec4(&likec4_source(store.graph())).unwrap();
        let mut imported = GraphStore::new();
        let report = import_likec4(&mut imported, &model, &actor()).unwrap();
        assert_eq!(report.defined_subjects.len(), 2);
        assert_eq!(report.declared_relations.len(), 1);
        assert_eq!(c4_view(imported.graph()), c4_view(store.graph()));
    }

    #[test]
    fn reimport_of_exported_model_is_a_noop() {
        let mut store = sample_store();
        let model = parse_likec4(&likec4_source(store.graph())).unwrap();
        import_likec4(&mut store, &model, &actor()).unwrap();
        let before = store.graph().revision();
        let report = import_likec4(&mut store, &model, &actor()).unwrap();
        assert_eq!(store.graph().revision(), before);
        assert_eq!(report.unchanged_subjects.len(), 2);
        assert_eq!(report.skipped_relations.len(), 1);
        assert!(report.updated_subjects.is_empty());
        assert!(report.declared_relations.is_empty());
    }

    #[test]
    fn export_sanitizes_ids_but_metadata_keeps_identity() {
        let mut store = GraphStore::new();
        defined(
            &mut store,
            arch("0198uuid", SubjectKind::System),
            props(&[("name", serde_json::json!("Weird"))]),
        );
        let source = likec4_source(store.graph());
        assert!(source.contains("system e0198uuid"));
        let model = parse_likec4(&source).unwrap();
        let mut imported = GraphStore::new();
        import_likec4(&mut imported, &model, &actor()).unwrap();
        assert_eq!(c4_view(imported.graph()), c4_view(store.graph()));
    }

    #[test]
    fn import_updates_only_changed_properties() {
        let mut store = sample_store();
        let source = likec4_source(store.graph());
        let renamed = source.replace("\"Checkout\"", "\"Checkout Prime\"");
        let model = parse_likec4(&renamed).unwrap();
        let report = import_likec4(&mut store, &model, &actor()).unwrap();
        assert_eq!(report.updated_subjects.len(), 1);
        assert_eq!(report.updated_subjects[0].to_string(), "arch:system:checkout");
        let node = store
            .graph()
            .subject(&arch("checkout", SubjectKind::System))
            .unwrap();
        assert_eq!(node.properties["name"], serde_json::json!("Checkout Prime"));
    }

    #[test]
    fn parses_nested_elements_scoped_relationships_and_this() {
        let source = r#"
            specification {
                element system
                element container
                element component
                element datastore
                relationship uses
                relationship queries
            }
            model {
                system cloud {
                    container api "API" {
                        component worker
                        this -> db
                        api -[uses]-> worker
                    }
                    datastore db "Database"
                }
                cloud.api -[queries]-> cloud.db "reads"
            }
        "#;
        let model = parse_likec4(source).unwrap();
        assert_eq!(model.elements.len(), 4);
        let fqns: Vec<&str> = model.elements.iter().map(|e| e.fqn.as_str()).collect();
        for expected in ["cloud", "cloud.api", "cloud.api.worker", "cloud.db"] {
            assert!(fqns.contains(&expected), "missing {expected}");
        }
        let api = model
            .elements
            .iter()
            .find(|e| e.fqn == "cloud.api")
            .unwrap();
        assert_eq!(api.name.as_deref(), Some("API"));
        assert_eq!(api.kind, "container");
        assert_eq!(model.relationships.len(), 3);
        assert!(model.relationships.iter().any(|r| {
            r.source == "cloud.api" && r.target == "cloud.api.worker" && r.kind.as_deref() == Some("uses")
        }));
        assert!(model
            .relationships
            .iter()
            .any(|r| r.source == "cloud.api" && r.target == "cloud.db" && r.kind.is_none()));
        assert!(model.relationships.iter().any(|r| {
            r.source == "cloud.api"
                && r.target == "cloud.db"
                && r.kind.as_deref() == Some("queries")
                && r.title.as_deref() == Some("reads")
        }));
    }

    #[test]
    fn extend_merges_metadata_tags_and_description() {
        let source = r#"
            model {
                system shop "Shop"
                extend shop {
                    description 'renamed shop'
                    metadata { owner 'payments-team' }
                    tag #legacy
                }
            }
        "#;
        let model = parse_likec4(source).unwrap();
        assert_eq!(model.elements.len(), 1);
        let shop = &model.elements[0];
        assert_eq!(shop.description.as_deref(), Some("renamed shop"));
        assert_eq!(
            shop.metadata.get("owner").map(String::as_str),
            Some("payments-team")
        );
        assert_eq!(shop.tags, vec!["legacy"]);
    }

    #[test]
    fn import_without_metadata_uses_fqn_identity_and_default_kind() {
        let source = r#"
            model {
                system crm {
                    container web "Web"
                }
                crm -[depends_on]-> crm.web
                crm.web -> crm
            }
        "#;
        let model = parse_likec4(source).unwrap();
        let mut store = GraphStore::new();
        let report = import_likec4(&mut store, &model, &actor()).unwrap();
        assert_eq!(report.defined_subjects.len(), 2);
        assert!(report
            .defined_subjects
            .iter()
            .any(|s| s.to_string() == "arch:system:crm"));
        assert!(report
            .defined_subjects
            .iter()
            .any(|s| s.to_string() == "arch:container:crm.web"));
        // typed relationship maps to its kind; the untyped one lands on `uses`
        let kinds: Vec<String> = report
            .declared_relations
            .iter()
            .map(|r| r.kind.to_string())
            .collect();
        assert!(kinds.contains(&"depends_on".to_owned()));
        assert!(kinds.contains(&"uses".to_owned()));
    }

    #[test]
    fn import_rejects_unsupported_element_kinds() {
        let source = "specification { element actor } model { actor user \"User\" }";
        let model = parse_likec4(source).unwrap();
        let mut store = GraphStore::new();
        let result = import_likec4(&mut store, &model, &actor());
        assert!(matches!(
            result,
            Err(LikeC4Error::UnsupportedElementKind(kind)) if kind == "actor"
        ));
    }

    #[test]
    fn import_rejects_invalid_vistalith_metadata() {
        let source = "model { system a { metadata { vistalith 'not-a-ref' } } }";
        let model = parse_likec4(source).unwrap();
        let mut store = GraphStore::new();
        let result = import_likec4(&mut store, &model, &actor());
        assert!(matches!(
            result,
            Err(LikeC4Error::InvalidIdentity(raw)) if raw == "not-a-ref"
        ));
    }

    #[test]
    fn parse_errors_are_explicit() {
        // unknown element kind used as a definition reads as a relationship
        // missing its arrow
        assert!(matches!(
            parse_likec4("model { pod x }"),
            Err(LikeC4Error::Parse(_))
        ));
        // dangling relationship target
        assert!(matches!(
            parse_likec4("model { system a a -> ghost }"),
            Err(LikeC4Error::UnknownElement(target)) if target == "ghost"
        ));
        // ambiguous suffix reference
        assert!(matches!(
            parse_likec4(
                "model { system one { component b } system two { component b } one -> two b -> one }"
            ),
            Err(LikeC4Error::AmbiguousElement(_))
        ));
        // unterminated string
        assert!(matches!(
            parse_likec4("model { system a \"oops }"),
            Err(LikeC4Error::Parse(_))
        ));
    }

    #[test]
    fn comments_and_views_are_tolerated() {
        let source = r#"
            // leading comment
            model {
                /* block
                   comment */
                system a "A" // trailing
            }
            views {
                view overview {
                    include *
                }
            }
        "#;
        let model = parse_likec4(source).unwrap();
        assert_eq!(model.elements.len(), 1);
        assert_eq!(model.elements[0].fqn, "a");
        assert_eq!(model.elements[0].name.as_deref(), Some("A"));
    }

    #[test]
    fn c4_diff_reports_rename_as_change_not_remove_add() {
        let mut store = sample_store();
        let from = store.graph().revision();
        store
            .append(import_event(
                &actor(),
                EventPayload::SubjectUpdated(SubjectUpdated {
                    subject: arch("checkout", SubjectKind::System),
                    properties: props(&[("name", serde_json::json!("Checkout Prime"))]),
                }),
                vec![],
            ))
            .unwrap();
        let gateway = arch("gateway", SubjectKind::Component);
        defined(
            &mut store,
            gateway,
            props(&[("name", serde_json::json!("Gateway"))]),
        );
        declared(
            &mut store,
            arch("gateway", SubjectKind::Component),
            "calls",
            arch("payments", SubjectKind::Container),
        );
        let diff = c4_diff(&store.graph_at_revision(from).unwrap(), store.graph());
        assert_eq!(diff.from_revision, from);
        assert_eq!(diff.added_elements.len(), 1);
        assert_eq!(diff.added_elements[0].identity, "arch:component:gateway");
        assert_eq!(diff.changed_elements.len(), 1);
        assert_eq!(diff.changed_elements[0].identity, "arch:system:checkout");
        assert_eq!(diff.changed_elements[0].changes[0].key, "name");
        assert_eq!(diff.changed_elements[0].changes[0].from, Some(serde_json::json!("Checkout")));
        assert_eq!(diff.changed_elements[0].changes[0].to, Some(serde_json::json!("Checkout Prime")));
        assert_eq!(diff.added_relationships.len(), 1);
        assert!(diff.removed_elements.is_empty());
        assert!(diff.removed_relationships.is_empty());
        assert!(diff.changed_relationships.is_empty());
    }

    #[test]
    fn c4_diff_of_identical_graphs_is_empty() {
        let store = sample_store();
        let diff = c4_diff(store.graph(), store.graph());
        assert!(diff.is_empty());
    }
}
