//! Citation and reference-list rendering (see [`crate::bib`] for the styles).

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{Bibliography, Entry, Name, Style};
use crate::ast::{
    cite_text, Attr, Block, Citation, CitationMode, Document, Inline, MetaValue, Warning,
};

/// See [`crate::bib::apply`].
pub(super) fn apply(
    doc: &Document,
    bib: &Bibliography,
    style: Style,
    title: &str,
) -> (Document, Vec<Warning>) {
    let locale = Locale::for_lang(doc.meta.lang.as_deref());
    // 1. Every cited key, in order of first citation (plus `nocite`).
    let mut order: Vec<String> = Vec::new();
    collect_blocks(&doc.blocks, &mut order);
    let mut seen: HashSet<String> = HashSet::new();
    order.retain(|k| seen.insert(k.clone()));
    for key in nocite(doc) {
        if key == "*" {
            for e in &bib.entries {
                if seen.insert(e.key.clone()) {
                    order.push(e.key.clone());
                }
            }
        } else if seen.insert(key.clone()) {
            order.push(key);
        }
    }
    let mut warnings = Vec::new();
    let mut entries: Vec<&Entry> = Vec::new();
    for key in &order {
        match bib.get(key) {
            Some(e) => entries.push(e),
            None => warnings.push(Warning::new(
                "citation-not-found",
                format!("citation key `{key}` is not in the bibliography: left as written"),
            )),
        }
    }
    let formatter = Formatter::new(style, entries, locale);
    // 2. Rewrite the citations.
    let mut out = doc.clone();
    out.blocks = rewrite_blocks(&doc.blocks, &formatter);
    // 3. The reference list.
    let list = formatter.reference_list();
    if !list.is_empty() || !title.is_empty() {
        let refs_div = Block::Div {
            attr: Attr {
                id: Some("refs".into()),
                classes: vec!["references".into(), "csl-bib-body".into()],
                kv: Vec::new(),
            },
            content: list,
        };
        if !place_in_refs_div(&mut out.blocks, &refs_div) {
            if !title.is_empty() {
                out.blocks.push(Block::Heading {
                    level: 1,
                    content: vec![Inline::Text(title.to_string())],
                    attr: Attr {
                        id: Some("bibliography".into()),
                        classes: vec!["unnumbered".into()],
                        kv: Vec::new(),
                    },
                });
            }
            out.blocks.push(refs_div);
        }
    }
    (out, warnings)
}

/// Keys listed in the front matter's `nocite` (`'@*'` or `'@a, @b'`).
fn nocite(doc: &Document) -> Vec<String> {
    let mut keys = Vec::new();
    let mut push = |s: &str| {
        for part in s.split([',', ';', ' ']) {
            if let Some(k) = part.trim().strip_prefix('@') {
                if !k.is_empty() {
                    keys.push(k.to_string());
                }
            }
        }
    };
    match doc.meta.raw.get("nocite") {
        Some(MetaValue::String(s)) => push(s),
        Some(MetaValue::List(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    push(&s);
                }
            }
        }
        _ => {}
    }
    keys
}

/// Replace the content of an existing `::: {#refs}` div; `false` when there is none.
fn place_in_refs_div(blocks: &mut [Block], refs: &Block) -> bool {
    for block in blocks.iter_mut() {
        if matches!(block, Block::Div { attr, .. } if attr.id.as_deref() == Some("refs")) {
            *block = refs.clone();
            return true;
        }
        let inner = match block {
            Block::Div { content, .. } | Block::BlockQuote(content) => content,
            _ => continue,
        };
        if place_in_refs_div(inner, refs) {
            return true;
        }
    }
    false
}

fn collect_blocks(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph(content) => {
                collect_inlines(content, out);
            }
            Block::Figure(image) => collect_inlines(&image.alt, out),
            Block::BlockQuote(inner) | Block::Notes(inner) => collect_blocks(inner, out),
            Block::Div { content, .. } | Block::FootnoteDef { content, .. } => {
                collect_blocks(content, out);
            }
            Block::List { items, .. } => {
                for item in items {
                    collect_blocks(&item.content, out);
                }
            }
            Block::Table {
                headers,
                rows,
                caption,
                ..
            } => {
                for cell in headers.iter().chain(rows.iter().flatten()) {
                    collect_inlines(cell, out);
                }
                collect_inlines(caption, out);
            }
            Block::CodeBlock { .. }
            | Block::Mermaid { .. }
            | Block::ThematicBreak
            | Block::RawHtml(_) => {}
        }
    }
}

fn collect_inlines(inlines: &[Inline], out: &mut Vec<String>) {
    for inline in inlines {
        match inline {
            Inline::Cite(cites) => out.extend(cites.iter().map(|c| c.key.clone())),
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::InlineNote(inner) => collect_inlines(inner, out),
            Inline::Link { text, .. } => collect_inlines(text, out),
            Inline::Image(image) => collect_inlines(&image.alt, out),
            _ => {}
        }
    }
}

fn rewrite_blocks(blocks: &[Block], f: &Formatter) -> Vec<Block> {
    blocks.iter().map(|b| rewrite_block(b, f)).collect()
}

