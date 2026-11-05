//! A small owned XML tree over `quick-xml`, and the zip/relationship plumbing every
//! Office reader needs.
//!
//! OOXML is deeply nested (a table cell holds paragraphs that hold runs that hold an
//! equation that holds a fraction), so the readers walk a tree rather than the flat event
//! stream. Names are kept qualified (`w:p`) and matched on their *local* part, which is
//! what every producer agrees on even when the prefixes differ.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};

/// An element: its qualified name, attributes and children, in document order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// The qualified name (`w:p`).
    pub name: String,
    /// Attributes as `(qualified name, unescaped value)`.
    pub attrs: Vec<(String, String)>,
    /// Child elements and text.
    pub children: Vec<Child>,
}

/// A child of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    /// A nested element.
    Elem(Node),
    /// Character data, entities resolved.
    Text(String),
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl Node {
    /// The name without its namespace prefix.
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    /// Is this element called `name` (local part)?
    pub fn is(&self, name: &str) -> bool {
        self.local() == name
    }

    /// The value of the attribute whose local name is `name`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| local(k) == name)
            .map(|(_, v)| v.as_str())
    }

    /// The child elements.
    pub fn elems(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|c| match c {
            Child::Elem(n) => Some(n),
            Child::Text(_) => None,
        })
    }

    /// The first child element called `name`.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.elems().find(|n| n.is(name))
    }

    /// Every child element called `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.elems().filter(move |n| n.is(name))
    }

    /// Follow a path of local names through first children.
    pub fn path(&self, names: &[&str]) -> Option<&Node> {
        let mut node = self;
        for name in names {
            node = node.child(name)?;
        }
        Some(node)
    }

    /// The first descendant (depth first, self excluded) called `name`.
    pub fn find(&self, name: &str) -> Option<&Node> {
        for child in self.elems() {
            if child.is(name) {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// Every descendant called `name`, in document order (not descending into matches).
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for child in self.elems() {
            if child.is(name) {
                out.push(child);
            } else {
                child.find_all(name, out);
            }
        }
    }

    /// The concatenated character data of this element and its descendants.
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.push_text(&mut s);
        s
    }

    fn push_text(&self, s: &mut String) {
        for child in &self.children {
            match child {
                Child::Text(t) => s.push_str(t),
                Child::Elem(n) => n.push_text(s),
            }
        }
    }

    /// `w:val`-style attribute of the child `name` (`<w:pStyle w:val="Heading1"/>`).
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|n| n.attr("val"))
    }

    /// An OOXML on/off property: present and not `0`/`false`/`off`.
    pub fn flag(&self, name: &str) -> bool {
        self.child(name).is_some_and(|n| {
            !matches!(
                n.attr("val"),
                Some("0") | Some("false") | Some("off") | Some("none")
            )
        })
    }
}

/// Parse an XML document into its root element.
///
/// # Errors
///
/// [`Error::Read`] on malformed XML or a document without a root.
pub fn parse(xml: &[u8]) -> Result<Node> {
    let mut reader = Reader::from_reader(xml);
    let mut stack: Vec<Node> = vec![Node::default()];
    let mut buf = Vec::new();
    let bad = |e: &dyn std::fmt::Display| Error::Read(format!("invalid XML: {e}"));
    loop {
        let event = reader.read_event_into(&mut buf).map_err(|e| bad(&e))?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let mut attrs = Vec::new();
                for attr in e.attributes().with_checks(false).flatten() {
                    let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                    let value = attr
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map(|v| v.into_owned())
                        .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).into_owned());
                    attrs.push((key, value));
                }
                let node = Node {
                    name,
                    attrs,
                    children: Vec::new(),
                };
                if matches!(event, Event::Empty(_)) {
                    push_child(&mut stack, Child::Elem(node));
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) if stack.len() > 1 => {
                let node = stack.pop().unwrap_or_default();
                push_child(&mut stack, Child::Elem(node));
            }
            Event::Text(t) => {
                let text = t.decode().map_err(|e| bad(&e))?;
                push_text(&mut stack, &text);
            }
            Event::CData(t) => {
                push_text(&mut stack, &String::from_utf8_lossy(&t));
            }
            Event::GeneralRef(r) => {
                let resolved = if r.is_char_ref() {
                    r.resolve_char_ref()
                        .ok()
                        .flatten()
                        .map(String::from)
                        .unwrap_or_default()
                } else {
                    let raw: &[u8] = &r;
                    match raw {
                        b"lt" => "<".into(),
                        b"gt" => ">".into(),
                        b"amp" => "&".into(),
                        b"quot" => "\"".into(),
                        b"apos" => "'".into(),
                        other => format!("&{};", String::from_utf8_lossy(other)),
                    }
                };
                push_text(&mut stack, &resolved);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    while stack.len() > 1 {
        let node = stack.pop().unwrap_or_default();
        push_child(&mut stack, Child::Elem(node));
    }
    stack
        .pop()
        .and_then(|doc| doc.elems().next().cloned())
        .ok_or_else(|| Error::Read("empty XML document".into()))
}

