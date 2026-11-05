//! The `.docx` package: the parts around `word/document.xml`, taken from a reference
//! document or generated, and the deterministic zip they are packed into.
//!
//! With `--reference-doc`, every part of the reference is kept as is — theme, fonts,
//! settings, headers and footers with their images, custom XML — except the ones the
//! conversion owns: the body (`document.xml`, whose final section properties, page size,
//! margins, header and footer references, are kept), the footnotes, the core properties,
//! the comments of the dropped body, and the media only the dropped body used.
//! `styles.xml` and `numbering.xml` are kept and *extended* (missing styles, our list
//! definitions with ids above the reference's own). This is how Pandoc treats a
//! reference document, without its post-hoc zip surgery.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use super::styles;
use super::xml::{escape_attr, escape_text};
use crate::error::{Error, Result};

/// Relationship type URIs.
pub(super) mod rel_type {
    pub const OFFICE_DOCUMENT: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    pub const CORE: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    pub const APP: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
    pub const STYLES: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
    pub const NUMBERING: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
    pub const FOOTNOTES: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
    pub const SETTINGS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
    pub const IMAGE: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    pub const HYPERLINK: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
}

const CT_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const CT_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// One relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rel {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub external: bool,
}

/// The relationships of one part, handing out fresh ids above the existing ones.
#[derive(Debug, Clone, Default)]
pub(super) struct RelSet {
    pub rels: Vec<Rel>,
    next: usize,
}

impl RelSet {
    pub(super) fn new(existing: Vec<Rel>) -> Self {
        let next = existing
            .iter()
            .filter_map(|r| {
                r.id.strip_prefix("rId")
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .max()
            .unwrap_or(0)
            + 1;
        RelSet {
            rels: existing,
            next,
        }
    }

    /// The id of the relationship `(kind, target)`, added if new.
    pub(super) fn add(&mut self, kind: &str, target: &str, external: bool) -> String {
        if let Some(r) = self
            .rels
            .iter()
            .find(|r| r.kind == kind && r.target == target && r.external == external)
        {
            return r.id.clone();
        }
        let id = format!("rId{}", self.next);
        self.next += 1;
        self.rels.push(Rel {
            id: id.clone(),
            kind: kind.to_string(),
            target: target.to_string(),
            external,
        });
        id
    }

    pub(super) fn has_kind(&self, kind: &str) -> bool {
        self.rels.iter().any(|r| r.kind == kind)
    }

    pub(super) fn to_xml(&self) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        );
        for r in &self.rels {
            s.push_str(&format!(
                "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
                escape_attr(&r.id),
                escape_attr(&r.kind),
                escape_attr(&r.target),
                if r.external {
                    " TargetMode=\"External\""
                } else {
                    ""
                }
            ));
        }
        s.push_str("</Relationships>");
        s
    }
}

/// Parse a `.rels` part.
pub(super) fn parse_rels(xml: &str) -> Vec<Rel> {
    tags(xml, "<Relationship ")
        .into_iter()
        .filter_map(|tag| {
            Some(Rel {
                id: unescape(styles::attr_value(tag, "Id")?),
                kind: unescape(styles::attr_value(tag, "Type")?),
                target: unescape(styles::attr_value(tag, "Target")?),
                external: styles::attr_value(tag, "TargetMode") == Some("External"),
            })
        })
        .collect()
}