fn rewrite_block(block: &Block, f: &Formatter) -> Block {
    match block {
        Block::Heading {
            level,
            content,
            attr,
        } => Block::Heading {
            level: *level,
            content: rewrite_inlines(content, f),
            attr: attr.clone(),
        },
        Block::Paragraph(content) => Block::Paragraph(rewrite_inlines(content, f)),
        Block::Figure(image) => {
            let mut image = image.clone();
            image.alt = rewrite_inlines(&image.alt, f);
            Block::Figure(image)
        }
        Block::BlockQuote(inner) => Block::BlockQuote(rewrite_blocks(inner, f)),
        Block::Notes(inner) => Block::Notes(rewrite_blocks(inner, f)),
        Block::Div { attr, content } => Block::Div {
            attr: attr.clone(),
            content: rewrite_blocks(content, f),
        },
        Block::FootnoteDef { label, content } => Block::FootnoteDef {
            label: label.clone(),
            content: rewrite_blocks(content, f),
        },
        Block::List {
            ordered,
            start,
            items,
        } => Block::List {
            ordered: *ordered,
            start: *start,
            items: items
                .iter()
                .map(|item| crate::ast::ListItem {
                    task: item.task,
                    content: rewrite_blocks(&item.content, f),
                })
                .collect(),
        },
        Block::Table {
            headers,
            rows,
            aligns,
            caption,
            attr,
        } => Block::Table {
            headers: headers.iter().map(|c| rewrite_inlines(c, f)).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| rewrite_inlines(c, f)).collect())
                .collect(),
            aligns: aligns.clone(),
            caption: rewrite_inlines(caption, f),
            attr: attr.clone(),
        },
        other => other.clone(),
    }
}

fn rewrite_inlines(inlines: &[Inline], f: &Formatter) -> Vec<Inline> {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::Cite(cites) => out.extend(f.citation(cites)),
            Inline::Emph(inner) => out.push(Inline::Emph(rewrite_inlines(inner, f))),
            Inline::Strong(inner) => out.push(Inline::Strong(rewrite_inlines(inner, f))),
            Inline::Strikeout(inner) => out.push(Inline::Strikeout(rewrite_inlines(inner, f))),
            Inline::Superscript(inner) => {
                out.push(Inline::Superscript(rewrite_inlines(inner, f)));
            }
            Inline::Subscript(inner) => out.push(Inline::Subscript(rewrite_inlines(inner, f))),
            Inline::InlineNote(inner) => out.push(Inline::InlineNote(rewrite_inlines(inner, f))),
            Inline::Link {
                text,
                url,
                title,
                attr,
            } => out.push(Inline::Link {
                text: rewrite_inlines(text, f),
                url: url.clone(),
                title: title.clone(),
                attr: attr.clone(),
            }),
            other => out.push(other.clone()),
        }
    }
    out
}

/// The words a style needs, per language.
#[derive(Debug, Clone, Copy)]
struct Locale {
    and: &'static str,
    et_al: &'static str,
    ed: &'static str,
    edited_by: &'static str,
    eds: &'static str,
    in_: &'static str,
    phd: &'static str,
    masters: &'static str,
    no_date: &'static str,
    accessed: &'static str,
    online: &'static str,
    available: &'static str,
    months: [&'static str; 12],
}

impl Locale {
    fn for_lang(lang: Option<&str>) -> Locale {
        let prefix = lang
            .and_then(|l| l.split(['-', '_']).next())
            .unwrap_or("en")
            .to_ascii_lowercase();
        let en = Locale {
            and: "and",
            et_al: "et al.",
            ed: "ed.",
            edited_by: "edited by",
            eds: "Ed.",
            in_: "In",
            phd: "PhD thesis",
            masters: "Master’s thesis",
            no_date: "n.d.",
            accessed: "Accessed",
            online: "[Online]",
            available: "Available",
            months: [
                "Jan.", "Feb.", "Mar.", "Apr.", "May", "Jun.", "Jul.", "Aug.", "Sep.", "Oct.",
                "Nov.", "Dec.",
            ],
        };
        match prefix.as_str() {
            "fr" => Locale {
                and: "et",
                ed: "éd.",
                edited_by: "sous la direction de",
                eds: "éd.",
                in_: "In",
                phd: "Thèse de doctorat",
                masters: "Mémoire de master",
                no_date: "s. d.",
                accessed: "Consulté le",
                online: "[En ligne]",
                available: "Disponible sur",
                months: [
                    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juill.", "août", "sept.",
                    "oct.", "nov.", "déc.",
                ],
                ..en
            },
            "es" => Locale { and: "y", ..en },
            "de" => Locale { and: "und", ..en },
            "it" | "pt" => Locale { and: "e", ..en },
            "nl" => Locale { and: "en", ..en },
            "ru" => Locale { and: "и", ..en },
            _ => en,
        }
    }
}

/// Renders citations and the reference list for one document.
struct Formatter<'a> {
    style: Style,
    locale: Locale,
    /// Entries in reference-list order.
    entries: Vec<&'a Entry>,
    /// key → index in `entries` (also the number − 1 in the numeric style).
    index: HashMap<&'a str, usize>,
    /// key → the year suffix (`a`, `b`) that tells same-author same-year works apart.
    suffix: HashMap<&'a str, char>,
}