fn push_child(stack: &mut [Node], child: Child) {
    if let Some(top) = stack.last_mut() {
        top.children.push(child);
    }
}

fn push_text(stack: &mut [Node], text: &str) {
    if let Some(top) = stack.last_mut() {
        if let Some(Child::Text(prev)) = top.children.last_mut() {
            prev.push_str(text);
        } else {
            top.children.push(Child::Text(text.to_string()));
        }
    }
}

/// An Office package (a zip), read fully into memory.
pub struct Package {
    parts: BTreeMap<String, Vec<u8>>,
}

/// One relationship of a part.
#[derive(Debug, Clone)]
pub struct Rel {
    /// The target, resolved to a package path when internal, verbatim when external.
    pub target: String,
    /// The relationship type URI's last segment (`image`, `hyperlink`, `slide`, …).
    pub kind: String,
    /// `TargetMode="External"`.
    pub external: bool,
}

impl Package {
    /// Open a zip from bytes.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] when the bytes are not a readable zip.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| Error::Read(format!("not an Office document (zip): {e}")))?;
        let mut parts = BTreeMap::new();
        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .map_err(|e| Error::Read(format!("corrupt zip entry: {e}")))?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().trim_start_matches('/').to_string();
            let mut data = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut data)
                .map_err(|e| Error::Read(format!("cannot inflate `{name}`: {e}")))?;
            parts.insert(name, data);
        }
        Ok(Package { parts })
    }

    /// The raw bytes of a part.
    pub fn bytes(&self, path: &str) -> Option<&[u8]> {
        self.parts.get(path).map(Vec::as_slice)
    }

    /// Does the package hold this part?
    pub fn has(&self, path: &str) -> bool {
        self.parts.contains_key(path)
    }

    /// Parse a part as XML; `None` when absent.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] when the part exists but is not well-formed XML.
    pub fn xml(&self, path: &str) -> Result<Option<Node>> {
        match self.parts.get(path) {
            Some(bytes) => parse(bytes)
                .map(Some)
                .map_err(|e| Error::Read(format!("{path}: {e}"))),
            None => Ok(None),
        }
    }

    /// The relationships of `part` (`word/document.xml` → `word/_rels/document.xml.rels`),
    /// keyed by id.
    ///
    /// # Errors
    ///
    /// [`Error::Read`] on a malformed relationships part.
    pub fn rels(&self, part: &str) -> Result<BTreeMap<String, Rel>> {
        let (dir, file) = match part.rfind('/') {
            Some(i) => (&part[..i], &part[i + 1..]),
            None => ("", part),
        };
        let rels_path = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let mut out = BTreeMap::new();
        let Some(root) = self.xml(&rels_path)? else {
            return Ok(out);
        };
        for rel in root.children_named("Relationship") {
            let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) else {
                continue;
            };
            let external = rel.attr("TargetMode") == Some("External");
            let kind = rel
                .attr("Type")
                .and_then(|t| t.rsplit('/').next())
                .unwrap_or_default()
                .to_string();
            let target = if external {
                target.to_string()
            } else {
                resolve(dir, target)
            };
            out.insert(
                id.to_string(),
                Rel {
                    target,
                    kind,
                    external,
                },
            );
        }
        Ok(out)
    }
}

/// Resolve a relative part reference against the directory of the referencing part.
fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_elements_text_and_entities() {
        let root = parse(br#"<?xml version="1.0"?><w:p xmlns:w="x"><w:r><w:t xml:space="preserve">a &amp; b &#233;</w:t></w:r><w:br/></w:p>"#).unwrap();
        assert!(root.is("p"));
        assert_eq!(root.text(), "a & b é");
        assert_eq!(root.elems().count(), 2);
        assert_eq!(
            root.path(&["r", "t"]).unwrap().attr("space"),
            Some("preserve")
        );
    }

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("word", "media/a.png"), "word/media/a.png");
        assert_eq!(resolve("word", "/word/x.xml"), "word/x.xml");
    }

    #[test]
    fn flags_follow_ooxml_on_off() {
        let rpr = parse(br#"<rPr><b/><i val="0"/></rPr>"#).unwrap();
        assert!(rpr.flag("b"));
        assert!(!rpr.flag("i"));
        assert!(!rpr.flag("strike"));
    }
}