/// Every start tag beginning with `open` (`"<Relationship "`), up to its `>`.
fn tags<'a>(xml: &'a str, open: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(open) {
        let tag = &rest[at..];
        let end = tag.find('>').map_or(tag.len(), |e| e + 1);
        out.push(&tag[..end]);
        rest = &tag[end..];
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The usable text area, in twips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Page {
    pub text_width: u64,
    pub text_height: u64,
}

/// What a conversion starts from: the reference document's parts, or our defaults.
#[derive(Debug, Clone)]
pub(super) struct Base {
    /// Parts copied verbatim, by name.
    parts: BTreeMap<String, Vec<u8>>,
    /// `word/styles.xml`.
    pub styles: String,
    /// The reference's own `word/numbering.xml`.
    pub numbering: Option<String>,
    /// The body's final `<w:sectPr>`.
    pub sect_pr: String,
    /// The document part's relationships worth keeping (headers, footers, theme…).
    pub doc_rels: Vec<Rel>,
    /// Package-level relationships worth keeping (custom properties, thumbnail).
    root_rels: Vec<Rel>,
    /// `[Content_Types].xml` defaults and overrides of the reference.
    ct_defaults: BTreeMap<String, String>,
    ct_overrides: BTreeMap<String, String>,
    /// The ids `settings.xml` gives the separator and continuation-separator footnotes.
    pub separator_ids: (i64, i64),
    pub page: Page,
}

/// A4 with one-inch margins, Word's default.
const DEFAULT_SECT_PR: &str = r#"<w:sectPr><w:footnotePr><w:numRestart w:val="eachSect"/></w:footnotePr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>"#;

impl Base {
    /// Our own package: no reference document.
    pub(super) fn default_base(lang: Option<&str>) -> Self {
        let mut parts = BTreeMap::new();
        parts.insert(
            "word/settings.xml".to_string(),
            DEFAULT_SETTINGS.as_bytes().to_vec(),
        );
        let mut ct_overrides = BTreeMap::new();
        ct_overrides.insert("/word/settings.xml".to_string(), CT_SETTINGS.to_string());
        Base {
            parts,
            styles: styles::default_styles(lang),
            numbering: None,
            sect_pr: DEFAULT_SECT_PR.to_string(),
            doc_rels: vec![Rel {
                id: "rId1".into(),
                kind: rel_type::SETTINGS.into(),
                target: "settings.xml".into(),
                external: false,
            }],
            root_rels: Vec::new(),
            ct_defaults: BTreeMap::new(),
            ct_overrides,
            separator_ids: (-1, 0),
            page: page_of(DEFAULT_SECT_PR),
        }
    }

    /// Start from a reference `.docx`.
    ///
    /// # Errors
    ///
    /// [`Error::Template`] when the bytes are not a zip holding a Word document.
    pub(super) fn from_reference(bytes: &[u8], lang: Option<&str>) -> Result<Self> {
        let bad = |why: String| Error::Template(why);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| bad(format!("not a .docx (zip): {e}")))?;
        let mut parts: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| bad(format!("unreadable zip entry: {e}")))?;
            if file.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            file.read_to_end(&mut data)
                .map_err(|e| bad(format!("unreadable zip entry: {e}")))?;
            parts.insert(file.name().trim_start_matches('/').to_string(), data);
        }
        let text = |parts: &BTreeMap<String, Vec<u8>>, name: &str| {
            parts
                .get(name)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        let document = text(&parts, "word/document.xml")
            .ok_or_else(|| bad("no word/document.xml: not a Word document".into()))?;
        let mut styles = text(&parts, "word/styles.xml").unwrap_or_else(|| {
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"></w:styles>".to_string()
        });
        if let Some(lang) = lang {
            styles = styles::set_language(&styles, lang);
        }
        let numbering = text(&parts, "word/numbering.xml");
        let settings = text(&parts, "word/settings.xml").unwrap_or_default();
        let sect_pr = final_sect_pr(&document).unwrap_or_else(|| DEFAULT_SECT_PR.to_string());
        let page = page_of(&sect_pr);

        // Content types.
        let ct = text(&parts, "[Content_Types].xml").unwrap_or_default();
        let ct_defaults = tags(&ct, "<Default ")
            .into_iter()
            .filter_map(|t| {
                Some((
                    styles::attr_value(t, "Extension")?.to_ascii_lowercase(),
                    styles::attr_value(t, "ContentType")?.to_string(),
                ))
            })
            .collect();
        let ct_overrides = tags(&ct, "<Override ")
            .into_iter()
            .filter_map(|t| {
                Some((
                    styles::attr_value(t, "PartName")?.to_string(),
                    styles::attr_value(t, "ContentType")?.to_string(),
                ))
            })
            .collect();

        // Relationships: drop what belonged to the body, and what we rewrite.
        const DROPPED: &[&str] = &[
            "/image",
            "/hyperlink",
            "/comments",
            "/commentsExtended",
            "/commentsIds",
            "/commentsExtensible",
            "/people",
            "/footnotes",
            "/styles",
            "/numbering",
            "/glossaryDocument",
        ];
        let doc_rels: Vec<Rel> = text(&parts, "word/_rels/document.xml.rels")
            .map(|x| parse_rels(&x))
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !DROPPED.iter().any(|d| r.kind.ends_with(d)))
            .collect();
        let root_rels: Vec<Rel> = text(&parts, "_rels/.rels")
            .map(|x| parse_rels(&x))
            .unwrap_or_default()
            .into_iter()
            .filter(|r| {
                r.kind != rel_type::OFFICE_DOCUMENT
                    && r.kind != rel_type::CORE
                    && r.kind != rel_type::APP
            })
            .collect();

        // Parts we own or that only the dropped body used.
        let owned = [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "word/footnotes.xml",
            "word/_rels/footnotes.xml.rels",
            "word/styles.xml",
            "word/numbering.xml",
            "docProps/core.xml",
        ];
        for name in owned {
            parts.remove(name);
        }
        parts.retain(|name, _| {
            let base = name.rsplit('/').next().unwrap_or(name);
            !(base.starts_with("comments")
                || base == "people.xml"
                || name.starts_with("word/glossary/"))
        });
        // Keep only the media some kept part still points at (a header's logo).
        let mut referenced: BTreeSet<String> = BTreeSet::new();
        for r in &doc_rels {
            referenced.insert(resolve("word/", &r.target));
        }
        for (name, data) in &parts {
            if let Some(dir) = rels_source_dir(name) {
                for r in parse_rels(&String::from_utf8_lossy(data)) {
                    if !r.external {
                        referenced.insert(resolve(&dir, &r.target));
                    }
                }
            }
        }
        parts.retain(|name, _| !name.starts_with("word/media/") || referenced.contains(name));

        Ok(Base {
            parts,
            styles,
            numbering,
            sect_pr,
            doc_rels,
            root_rels,
            ct_defaults,
            ct_overrides,
            separator_ids: separator_ids(&settings),
            page,
        })
    }
}