impl<'a> Formatter<'a> {
    fn new(style: Style, mut entries: Vec<&'a Entry>, locale: Locale) -> Self {
        if style == Style::AuthorDate {
            entries.sort_by_cached_key(|e| sort_key(e));
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.as_str(), i))
            .collect();
        let mut suffix = HashMap::new();
        if style == Style::AuthorDate {
            let mut groups: BTreeMap<(String, String), Vec<&'a str>> = BTreeMap::new();
            for e in &entries {
                groups
                    .entry((short_authors(e, &locale), raw_year(e)))
                    .or_default()
                    .push(e.key.as_str());
            }
            for keys in groups.values().filter(|k| k.len() > 1) {
                for (i, key) in keys.iter().enumerate() {
                    if let Some(c) = char::from_u32(u32::from(b'a') + i as u32) {
                        suffix.insert(*key, c);
                    }
                }
            }
        }
        Formatter {
            style,
            locale,
            entries,
            index,
            suffix,
        }
    }

    fn year(&self, e: &Entry) -> String {
        let mut y = raw_year(e);
        if y.is_empty() {
            y = self.locale.no_date.to_string();
        }
        if let Some(c) = self.suffix.get(e.key.as_str()) {
            y.push(*c);
        }
        y
    }

    /// The inlines that replace one citation group.
    fn citation(&self, cites: &[Citation]) -> Vec<Inline> {
        if cites
            .iter()
            .any(|c| !self.index.contains_key(c.key.as_str()))
        {
            return vec![Inline::Strong(vec![Inline::Text(cite_text(cites))])];
        }
        let text = match self.style {
            Style::AuthorDate => self.author_date_citation(cites),
            Style::Numeric => self.numeric_citation(cites),
        };
        vec![Inline::Text(text)]
    }

    fn entry(&self, key: &str) -> &'a Entry {
        self.entries[self.index[key]]
    }

    fn author_date_citation(&self, cites: &[Citation]) -> String {
        // In-text citations stand alone: "Pearl (2009, chap. 2)".
        if cites.iter().all(|c| c.mode == CitationMode::AuthorInText) {
            let parts: Vec<String> = cites
                .iter()
                .map(|c| {
                    let e = self.entry(&c.key);
                    let mut inner = self.year(e);
                    let loc = chicago_locator(&c.suffix);
                    if !loc.is_empty() {
                        inner.push_str(", ");
                        inner.push_str(&loc);
                    }
                    format!("{} ({inner})", short_authors(e, &self.locale))
                })
                .collect();
            return parts.join("; ");
        }
        let mut items: Vec<String> = Vec::new();
        let mut last_author: Option<String> = None;
        for c in cites {
            let e = self.entry(&c.key);
            let author = short_authors(e, &self.locale);
            let loc = chicago_locator(&c.suffix);
            let mut year = self.year(e);
            if !loc.is_empty() {
                year.push_str(", ");
                year.push_str(&loc);
            }
            let suppress = c.mode == CitationMode::SuppressAuthor;
            // Same author as the previous item, nothing in between: "Pearl 2009, 2000".
            if c.prefix.is_empty()
                && !suppress
                && last_author.as_deref() == Some(author.as_str())
                && !items.is_empty()
            {
                if let Some(prev) = items.last_mut() {
                    prev.push_str(", ");
                    prev.push_str(&year);
                }
                continue;
            }
            let mut item = String::new();
            if !c.prefix.is_empty() {
                item.push_str(&c.prefix);
                item.push(' ');
            }
            if suppress {
                item.push_str(&year);
                last_author = None;
            } else {
                item.push_str(&author);
                item.push(' ');
                item.push_str(&year);
                last_author = Some(author);
            }
            items.push(item);
        }
        format!("({})", items.join("; "))
    }

    fn numeric_citation(&self, cites: &[Citation]) -> String {
        let items: Vec<String> = cites
            .iter()
            .map(|c| {
                let n = self.index[c.key.as_str()] + 1;
                let mut item = String::new();
                if !c.prefix.is_empty() {
                    item.push_str(&c.prefix);
                    item.push(' ');
                }
                if c.suffix.is_empty() {
                    item.push_str(&format!("[{n}]"));
                } else {
                    item.push_str(&format!("[{n}, {}]", c.suffix.trim()));
                }
                item
            })
            .collect();
        items.join(", ")
    }

    /// One `Div {#ref-KEY .csl-entry custom-style=Bibliography}` per entry.
    fn reference_list(&self) -> Vec<Block> {
        let mut previous_names: Option<String> = None;
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let inlines = match self.style {
                    Style::AuthorDate => {
                        let names = chicago_names(&e.authors, &e.editors, &self.locale);
                        let repeated = !names.is_empty() && previous_names.as_ref() == Some(&names);
                        previous_names = Some(names.clone());
                        self.chicago_entry(e, if repeated { "———" } else { &names })
                    }
                    Style::Numeric => self.ieee_entry(e, i + 1),
                };
                Block::Div {
                    attr: Attr {
                        id: Some(format!("ref-{}", e.key)),
                        classes: vec!["csl-entry".into()],
                        kv: vec![("custom-style".into(), "Bibliography".into())],
                    },
                    content: vec![Block::Paragraph(inlines)],
                }
            })
            .collect()
    }

    fn chicago_entry(&self, e: &Entry, names: &str) -> Vec<Inline> {
        let l = &self.locale;
        let mut o = Out::default();
        let title = e.field("title").unwrap_or(&e.key);
        if names.is_empty() {
            // No author: the title leads.
            o.title(e, title, true);
            o.space();
            o.text(&self.year(e));
            o.period();
        } else {
            o.text(names);
            o.period();
            o.space();
            o.text(&self.year(e));
            o.period();
            o.space();
            o.title(e, title, true);
        }
        let kind = e.kind.as_str();
        match kind {
            "article" => {
                if let Some(j) = e.field("journal").or_else(|| e.field("journaltitle")) {
                    o.space();
                    o.emph(j);
                    if let Some(v) = e.field("volume") {
                        o.text(&format!(" {v}"));
                    }
                    if let Some(n) = e.field("number").or_else(|| e.field("issue")) {
                        o.text(&format!(" ({n})"));
                    }
                    if let Some(p) = e.field("pages") {
                        o.text(&format!(": {p}"));
                    }
                    o.period();
                }
            }
            "inproceedings" | "conference" | "incollection" | "inbook" => {
                if let Some(b) = e.field("booktitle") {
                    o.space();
                    o.text(l.in_);
                    o.space();
                    o.emph(b);
                    if !e.editors.is_empty() && !e.authors.is_empty() {
                        o.text(&format!(", {} {}", l.edited_by, plain_names(&e.editors, l)));
                    }
                    if let Some(p) = e.field("pages") {
                        o.text(&format!(", {p}"));
                    }
                    o.period();
                }
                publisher_block(&mut o, e);
            }
            "phdthesis" | "mastersthesis" | "thesis" => {
                o.space();
                o.text(if kind == "mastersthesis" {
                    l.masters
                } else {
                    l.phd
                });
                if let Some(s) = e.field("school").or_else(|| e.field("institution")) {
                    o.text(&format!(", {s}"));
                }
                o.period();
            }
            "techreport" | "report" => {
                if let Some(n) = e.field("number") {
                    o.space();
                    o.text(n);
                    o.period();
                }
                if let Some(i) = e.field("institution") {
                    o.space();
                    o.text(i);
                    o.period();
                }
            }
            _ if is_book_like(kind) => {
                if let Some(ed) = e.field("edition") {
                    o.space();
                    o.text(&format!("{} {}", ordinal_edition(ed), l.ed));
                }
                if !e.editors.is_empty() && !e.authors.is_empty() {
                    o.space();
                    o.text(&format!(
                        "{} {}",
                        capitalise(l.edited_by),
                        plain_names(&e.editors, l)
                    ));
                    o.period();
                }
                publisher_block(&mut o, e);
            }
            _ => {
                if let Some(h) = e.field("howpublished").or_else(|| e.field("note")) {
                    if !h.starts_with("\\url") && !h.starts_with("http") {
                        o.space();
                        o.text(h);
                        o.period();
                    }
                }
                publisher_block(&mut o, e);
            }
        }
        if let Some(doi) = e.field("doi") {
            o.space();
            o.link(&doi_url(doi));
            o.period();
        } else if let Some(url) = e.field("url") {
            o.space();
            o.link(url);
            o.period();
        }
        o.finish()
    }

    fn ieee_entry(&self, e: &Entry, n: usize) -> Vec<Inline> {
        let l = &self.locale;
        let mut o = Out::default();
        o.text(&format!("[{n}] "));
        let names = ieee_names(
            if e.authors.is_empty() {
                &e.editors
            } else {
                &e.authors
            },
            l,
        );
        if !names.is_empty() {
            o.text(&names);
            o.text(", ");
        }
        let title = e.field("title").unwrap_or(&e.key);
        let year = raw_year(e);
        let kind = e.kind.as_str();
        // Each kind ends where its date goes; doi/url follow.
        match kind {
            "article" => {
                o.quoted_comma(title);
                if let Some(j) = e.field("journal").or_else(|| e.field("journaltitle")) {
                    o.space();
                    o.emph(j);
                    o.text(",");
                }
                if let Some(v) = e.field("volume") {
                    o.text(&format!(" vol. {v},"));
                }
                if let Some(num) = e.field("number").or_else(|| e.field("issue")) {
                    o.text(&format!(" no. {num},"));
                }
                if let Some(p) = e.field("pages") {
                    o.text(&format!(" {} {p},", pages_label(p)));
                }
                o.space();
                o.text(&year);
            }
            "inproceedings" | "conference" | "incollection" | "inbook" => {
                o.quoted_comma(title);
                if let Some(b) = e.field("booktitle") {
                    o.text(" in ");
                    o.emph(b);
                    o.text(",");
                }
                if !e.editors.is_empty() && !e.authors.is_empty() {
                    o.text(&format!(" {}, {},", ieee_names(&e.editors, l), l.eds));
                }
                if let Some(p) = publisher_text(e) {
                    o.text(&format!(" {p},"));
                }
                o.space();
                o.text(&year);
                if let Some(p) = e.field("pages") {
                    o.text(&format!(", {} {p}", pages_label(p)));
                }
            }
            "phdthesis" | "mastersthesis" | "thesis" => {
                o.quoted_comma(title);
                o.space();
                o.text(if kind == "mastersthesis" {
                    l.masters
                } else {
                    l.phd
                });
                if let Some(s) = e.field("school").or_else(|| e.field("institution")) {
                    o.text(&format!(", {s}"));
                }
                o.text(&format!(", {year}"));
            }
            "techreport" | "report" => {
                o.quoted_comma(title);
                if let Some(i) = e.field("institution") {
                    o.text(&format!(" {i},"));
                }
                if let Some(num) = e.field("number") {
                    o.text(&format!(" {num},"));
                }
                o.space();
                o.text(&year);
            }
            _ if is_book_like(kind) => {
                o.emph(title);
                if let Some(ed) = e.field("edition") {
                    o.text(&format!(", {} {}", ordinal_edition(ed), l.ed));
                }
                o.period();
                match publisher_text(e) {
                    Some(p) => {
                        o.text(&format!(" {p}, {year}"));
                    }
                    None => {
                        o.space();
                        o.text(&year);
                    }
                }
            }
            _ => {
                o.text("“");
                o.text(title);
                o.period();
                o.text("”");
                o.space();
                o.text(&year);
            }
        }
        let has_doi = e.field("doi").is_some();
        if let Some(doi) = e.field("doi") {
            o.text(", doi: ");
            o.text(doi);
        }
        o.period();
        if !has_doi {
            if let Some(url) = e.field("url") {
                if let Some(date) = e.field("urldate").and_then(|d| ieee_date(d, l)) {
                    o.text(&format!(" {}: {date}.", l.accessed));
                }
                o.text(&format!(" {}. {}: ", l.online, l.available));
                o.link(url);
            }
        }
        o.finish()
    }
}

