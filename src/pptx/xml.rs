//! A small owned XML tree over `quick-xml`, and the escaping the writer needs.
//!
//! OOXML parts are small (a slide, a layout, a master: tens of kilobytes), and every
//! question the PPTX code asks of them is structural ("the `<a:xfrm>` of this
//! placeholder", "the `sz` of `lvl2pPr` in the master's body style"). A tree answers
//! those directly, where a streaming reader would need a state machine per question.
//! Names are kept as written (`p:sp`, `a:off`): OOXML producers use the conventional
//! prefixes, and matching on local names ([`Node::local`]) makes the rare exception
//! harmless.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};

/// One element: its qualified name, attributes in document order, children and the
/// text directly inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// The qualified name (`p:sp`).
    pub name: String,
    /// Attributes, in document order, values unescaped.
    pub attrs: Vec<(String, String)>,
    /// Child elements, in document order.
    pub children: Vec<Node>,
    /// The text directly inside this element (concatenated), unescaped.
    pub text: String,
}

impl Node {
    /// The name without its prefix (`sp` for `p:sp`).
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    /// The value of the attribute whose local name is `key`.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key || local(k) == key)
            .map(|(_, v)| v.as_str())
    }

    /// An attribute parsed as an integer.
    pub fn attr_i64(&self, key: &str) -> Option<i64> {
        self.attr(key)?.trim().parse().ok()
    }

    /// The first child whose local name is `name`.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.local() == name)
    }

    /// Every child whose local name is `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.local() == name)
    }

    /// Follow a path of local names (`["nvSpPr", "nvPr", "ph"]`), first match each step.
    pub fn path(&self, path: &[&str]) -> Option<&Node> {
        let mut node = self;
        for step in path {
            node = node.child(step)?;
        }
        Some(node)
    }

    /// Every descendant (depth first, document order) whose local name is `name`.
    pub fn descendants<'a>(&'a self, name: &str) -> Vec<&'a Node> {
        let mut out = Vec::new();
        self.collect(name, &mut out);
        out
    }

    fn collect<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.local() == name {
                out.push(c);
            }
            c.collect(name, out);
        }
    }
}

/// The local part of a qualified name.
pub fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Parse an XML document into its root element.
///
/// # Errors
///
/// [`Error::Read`] on malformed XML or an empty document.
pub fn parse(bytes: &[u8]) -> Result<Node> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Read(format!("not UTF-8: {e}")))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    let bad = |e: &dyn std::fmt::Display| Error::Read(format!("invalid XML: {e}"));
    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(start) => {
                stack.push(element(&start, reader.decoder()).map_err(|e| bad(&e))?);
            }
            Event::Empty(start) => {
                let node = element(&start, reader.decoder()).map_err(|e| bad(&e))?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| bad(&"unbalanced end tag"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Err(bad(&"unbalanced end tag")),
                }
            }
            Event::Text(t) => {
                let raw = t.decode().map_err(|e| bad(&e))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&raw);
                }
            }
            Event::CData(t) => {
                let raw = t.decode().map_err(|e| bad(&e))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&raw);
                }
            }
            Event::GeneralRef(r) => {
                let ch = match r.resolve_char_ref().map_err(|e| bad(&e))? {
                    Some(c) => c.to_string(),
                    None => match r.decode().map_err(|e| bad(&e))?.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => format!("&{other};"),
                    },
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&ch);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut doc = stack.pop().ok_or_else(|| bad(&"empty document"))?;
    if !stack.is_empty() {
        return Err(bad(&"unclosed element"));
    }
    let root = doc.children.drain(..).next();
    root.ok_or_else(|| bad(&"no root element"))
}

fn element(
    start: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> std::result::Result<Node, String> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for a in start.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let value = a
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, decoder)
            .map_err(|e| e.to_string())?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Node {
        name,
        attrs,
        children: Vec::new(),
        text: String::new(),
    })
}

/// Escape text for an XML text node or attribute value.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Characters XML 1.0 cannot carry at all: dropped rather than producing a
            // part PowerPoint refuses to open.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_attributes_and_text() {
        let root = parse(br#"<?xml version="1.0"?><p:sp a="1&amp;2"><a:t>x &lt; y&#x41;</a:t><a:off x="3"/></p:sp>"#).unwrap();
        assert_eq!(root.local(), "sp");
        assert_eq!(root.attr("a"), Some("1&2"));
        assert_eq!(root.child("t").unwrap().text, "x < yA");
        assert_eq!(root.path(&["off"]).unwrap().attr_i64("x"), Some(3));
    }

    #[test]
    fn rejects_broken_xml() {
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn escapes_and_drops_control_characters() {
        assert_eq!(esc("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
    }
}