/// The directory a `.rels` part's targets are relative to (`word/_rels/header1.xml.rels`
/// → `word/`), `None` for other parts.
fn rels_source_dir(name: &str) -> Option<String> {
    let (dir, file) = name.rsplit_once("_rels/")?;
    file.ends_with(".rels").then(|| dir.to_string())
}

/// Resolve a relationship target against the source part's directory.
fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                segments.pop();
            }
            "." | "" => {}
            s => segments.push(s),
        }
    }
    segments.join("/")
}

/// The body's last `<w:sectPr>…</w:sectPr>` (the document's final section).
fn final_sect_pr(document: &str) -> Option<String> {
    let body_end = document.rfind("</w:body>").unwrap_or(document.len());
    let start = document[..body_end].rfind("<w:sectPr")?;
    let rest = &document[start..body_end];
    let tag_end = rest.find('>')?;
    if rest[..tag_end].ends_with('/') {
        return Some(rest[..=tag_end].to_string());
    }
    let close = rest.find("</w:sectPr>")?;
    Some(rest[..close + "</w:sectPr>".len()].to_string())
}

/// The text area of a section, from `w:pgSz` and `w:pgMar`.
fn page_of(sect_pr: &str) -> Page {
    let num = |tag: &str, attr: &str, default: u64| -> u64 {
        sect_pr
            .find(tag)
            .and_then(|at| {
                let t = &sect_pr[at..];
                let end = t.find('>').unwrap_or(t.len());
                styles::attr_value(&t[..end], attr)
            })
            .and_then(|v| v.parse::<i64>().ok())
            .map_or(default, |v| v.max(0) as u64)
    };
    let width = num("<w:pgSz", "w:w", 11906);
    let height = num("<w:pgSz", "w:h", 16838);
    let left = num("<w:pgMar", "w:left", 1440);
    let right = num("<w:pgMar", "w:right", 1440);
    let top = num("<w:pgMar", "w:top", 1440);
    let bottom = num("<w:pgMar", "w:bottom", 1440);
    Page {
        text_width: width.saturating_sub(left + right).max(1440),
        text_height: height.saturating_sub(top + bottom).max(1440),
    }
}