/// Book-like entry kinds (italic title, publisher).
fn is_book_like(kind: &str) -> bool {
    matches!(
        kind,
        "book" | "booklet" | "manual" | "proceedings" | "collection" | "mvbook" | "periodical"
    )
}

/// `pp.` for a range, `p.` for one page.
fn pages_label(pages: &str) -> &'static str {
    if pages.contains(['–', '-', ',']) {
        "pp."
    } else {
        "p."
    }
}

/// `Address: Publisher`, or just the publisher.
fn publisher_text(e: &Entry) -> Option<String> {
    let publisher = e.field("publisher").or_else(|| e.field("organization"))?;
    Some(match e.field("address").or_else(|| e.field("location")) {
        Some(addr) => format!("{addr}: {publisher}"),
        None => publisher.to_string(),
    })
}

fn publisher_block(o: &mut Out, e: &Entry) {
    if let Some(p) = publisher_text(e) {
        o.space();
        o.text(&p);
        o.period();
    }
}

/// `2` → `2nd`; `2nd`, `Second` kept.
fn ordinal_edition(ed: &str) -> String {
    match ed.trim().parse::<u32>() {
        Ok(n) => {
            let suffix = match (n % 10, n % 100) {
                (1, x) if x != 11 => "st",
                (2, x) if x != 12 => "nd",
                (3, x) if x != 13 => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
        Err(_) => ed.trim().to_string(),
    }
}

fn doi_url(doi: &str) -> String {
    if doi.starts_with("http") {
        doi.to_string()
    } else {
        format!("https://doi.org/{doi}")
    }
}

/// `2026-02-21` → `Feb. 21, 2026` (`21 févr. 2026` in French).
fn ieee_date(date: &str, l: &Locale) -> Option<String> {
    let mut parts = date.trim().split('-');
    let year = parts.next()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let name = l.months.get(month.checked_sub(1)?)?;
    Some(if l.and == "et" {
        format!("{day} {name} {year}")
    } else {
        format!("{name} {day}, {year}")
    })
}

/// The year of an entry: `year`, else the year of `date`; empty when neither.
fn raw_year(e: &Entry) -> String {
    if let Some(y) = e.field("year") {
        return y.to_string();
    }
    e.field("date")
        .map(|d| d.chars().take_while(|c| c.is_ascii_digit()).collect())
        .unwrap_or_default()
}

/// Chicago drops the page label (`p. 3` → `3`) and keeps the others (`chap. 2`).
fn chicago_locator(suffix: &str) -> String {
    let s = suffix.trim().trim_start_matches(',').trim();
    for label in ["pp. ", "pp.", "p. ", "p.", "pages ", "page "] {
        if let Some(rest) = s.strip_prefix(label) {
            if rest.starts_with(|c: char| c.is_ascii_digit() || c.is_ascii_alphabetic()) {
                return rest.trim().to_string();
            }
        }
    }
    s.to_string()
}

/// Family name with its particle when the particle is capitalised (kept by citeproc).
fn family(n: &Name) -> String {
    n.family.clone()
}

/// The author part of an author-date citation: `Pearl`, `Golub and Loan`,
/// `Goodfellow, Bengio, and Courville`, `Bouveyron et al.`.
fn short_authors(e: &Entry, l: &Locale) -> String {
    let names: &[Name] = if e.authors.is_empty() {
        &e.editors
    } else {
        &e.authors
    };
    let real: Vec<&Name> = names.iter().filter(|n| !n.others).collect();
    let others = names.iter().any(|n| n.others);
    if real.is_empty() {
        return e
            .field("title")
            .map(|t| format!("“{t}”"))
            .unwrap_or_else(|| e.key.clone());
    }
    if real.len() >= 4 || others {
        return format!("{} {}", family(real[0]), l.et_al);
    }
    let fams: Vec<String> = real.iter().map(|n| family(n)).collect();
    join_list(&fams, l.and, true)
}

/// `A`, `A and B`, `A, B, and C` (serial comma, as Chicago and IEEE in English).
fn join_list(items: &[String], and: &str, serial: bool) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} {and} {b}"),
        [init @ .., last] => {
            let comma = if serial && and == "and" { "," } else { "" };
            format!("{}{comma} {and} {last}", init.join(", "))
        }
    }
}