/// The separator footnote ids `settings.xml` declares (Word: -1 and 0).
fn separator_ids(settings: &str) -> (i64, i64) {
    let Some(at) = settings.find("<w:footnotePr>") else {
        return (-1, 0);
    };
    let end = settings[at..]
        .find("</w:footnotePr>")
        .map_or(settings.len(), |e| e + at);
    let ids: Vec<i64> = tags(&settings[at..end], "<w:footnote ")
        .into_iter()
        .filter_map(|t| styles::attr_value(t, "w:id")?.parse().ok())
        .collect();
    match ids.as_slice() {
        [a, b, ..] => (*a, *b),
        _ => (-1, 0),
    }
}

/// The highest `w:abstractNumId` and `w:numId` a `numbering.xml` declares.
pub(super) fn numbering_max(numbering: &str) -> (usize, usize) {
    let max_of = |open: &str, attr: &str| {
        tags(numbering, open)
            .into_iter()
            .filter_map(|t| styles::attr_value(t, attr)?.parse::<usize>().ok())
            .max()
            .unwrap_or(0)
    };
    (
        max_of("<w:abstractNum ", "w:abstractNumId"),
        max_of("<w:num ", "w:numId"),
    )
}

/// Merge our list definitions into a numbering part (the reference's, or a new one):
/// abstract definitions before the first `<w:num>`, instances at the end, as the schema
/// orders them.
pub(super) fn merge_numbering(existing: Option<&str>, abstracts: &str, nums: &str) -> String {
    let Some(xml) = existing else {
        return format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
             <w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
             {abstracts}{nums}</w:numbering>"
        );
    };
    let first_num = find_tag(xml, "<w:num ")
        .or_else(|| find_tag(xml, "<w:num>"))
        .or_else(|| xml.find("<w:numIdMacAtCleanup"))
        .or_else(|| xml.rfind("</w:numbering>"));
    let Some(first_num) = first_num else {
        return xml.to_string();
    };
    let with_abstracts = format!("{}{abstracts}{}", &xml[..first_num], &xml[first_num..]);
    let end = with_abstracts
        .find("<w:numIdMacAtCleanup")
        .or_else(|| with_abstracts.rfind("</w:numbering>"))
        .unwrap_or(with_abstracts.len());
    format!("{}{nums}{}", &with_abstracts[..end], &with_abstracts[end..])
}

fn find_tag(xml: &str, open: &str) -> Option<usize> {
    xml.find(open)
}

/// Everything the writer produced, ready to pack.
pub(super) struct Output {
    pub document: String,
    pub footnotes: String,
    pub doc_rels: RelSet,
    pub foot_rels: RelSet,
    pub media: Vec<(String, Vec<u8>)>,
    pub styles: String,
    pub numbering: String,
    pub core: String,
}