/// `Pearl, Judea`, `Prado, Marcos Lopez de`, `Ford, Henry, Jr.`.
fn inverted(n: &Name) -> String {
    let mut s = n.family.clone();
    let mut given = n.given.clone();
    if !n.particle.is_empty() {
        if !given.is_empty() {
            given.push(' ');
        }
        given.push_str(&n.particle);
    }
    if !given.is_empty() {
        s.push_str(", ");
        s.push_str(&given);
    }
    if !n.suffix.is_empty() {
        s.push_str(", ");
        s.push_str(&n.suffix);
    }
    s
}

/// `Judea Pearl`, `Marcos Lopez de Prado`.
fn direct(n: &Name) -> String {
    let parts: Vec<&str> = [n.given.as_str(), n.particle.as_str(), n.family.as_str()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    let mut s = parts.join(" ");
    if !n.suffix.is_empty() {
        s.push_str(", ");
        s.push_str(&n.suffix);
    }
    s
}

/// The reference-list names (Chicago): first inverted, the rest direct, serial "and",
/// more than ten → seven and "et al.". Editors stand in (with `ed.`) for a work without
/// authors.
fn chicago_names(authors: &[Name], editors: &[Name], l: &Locale) -> String {
    let (names, edited) = if authors.is_empty() {
        (editors, true)
    } else {
        (authors, false)
    };
    let real: Vec<&Name> = names.iter().filter(|n| !n.others).collect();
    if real.is_empty() {
        return String::new();
    }
    let others = names.iter().any(|n| n.others) || real.len() > 10;
    let shown = if real.len() > 10 {
        &real[..7]
    } else {
        &real[..]
    };
    let mut parts: Vec<String> = Vec::new();
    for (i, n) in shown.iter().enumerate() {
        parts.push(if i == 0 { inverted(n) } else { direct(n) });
    }
    let mut s = if others {
        format!("{}, {}", parts.join(", "), l.et_al)
    } else if parts.len() == 2 {
        format!("{}, {} {}", parts[0], l.and, parts[1])
    } else {
        join_list(&parts, l.and, true)
    };
    if edited {
        s.push_str(", ");
        s.push_str(l.ed);
    }
    s
}

/// Names in running text (`Ann Smith and Bo Lee`).
fn plain_names(names: &[Name], l: &Locale) -> String {
    let parts: Vec<String> = names.iter().filter(|n| !n.others).map(direct).collect();
    join_list(&parts, l.and, true)
}

/// `J. Pearl`, `C. F. V. Loan`, `J.-P. Sartre`, `M. L. de Prado` (IEEE); more than six
/// → the first and "et al.".
fn ieee_names(names: &[Name], l: &Locale) -> String {
    let real: Vec<&Name> = names.iter().filter(|n| !n.others).collect();
    if real.is_empty() {
        return String::new();
    }
    let one = |n: &Name| {
        let initials: Vec<String> = n
            .given
            .split_whitespace()
            .map(|word| {
                word.split('-')
                    .filter_map(|part| part.chars().find(|c| c.is_alphabetic()))
                    .map(|c| format!("{c}."))
                    .collect::<Vec<_>>()
                    .join("-")
            })
            .filter(|s| !s.is_empty())
            .collect();
        let mut s = String::new();
        for p in [initials.join(" "), n.particle.clone(), n.family.clone()] {
            if !p.is_empty() {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&p);
            }
        }
        s
    };
    if real.len() > 6 || names.iter().any(|n| n.others) {
        return format!("{} {}", one(real[0]), l.et_al);
    }
    let parts: Vec<String> = real.iter().map(|n| one(n)).collect();
    join_list(&parts, l.and, true)
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The reference-list sort key: first author's family, given, year, title.
fn sort_key(e: &Entry) -> (String, String, String, String) {
    let names: &[Name] = if e.authors.is_empty() {
        &e.editors
    } else {
        &e.authors
    };
    let fold = |s: &str| fold_accents(&s.to_lowercase());
    let title = fold(e.field("title").unwrap_or(""));
    match names.iter().find(|n| !n.others) {
        Some(n) => (fold(&n.family), fold(&n.given), raw_year(e), title),
        None => (title.clone(), String::new(), raw_year(e), title),
    }
}

/// Strip the common Latin accents so `É` sorts with `E`.
fn fold_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'š' | 'ś' | 'ş' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            'ř' => 'r',
            'ł' => 'l',
            'ğ' => 'g',
            other => other,
        })
        .collect()
}

/// Builds a run of inlines with punctuation that never doubles.
#[derive(Default)]
struct Out {
    inlines: Vec<Inline>,
    /// The visible text so far (to decide punctuation).
    tail: String,
}

impl Out {
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.tail.push_str(s);
        if let Some(Inline::Text(t)) = self.inlines.last_mut() {
            t.push_str(s);
        } else {
            self.inlines.push(Inline::Text(s.to_string()));
        }
    }

    fn emph(&mut self, s: &str) {
        self.tail.push_str(s);
        self.inlines
            .push(Inline::Emph(vec![Inline::Text(s.to_string())]));
    }

    fn link(&mut self, url: &str) {
        self.tail.push_str(url);
        self.inlines.push(Inline::Link {
            text: vec![Inline::Text(url.to_string())],
            url: url.to_string(),
            title: None,
            attr: Attr::default(),
        });
    }

    fn space(&mut self) {
        if !self.tail.is_empty() && !self.tail.ends_with(' ') {
            self.text(" ");
        }
    }

    /// End a sentence: add `.` unless the text already ends with one (or `?`, `!`),
    /// also when that mark sits inside a closing quote.
    fn period(&mut self) {
        let trimmed = self.tail.trim_end_matches(['”', '’', '"']);
        if trimmed.ends_with(['.', '?', '!']) || self.tail.is_empty() {
            return;
        }
        self.text(".");
    }

    /// A title, italic for book-like kinds, in quotes (sentence closed inside) otherwise.
    fn title(&mut self, e: &Entry, title: &str, close: bool) {
        if is_book_like(&e.kind) {
            self.emph(title);
            if close {
                self.period();
            }
        } else {
            self.text("“");
            self.text(title);
            if close {
                self.period();
            }
            self.text("”");
        }
    }

    /// `“Title,”` (IEEE).
    fn quoted_comma(&mut self, title: &str) {
        self.text("“");
        self.text(title);
        if !title.ends_with(['?', '!']) {
            self.text(",");
        }
        self.text("”");
    }

    fn finish(mut self) -> Vec<Inline> {
        if let Some(Inline::Text(t)) = self.inlines.last_mut() {
            let trimmed = t.trim_end().to_string();
            *t = trimmed;
        }
        self.inlines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::plain_text;
    use crate::bib::parse;

    fn bib() -> Bibliography {
        parse(
            r#"
@book{pearl2009, title={Causality: Models, Reasoning, and Inference}, author={Pearl, Judea},
  year={2009}, publisher={Cambridge University Press}, edition={2nd}}
@book{pearl2000, title={Causality: Models, Reasoning, and Inference}, author={Pearl, Judea},
  year={2000}, publisher={Cambridge University Press}, edition={1st}}
@book{golub, title={Matrix Computations}, author={Gene H. Golub and Charles F. Van Loan}, year={2013}}
@book{three, title={Deep Learning}, author={Goodfellow, Ian and Bengio, Yoshua and Courville, Aaron}, year={2016}}
@book{four, title={Model-Based Clustering}, author={A, B and C, D and E, F and G, H}, year={2021}}
@misc{kaggle, title={Kaggle Learn}, author={Cook, Alexis and Kaggle}, year={2017}, url={https://www.kaggle.com/learn}}
@article{vaswani2017, author = {Vaswani, Ashish and Shazeer, Noam and Parmar, Niki},
  title = {Attention Is All You Need}, journal = {Advances in Neural Information Processing Systems},
  volume = {30}, number = {2}, pages = {5998--6008}, year = {2017}, doi = {10.5555/3295222}}
@inproceedings{he2016, author = {He, Kaiming and Zhang, Xiangyu}, title = {Deep Residual Learning},
  booktitle = {Proceedings of CVPR}, pages = {770--778}, year = {2016}, publisher = {IEEE}, address = {Las Vegas}}
@phdthesis{doe2020, author={Doe, John}, title={A Thesis}, school={MIT}, year={2020}}
@misc{woodlock, title={Don Woodlock}, author={Don Woodlock}, year={2010},
  url={https://www.youtube.com/dwoodlock}, urldate={2026-02-21}}
"#,
        )
        .unwrap()
    }

    fn cite(key: &str, mode: CitationMode, prefix: &str, suffix: &str) -> Citation {
        Citation {
            key: key.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
            mode,
        }
    }

    fn render(md: &str, style: Style) -> (Document, Vec<Warning>) {
        apply(&crate::reader::read(md), &bib(), style, "References")
    }

    fn first_paragraph(doc: &Document) -> String {
        match &doc.blocks[0] {
            Block::Paragraph(p) => plain_text(p),
            other => panic!("{other:?}"),
        }
    }

    /// The reference list, one entry per line, as plain text.
    fn refs(doc: &Document) -> Vec<String> {
        let Some(Block::Div { content, .. }) = doc.blocks.last() else {
            panic!("no refs div")
        };
        content
            .iter()
            .map(|b| match b {
                Block::Div { content, .. } => match &content[0] {
                    Block::Paragraph(p) => plain_text(p),
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn author_date_matches_pandoc_chicago() {
        // Pandoc 3.6 + citeproc (chicago-author-date) on the same input:
        // "As argued (Pearl 2009, 3; see also Pearl 2000), and Pearl (2009, chap. 2) agrees;
        //  the year alone (2000)."
        let (doc, warnings) = render(
            "As argued [@pearl2009, p. 3; see also @pearl2000], and @pearl2009 [chap. 2] \
             agrees; the year alone [-@pearl2000].",
            Style::AuthorDate,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            first_paragraph(&doc),
            "As argued (Pearl 2009, 3; see also Pearl 2000), and Pearl (2009, chap. 2) \
             agrees; the year alone (2000)."
        );
        assert_eq!(
            refs(&doc),
            vec![
                "Pearl, Judea. 2000. Causality: Models, Reasoning, and Inference. 1st ed. \
                 Cambridge University Press.",
                "———. 2009. Causality: Models, Reasoning, and Inference. 2nd ed. Cambridge \
                 University Press.",
            ]
        );
    }

    #[test]
    fn author_lists_follow_chicago() {
        let (doc, _) = render(
            "[@golub] [@three] [@four] [@kaggle] [@pearl2009; @pearl2000]",
            Style::AuthorDate,
        );
        assert_eq!(
            first_paragraph(&doc),
            "(Golub and Loan 2013) (Goodfellow, Bengio, and Courville 2016) (A et al. 2021) \
             (Cook and Kaggle 2017) (Pearl 2009, 2000)"
        );
        let refs = refs(&doc);
        assert!(
            refs.contains(
                &"Cook, Alexis, and Kaggle. 2017. “Kaggle Learn.” https://www.kaggle.com/learn."
                    .to_string()
            ),
            "{refs:#?}"
        );
        assert!(
            refs.contains(
                &"Golub, Gene H., and Charles F. Van Loan. 2013. Matrix Computations.".to_string()
            ),
            "{refs:#?}"
        );
        assert!(
            refs.contains(
                &"Goodfellow, Ian, Yoshua Bengio, and Aaron Courville. 2016. Deep Learning."
                    .to_string()
            ),
            "{refs:#?}"
        );
    }

    #[test]
    fn same_author_same_year_is_disambiguated() {
        let bib = parse(
            "@book{a, author={Pearl, Judea}, title={Alpha}, year=2009}\n\
             @book{b, author={Pearl, Judea}, title={Beta}, year=2009}",
        )
        .unwrap();
        let doc = crate::reader::read("[@b] and [@a]");
        let (doc, _) = apply(&doc, &bib, Style::AuthorDate, "");
        assert_eq!(first_paragraph(&doc), "(Pearl 2009b) and (Pearl 2009a)");
    }

    #[test]
    fn other_kinds_in_chicago() {
        let (doc, _) = render("[@vaswani2017; @he2016; @doe2020]", Style::AuthorDate);
        assert_eq!(
            first_paragraph(&doc),
            "(Vaswani, Shazeer, and Parmar 2017; He and Zhang 2016; Doe 2020)"
        );
        assert_eq!(
            refs(&doc),
            vec![
                "Doe, John. 2020. “A Thesis.” PhD thesis, MIT.",
                "He, Kaiming, and Xiangyu Zhang. 2016. “Deep Residual Learning.” In Proceedings \
                 of CVPR, 770–778. Las Vegas: IEEE.",
                "Vaswani, Ashish, Noam Shazeer, and Niki Parmar. 2017. “Attention Is All You \
                 Need.” Advances in Neural Information Processing Systems 30 (2): 5998–6008. \
                 https://doi.org/10.5555/3295222.",
            ]
        );
    }

    #[test]
    fn numeric_matches_pandoc_ieee() {
        // Pandoc 3.6 + ieee.csl: "One [1, p. 3], two [2], [1], in-text [2] says,
        // suppress [1]."
        let (doc, warnings) = render(
            "One [@pearl2009, p. 3], two [@pearl2000; @pearl2009], in-text @pearl2000 says, \
             suppress [-@pearl2009].",
            Style::Numeric,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            first_paragraph(&doc),
            "One [1, p. 3], two [2], [1], in-text [2] says, suppress [1]."
        );
        assert_eq!(
            refs(&doc),
            vec![
                "[1] J. Pearl, Causality: Models, Reasoning, and Inference, 2nd ed. Cambridge \
                 University Press, 2009.",
                "[2] J. Pearl, Causality: Models, Reasoning, and Inference, 1st ed. Cambridge \
                 University Press, 2000.",
            ]
        );
    }

    #[test]
    fn numeric_other_kinds() {
        let (doc, _) = render(
            "[@vaswani2017; @he2016; @doe2020; @woodlock; @golub]",
            Style::Numeric,
        );
        assert_eq!(
            refs(&doc),
            vec![
                "[1] A. Vaswani, N. Shazeer, and N. Parmar, “Attention Is All You Need,” \
                 Advances in Neural Information Processing Systems, vol. 30, no. 2, pp. \
                 5998–6008, 2017, doi: 10.5555/3295222.",
                "[2] K. He and X. Zhang, “Deep Residual Learning,” in Proceedings of CVPR, Las \
                 Vegas: IEEE, 2016, pp. 770–778.",
                "[3] J. Doe, “A Thesis,” PhD thesis, MIT, 2020.",
                "[4] D. Woodlock, “Don Woodlock.” 2010. Accessed: Feb. 21, 2026. [Online]. \
                 Available: https://www.youtube.com/dwoodlock",
                "[5] G. H. Golub and C. F. V. Loan, Matrix Computations. 2013.",
            ]
        );
    }

    #[test]
    fn an_unknown_key_stays_as_written_in_bold_with_a_warning() {
        let (doc, warnings) = render("See [@nokey; @pearl2009] and @nokey.", Style::AuthorDate);
        assert_eq!(warnings.len(), 1, "one warning per key: {warnings:?}");
        assert_eq!(warnings[0].code, "citation-not-found");
        let Block::Paragraph(p) = &doc.blocks[0] else {
            panic!()
        };
        assert!(
            matches!(&p[1], Inline::Strong(inner) if plain_text(inner) == "[@nokey; @pearl2009]")
        );
        assert!(p
            .iter()
            .any(|i| matches!(i, Inline::Strong(inner) if plain_text(inner) == "@nokey")));
    }

    #[test]
    fn the_list_goes_into_an_existing_refs_div() {
        let doc = crate::reader::read("[@golub]\n\n::: {#refs}\n:::\n\nAfter.\n");
        let (doc, _) = apply(&doc, &bib(), Style::AuthorDate, "References");
        assert!(
            matches!(&doc.blocks[1], Block::Div { attr, content } if attr.id.as_deref() == Some("refs") && content.len() == 1)
        );
        assert!(matches!(doc.blocks.last(), Some(Block::Paragraph(_))));
        assert!(!doc
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Heading { .. })));
    }

    #[test]
    fn nocite_star_lists_everything() {
        let doc = crate::reader::read("---\nnocite: '@*'\n---\nNo citation.\n");
        let (doc, _) = apply(&doc, &bib(), Style::Numeric, "");
        assert_eq!(refs(&doc).len(), bib().entries.len());
    }

    #[test]
    fn french_words() {
        let doc = crate::reader::read("---\nlang: fr-FR\n---\n[@golub] [@pearl2009]\n");
        let (doc, _) = apply(&doc, &bib(), Style::AuthorDate, "Bibliographie");
        assert_eq!(first_paragraph(&doc), "(Golub et Loan 2013) (Pearl 2009)");
        assert!(refs(&doc).iter().any(|r| r.contains("2nd éd.")));
    }

    #[test]
    fn citation_unit_helpers() {
        assert_eq!(chicago_locator("p. 3"), "3");
        assert_eq!(chicago_locator("pp. 3–5"), "3–5");
        assert_eq!(chicago_locator("chap. 2"), "chap. 2");
        assert_eq!(ordinal_edition("2"), "2nd");
        assert_eq!(ordinal_edition("11"), "11th");
        assert_eq!(ordinal_edition("Second"), "Second");
        let c = cite("a", CitationMode::Normal, "", "");
        assert_eq!(c.key, "a");
    }
}