/// Pack the package, deterministically: fixed entry order, fixed timestamps.
///
/// # Errors
///
/// [`Error::Docx`] when zipping fails.
pub(super) fn pack(base: Base, mut out: Output) -> Result<Vec<u8>> {
    let mut parts: BTreeMap<String, Vec<u8>> = base.parts;
    // Document relationships every package needs.
    out.doc_rels.add(rel_type::STYLES, "styles.xml", false);
    out.doc_rels
        .add(rel_type::NUMBERING, "numbering.xml", false);
    out.doc_rels
        .add(rel_type::FOOTNOTES, "footnotes.xml", false);
    if parts.contains_key("word/settings.xml") && !out.doc_rels.has_kind(rel_type::SETTINGS) {
        out.doc_rels.add(rel_type::SETTINGS, "settings.xml", false);
    }
    parts.insert("word/document.xml".into(), out.document.into_bytes());
    parts.insert(
        "word/_rels/document.xml.rels".into(),
        out.doc_rels.to_xml().into_bytes(),
    );
    parts.insert("word/styles.xml".into(), out.styles.into_bytes());
    parts.insert("word/numbering.xml".into(), out.numbering.into_bytes());
    parts.insert("word/footnotes.xml".into(), out.footnotes.into_bytes());
    if !out.foot_rels.rels.is_empty() {
        parts.insert(
            "word/_rels/footnotes.xml.rels".into(),
            out.foot_rels.to_xml().into_bytes(),
        );
    }
    for (name, data) in out.media {
        parts.insert(name, data);
    }
    parts.insert("docProps/core.xml".into(), out.core.into_bytes());
    if !parts.contains_key("docProps/app.xml") {
        parts.insert("docProps/app.xml".into(), DEFAULT_APP.as_bytes().to_vec());
    }

    // Package relationships.
    let mut root = RelSet::new(base.root_rels);
    root.add(rel_type::OFFICE_DOCUMENT, "word/document.xml", false);
    root.add(rel_type::CORE, "docProps/core.xml", false);
    root.add(rel_type::APP, "docProps/app.xml", false);
    parts.insert("_rels/.rels".into(), root.to_xml().into_bytes());

    // Content types.
    let mut defaults = base.ct_defaults;
    defaults.insert("rels".into(), CT_RELS.into());
    defaults.insert("xml".into(), "application/xml".into());
    for name in parts.keys() {
        if let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) {
            if let Some(ct) = media_type(&ext) {
                defaults.entry(ext).or_insert_with(|| ct.to_string());
            }
        }
    }
    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    for name in parts.keys() {
        let part = format!("/{name}");
        let ct = match name.as_str() {
            "word/document.xml" => Some(CT_MAIN.to_string()),
            "word/styles.xml" => Some(CT_STYLES.to_string()),
            "word/numbering.xml" => Some(CT_NUMBERING.to_string()),
            "word/footnotes.xml" => Some(CT_FOOTNOTES.to_string()),
            "word/settings.xml" => Some(CT_SETTINGS.to_string()),
            "docProps/core.xml" => Some(CT_CORE.to_string()),
            "docProps/app.xml" => Some(CT_APP.to_string()),
            _ => base.ct_overrides.get(&part).cloned(),
        };
        if let Some(ct) = ct {
            overrides.insert(part, ct);
        }
    }
    let mut ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
    );
    for (ext, kind) in &defaults {
        ct.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            escape_attr(ext),
            escape_attr(kind)
        ));
    }
    for (part, kind) in &overrides {
        ct.push_str(&format!(
            "<Override PartName=\"{}\" ContentType=\"{}\"/>",
            escape_attr(part),
            escape_attr(kind)
        ));
    }
    ct.push_str("</Types>");

    // Zip: content types and package rels first, then the rest by name.
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o644);
        let mut write = |name: &str, data: &[u8]| -> Result<()> {
            zip.start_file(name, options)
                .map_err(|e| Error::Docx(e.to_string()))?;
            zip.write_all(data).map_err(|e| Error::Docx(e.to_string()))
        };
        write("[Content_Types].xml", ct.as_bytes())?;
        if let Some(rels) = parts.remove("_rels/.rels") {
            write("_rels/.rels", &rels)?;
        }
        for (name, data) in &parts {
            write(name, data)?;
        }
        zip.finish().map_err(|e| Error::Docx(e.to_string()))?;
    }
    Ok(buffer.into_inner())
}

/// The media type of an image extension.
fn media_type(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => return None,
    })
}

/// `docProps/core.xml` from the document's metadata. No timestamp: the output must be
/// the same bytes on every run.
pub(super) fn core_properties(
    title: Option<&str>,
    subject: Option<&str>,
    authors: &[String],
    keywords: &[String],
    description: Option<&str>,
    lang: Option<&str>,
) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
    );
    let mut field = |tag: &str, value: Option<&str>| {
        if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
            s.push_str(&format!("<{tag}>{}</{tag}>", escape_text(v.trim())));
        }
    };
    field("dc:title", title);
    field("dc:subject", subject);
    let creators = authors.join("; ");
    field("dc:creator", Some(&creators));
    let keywords = keywords.join(", ");
    field("cp:keywords", Some(&keywords));
    field("dc:description", description);
    field("dc:language", lang);
    s.push_str("</cp:coreProperties>");
    s
}

const DEFAULT_SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/><w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat><m:mathPr><m:mathFont m:val="Cambria Math"/><m:dispDef/><m:defJc m:val="centerGroup"/></m:mathPr></w:settings>"#;

const DEFAULT_APP: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><Application>md2star-rs</Application><DocSecurity>0</DocSecurity></Properties>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationship_ids_never_collide() {
        let mut set = RelSet::new(vec![Rel {
            id: "rId7".into(),
            kind: rel_type::STYLES.into(),
            target: "styles.xml".into(),
            external: false,
        }]);
        assert_eq!(set.add(rel_type::HYPERLINK, "https://a", true), "rId8");
        assert_eq!(
            set.add(rel_type::HYPERLINK, "https://a", true),
            "rId8",
            "deduplicated"
        );
        assert_eq!(set.add(rel_type::STYLES, "styles.xml", false), "rId7");
        let parsed = parse_rels(&set.to_xml());
        assert_eq!(parsed, set.rels);
    }

    #[test]
    fn the_final_section_is_found() {
        let doc = r#"<w:body><w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p><w:sectPr w:rsidR="00"><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720"/></w:sectPr></w:body>"#;
        let s = final_sect_pr(doc).unwrap();
        assert!(s.starts_with("<w:sectPr w:rsidR"));
        let page = page_of(&s);
        assert_eq!(page.text_width, 12240 - 1440);
        assert_eq!(page.text_height, 15840 - 1440);
        assert_eq!(
            final_sect_pr("<w:body><w:sectPr/></w:body>").unwrap(),
            "<w:sectPr/>"
        );
    }

    #[test]
    fn numbering_is_merged_in_schema_order() {
        let xml = r#"<w:numbering><w:abstractNum w:abstractNumId="3"/><w:num w:numId="5"><w:abstractNumId w:val="3"/></w:num></w:numbering>"#;
        assert_eq!(numbering_max(xml), (3, 5));
        let merged = merge_numbering(Some(xml), "<A/>", "<N/>");
        assert_eq!(
            merged,
            r#"<w:numbering><w:abstractNum w:abstractNumId="3"/><A/><w:num w:numId="5"><w:abstractNumId w:val="3"/></w:num><N/></w:numbering>"#
        );
    }

    #[test]
    fn targets_resolve_against_their_part() {
        assert_eq!(
            resolve("word/", "media/image1.png"),
            "word/media/image1.png"
        );
        assert_eq!(
            resolve("word/", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(resolve("word/", "/word/media/x.png"), "word/media/x.png");
        assert_eq!(
            rels_source_dir("word/_rels/header1.xml.rels").as_deref(),
            Some("word/")
        );
        assert_eq!(rels_source_dir("word/header1.xml"), None);
    }

    #[test]
    fn settings_footnote_separators() {
        assert_eq!(separator_ids(DEFAULT_SETTINGS), (-1, 0));
        assert_eq!(
            separator_ids(
                "<w:footnotePr><w:footnote w:id=\"0\"/><w:footnote w:id=\"1\"/></w:footnotePr>"
            ),
            (0, 1)
        );
        assert_eq!(separator_ids(""), (-1, 0));
    }
}
