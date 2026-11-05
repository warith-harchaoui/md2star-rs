//! Slide segmentation and deterministic layout selection.
//!
//! A port of the deterministic half of md2star's `pptx_layout.py`:
//!
//! - **Segmentation** ([`segment`]): every thematic break ends a slide, every `#`/`##`
//!   heading starts one; speaker notes ([`Block::Notes`]) go to the slide they are
//!   written on, and notes alone after a `---` go to the slide before; a front-matter
//!   `title` that no slide carries becomes the cover; the cover's subtitle falls back to
//!   the front matter's `subtitle`, `author` and `date`.
//! - **Role** ([`choose_role`]): an explicit `{layout="Name"}` wins; then a class
//!   (`.statement`/`.big`, `.bignumber`, `.quote`, `.recap`, `.agenda`, `.section`,
//!   `.closing`, `.cover`, `.photo`); then the slide's form — the first title-only slide
//!   is the cover, the last the closing, a title-only `#` in between a section; a short
//!   bold figure alone is a big number; a quote with no list is a quote; one short
//!   sentence is a statement; a lone figure is a figure slide; `::: columns` is two
//!   columns; anything else is content. A recap title ("What we learned", "Ce que nous
//!   avons appris", "Key takeaways") is a recap.
//! - **Layout** (`place`): a role the template names (`Big Number`, `Quote`,
//!   `Section Header`…) gets that layout; a cover, divider or closing gets the layout
//!   whose geometry says so; a statement, big number or quotation the template has no
//!   layout for is set large on the stage of its title-only layout; everything else is
//!   ranked by [`super::fit::plan`] over the layouts content may land on.
//! - **Fit** (`--min-pt`): no text under N points — a slide no layout holds moves to a
//!   roomier one or is split into "(cont.)" / "(suite)" slides, between list items,
//!   paragraphs, table rows or code lines. Never shrunk.
//!
//! A language model (`--smart-layout`, feature `llm`, or any [`LayoutJudge`] a host
//! provides) is only asked to break near-ties between layouts that fit; it never
//! overrides a hint, a role or the geometry.

use super::content::{self, Para, Unit};
use super::fit::{self, Archetype, Content, Placed, Plan, Typo};
use super::judge::{LayoutFacts, LayoutJudge, SlideFacts};
use super::template::TemplateInfo;
use crate::ast::{plain_text, Attr, Block, Document, Inline, Warning};
use crate::error::Result;

/// One slide, cut out of the document.
#[derive(Debug, Clone, PartialEq)]
pub struct Slide {
    /// 1-based position in the deck.
    pub index: usize,
    /// The slide title (the heading's inlines; empty for an untitled slide).
    pub title: Vec<Inline>,
    /// The heading level that opened the slide (0 when none did).
    pub level: u8,
    /// The heading's attributes (`{.statement layout="Big Number"}`).
    pub attr: Attr,
    /// What is drawn on the slide, notes removed.
    pub body: Vec<Block>,
    /// The speaker notes' blocks.
    pub notes: Vec<Block>,
    /// A cover, closing or divider with nothing to place: its heading, and its prose
    /// as a subtitle, nothing else.
    pub title_only: bool,
    /// The lines under a title-only slide's title (its prose; for the cover, the front
    /// matter's `subtitle`, `author` and `date` too).
    pub subtitle: Vec<Vec<Inline>>,
}

/// The role a slide plays, which picks its layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// An explicit `{layout="Name"}`: that template layout, by name.
    Named(String),
    /// The title slide.
    Cover,
    /// A section divider.
    Section,
    /// One short sentence, set large.
    Statement,
    /// A key figure, set very large.
    BigNumber,
    /// A quotation.
    Quote,
    /// A recap of what came before.
    Recap,
    /// An agenda.
    Agenda,
    /// The closing slide.
    Closing,
    /// A full-bleed photograph (`.photo`).
    Photo,
    /// A lone figure.
    Figure,
    /// Two columns (`::: columns`).
    TwoColumn,
    /// Ordinary content: title and body.
    Content,
}

impl Role {
    /// The layout names that mean this role in a template that names its layouts
    /// plainly (PowerPoint's own vocabulary and md2star's course template).
    pub fn layout_names(&self) -> &'static [&'static str] {
        match self {
            Role::Statement => &["Statement"],
            Role::BigNumber => &["Big Number"],
            Role::Quote => &["Quote"],
            Role::Recap => &["Recap"],
            Role::Agenda => &["Agenda"],
            Role::Section => &["Section Header", "Section"],
            Role::Closing => &["Closing"],
            Role::Cover => &["Title Slide"],
            Role::Photo => &["Photo"],
            _ => &[],
        }
    }

    /// A word for the role (prompts, warnings).
    pub fn label(&self) -> &str {
        match self {
            Role::Named(n) => n,
            Role::Cover => "cover",
            Role::Section => "section divider",
            Role::Statement => "statement",
            Role::BigNumber => "big number",
            Role::Quote => "quotation",
            Role::Recap => "recap",
            Role::Agenda => "agenda",
            Role::Closing => "closing",
            Role::Photo => "photo",
            Role::Figure => "figure",
            Role::TwoColumn => "two columns",
            Role::Content => "content",
        }
    }
}

/// Classes that ask for a role.
fn class_role(class: &str) -> Option<Role> {
    Some(match class.to_lowercase().as_str() {
        "statement" | "big" => Role::Statement,
        "bignumber" | "big-number" => Role::BigNumber,
        "quote" => Role::Quote,
        "recap" => Role::Recap,
        "agenda" => Role::Agenda,
        "section" => Role::Section,
        "closing" => Role::Closing,
        "cover" => Role::Cover,
        "photo" => Role::Photo,
        _ => return None,
    })
}

fn is_recap_title(title: &str) -> bool {
    let t = title
        .trim()
        .trim_end_matches(['?', '!', '.', ':', ' '])
        .to_lowercase()
        .replace('’', "'");
    matches!(
        t.as_str(),
        "qu'est-ce que nous avons appris"
            | "qu'est ce que nous avons appris"
            | "ce que nous avons appris"
            | "what we learned"
            | "what we learnt"
            | "what we have learned"
            | "what we have learnt"
            | "what we've learned"
            | "what we've learnt"
            | "key takeaway"
            | "key takeaways"
    )
}

/// Something to *place* (not prose): a slide carrying one is content wherever it is.
fn is_payload(b: &Block) -> bool {
    match b {
        Block::List { .. }
        | Block::Table { .. }
        | Block::Figure(_)
        | Block::CodeBlock { .. }
        | Block::Mermaid { .. }
        | Block::BlockQuote(_) => true,
        Block::Paragraph(inl) => inl.iter().any(|i| matches!(i, Inline::Image(_))),
        Block::Div { content, .. } => content.iter().any(is_payload) || content.is_empty(),
        _ => false,
    }
}

fn prose(body: &[Block]) -> Vec<Vec<Inline>> {
    body.iter()
        .filter_map(|b| match b {
            Block::Paragraph(i) if !i.is_empty() => Some(i.clone()),
            Block::Heading { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn norm(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cut a document into slides (see the module docs).
///
/// # Errors
///
/// None today; the `Result` keeps the signature open.
pub fn segment(doc: &Document) -> Result<Vec<Slide>> {
    let mut slides: Vec<Slide> = Vec::new();
    let mut current: Option<Slide> = None;
    let mut pending_notes: Vec<Block> = Vec::new();
    let flush = |current: &mut Option<Slide>, slides: &mut Vec<Slide>, pending: &mut Vec<Block>| {
        if let Some(mut s) = current.take() {
            let visible = s.body.iter().any(|b| !matches!(b, Block::RawHtml(_)));
            if !visible && s.title.is_empty() && s.level == 0 {
                // Notes alone: they belong to the slide before.
                match slides.last_mut() {
                    Some(prev) => prev.notes.extend(s.notes),
                    None => pending.extend(s.notes),
                }
                return;
            }
            if !pending.is_empty() {
                let mut notes = std::mem::take(pending);
                notes.extend(s.notes);
                s.notes = notes;
            }
            slides.push(s);
        }
    };
    let blank = |level: u8, title: Vec<Inline>, attr: Attr| Slide {
        index: 0,
        title,
        level,
        attr,
        body: Vec::new(),
        notes: Vec::new(),
        title_only: false,
        subtitle: Vec::new(),
    };
    for block in &doc.blocks {
        match block {
            Block::Heading {
                level: level @ (1 | 2),
                content,
                attr,
            } => {
                flush(&mut current, &mut slides, &mut pending_notes);
                current = Some(blank(*level, content.clone(), attr.clone()));
            }
            Block::ThematicBreak => flush(&mut current, &mut slides, &mut pending_notes),
            Block::FootnoteDef { .. } => {}
            Block::Notes(n) => match current.as_mut() {
                Some(s) => s.notes.extend(n.iter().cloned()),
                None => match slides.last_mut() {
                    Some(prev) => prev.notes.extend(n.iter().cloned()),
                    None => pending_notes.extend(n.iter().cloned()),
                },
            },
            Block::RawHtml(_) => {}
            other => current
                .get_or_insert_with(|| blank(0, Vec::new(), Attr::default()))
                .body
                .push(other.clone()),
        }
    }
    flush(&mut current, &mut slides, &mut pending_notes);

    let meta = &doc.meta;
    let meta_title = meta.title.clone().unwrap_or_default();
    if !meta_title.trim().is_empty() {
        match slides.first_mut() {
            Some(first) if first.title.is_empty() && first.level == 0 => {
                // An untitled opening slide takes the document's title.
                first.title = vec![Inline::Text(meta_title.clone())];
            }
            Some(first) if norm(&plain_text(&first.title)) == norm(&meta_title) => {}
            _ => {
                let mut cover = blank(1, vec![Inline::Text(meta_title.clone())], Attr::default());
                cover.notes = std::mem::take(&mut pending_notes);
                slides.insert(0, cover);
            }
        }
    }
    if slides.is_empty() && !pending_notes.is_empty() {
        let mut s = blank(0, Vec::new(), Attr::default());
        s.notes = pending_notes;
        slides.push(s);
    }
    let n = slides.len();
    for (i, s) in slides.iter_mut().enumerate() {
        s.index = i + 1;
        let payload = s.body.iter().any(is_payload);
        let edge = s.index == 1 || s.index == n;
        let divider = s.level == 1 && s.index != 1 && !payload;
        let lines = prose(&s.body);
        // A subtitle is a line or two, not an essay: long prose under a heading is
        // content, wherever the slide sits.
        let short = lines.len() <= 4
            && lines.iter().all(|l| plain_text(l).chars().count() <= 160)
            && lines
                .iter()
                .map(|l| plain_text(l).chars().count())
                .sum::<usize>()
                <= 320;
        if (edge || divider) && !payload && short {
            s.title_only = true;
            s.subtitle = lines;
        }
    }
    if let Some(first) = slides.first_mut() {
        if first.title_only {
            let mut known: Vec<String> = first
                .subtitle
                .iter()
                .map(|l| norm(&plain_text(l)))
                .collect();
            let mut extra: Vec<String> = Vec::new();
            if let Some(sub) = meta.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
                extra.push(sub.trim().to_string());
            }
            // Author and date share one line: a cover's subtitle box is drawn for one
            // or two lines, and "Name · date" reads as one credit.
            let byline: Vec<String> = meta
                .authors
                .iter()
                .map(|a| a.trim().to_string())
                .chain(meta.date.iter().map(|d| d.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            if !byline.is_empty() {
                extra.push(byline.join(" · "));
            }
            for line in extra {
                let key = norm(&line);
                if !known.contains(&key) {
                    known.push(key);
                    first.subtitle.push(vec![Inline::Text(line)]);
                }
            }
        }
    }
    Ok(slides)
}

/// The role of `slide` in a deck of `slide_count` slides (see the module docs).
///
/// # Errors
///
/// None today; the `Result` keeps the signature open.
pub fn choose_role(slide: &Slide, slide_count: usize) -> Result<Role> {
    if let Some(name) = slide.attr.get("layout") {
        if !name.trim().is_empty() {
            return Ok(Role::Named(name.trim().to_string()));
        }
    }
    if let Some(role) = slide.attr.classes.iter().find_map(|c| class_role(c)) {
        return Ok(role);
    }
    if is_recap_title(&plain_text(&slide.title)) {
        return Ok(Role::Recap);
    }
    if slide.title_only {
        if slide.index == 1 {
            return Ok(Role::Cover);
        }
        if slide.index == slide_count {
            return Ok(Role::Closing);
        }
        return Ok(Role::Section);
    }
    if content::columns(&slide.body).is_some() {
        return Ok(Role::TwoColumn);
    }
    let structural = slide.body.iter().any(|b| {
        matches!(
            b,
            Block::List { .. }
                | Block::Table { .. }
                | Block::Figure(_)
                | Block::CodeBlock { .. }
                | Block::Mermaid { .. }
        ) || matches!(b, Block::Paragraph(i) if i.iter().any(|x| matches!(x, Inline::Image(_))))
            || matches!(b, Block::Div { .. })
    });
    let figures = slide
        .body
        .iter()
        .filter(|b| {
            matches!(b, Block::Figure(_))
                || matches!(b, Block::Paragraph(i) if i.iter().all(|x| matches!(x, Inline::Image(_) | Inline::SoftBreak)) && !i.is_empty())
        })
        .count();
    let words: usize = prose(&slide.body)
        .iter()
        .map(|p| plain_text(p).split_whitespace().count())
        .sum();
    if figures == 1
        && words <= 25
        && slide
            .body
            .iter()
            .all(|b| matches!(b, Block::Figure(_) | Block::Paragraph(_)))
    {
        return Ok(Role::Figure);
    }
    if !structural {
        // A short bold figure alone on its line is a big number.
        let big = slide.body.iter().any(|b| match b {
            Block::Paragraph(i) => match i.as_slice() {
                [Inline::Strong(inner)] => {
                    let t = plain_text(inner);
                    let t = t.trim();
                    t.chars().count() <= 12 && t.chars().any(|c| c.is_ascii_digit())
                }
                _ => false,
            },
            _ => false,
        });
        if big {
            return Ok(Role::BigNumber);
        }
        if slide.body.iter().any(|b| matches!(b, Block::BlockQuote(_))) {
            return Ok(Role::Quote);
        }
        let lines = prose(&slide.body);
        if lines.len() == 1 && plain_text(&lines[0]).chars().count() <= 140 {
            return Ok(Role::Statement);
        }
    }
    Ok(Role::Content)
}

/// `"(suite)"` for a French deck, `"(cont.)"` otherwise; without a `lang`, the commoner
/// function words of `sample` decide.
pub fn continuation_label(lang: Option<&str>, sample: &str) -> &'static str {
    if let Some(code) = lang.map(str::trim).filter(|l| !l.is_empty()) {
        return if code.to_lowercase().starts_with("fr") {
            "(suite)"
        } else {
            "(cont.)"
        };
    }
    let lower = sample.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphabetic() || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    let fr = [
        "le", "la", "les", "des", "est", "un", "une", "de", "du", "et", "pour", "que", "à", "en",
    ];
    let en = [
        "the", "and", "is", "of", "to", "for", "that", "with", "are", "a",
    ];
    let f = words.iter().filter(|w| fr.contains(w)).count();
    let e = words.iter().filter(|w| en.contains(w)).count();
    if f > e {
        "(suite)"
    } else {
        "(cont.)"
    }
}

// ── selection ───────────────────────────────────────────────────────────────

/// The score a template's canonical layout of an archetype gets over its siblings.
pub const CANONICAL_BONUS: f32 = 0.3;

/// What selection needs to know about the deck.
pub struct Ctx<'a> {
    /// The template.
    pub info: &'a TemplateInfo,
    /// Each layout's archetype, in template order.
    pub archetypes: Vec<Archetype>,
    /// Sizes and faces.
    pub typo: Typo,
    /// The near-tie arbiter, if any.
    pub judge: Option<&'a dyn LayoutJudge>,
}

impl<'a> Ctx<'a> {
    /// A context over `info`.
    pub fn new(info: &'a TemplateInfo, typo: Typo, judge: Option<&'a dyn LayoutJudge>) -> Self {
        let archetypes = info
            .layouts
            .iter()
            .map(|l| fit::archetype(l, info.slide_size))
            .collect();
        Ctx {
            info,
            archetypes,
            typo,
            judge,
        }
    }

    fn by_name(&self, name: &str) -> Option<usize> {
        let layouts = &self.info.layouts;
        layouts.iter().position(|l| l.name == name).or_else(|| {
            let folded = name.to_lowercase();
            layouts.iter().position(|l| l.name.to_lowercase() == folded)
        })
    }

    fn by_archetype(&self, a: Archetype) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, x) in self.archetypes.iter().enumerate() {
            if *x == a {
                let uses = self.info.layouts[i].example_slides;
                if best.is_none_or(|b| uses > self.info.layouts[b].example_slides) {
                    best = Some(i);
                }
            }
        }
        best
    }

    /// Whether layout `i` is the canonical one of its archetype: the most used by the
    /// template's own example slides, the first in master order on a tie.
    pub fn canonical(&self, i: usize) -> bool {
        self.by_archetype(self.archetypes[i]) == Some(i)
    }

    /// Layouts ordinary content may land on: automatic archetypes whose name does not
    /// reserve them for a role.
    pub fn automatic(&self) -> Vec<usize> {
        const RESERVED: &[&str] = &[
            "recap",
            "agenda",
            "closing",
            "statement",
            "big number",
            "quote",
            "photo",
        ];
        (0..self.info.layouts.len())
            .filter(|&i| {
                self.archetypes[i].automatic()
                    && !RESERVED.contains(&self.info.layouts[i].name.to_lowercase().as_str())
            })
            .collect()
    }
}

/// A slide placed on a layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The plan (layout and positions).
    pub plan: Plan,
    /// The title as written.
    pub title: Vec<Inline>,
    /// The slide's notes (first part of a split slide only).
    pub notes: Vec<Block>,
    /// Why this layout, in words.
    pub why: String,
    /// The role it plays.
    pub role: Role,
}

/// The paragraphs of a title-only slide's subtitle.
fn subtitle_paras(slide: &Slide) -> Vec<Para> {
    slide
        .subtitle
        .iter()
        .map(|l| Para::plain(l.clone()))
        .collect()
}

/// A title-only slide on layout `index`: the title in its placeholder, the subtitle in
/// the subtitle (or largest body) placeholder, grown downward when it needs more room
/// than the template drew, never shrunk.
fn title_only_plan(subtitle: &[Para], title: &str, index: usize, ctx: &Ctx) -> Plan {
    let info = ctx.info;
    let layout = &info.layouts[index];
    let mut plan = fit::plan(
        &Content {
            title: title.to_string(),
            flows: vec![],
            columns: false,
        },
        index,
        info,
        &ctx.typo,
    );
    plan.placed.clear();
    plan.fits = !plan
        .reasons
        .iter()
        .any(|r| r.contains("title") && !r.contains("stay empty"));
    plan.reasons.retain(|r| !r.contains("stay empty"));
    if subtitle.is_empty() {
        return plan;
    }
    let sub = layout
        .placeholders
        .iter()
        .position(|p| p.kind == "subTitle")
        .or_else(|| fit::bodies(layout, info).first().copied());
    match sub {
        Some(s) => {
            let ph = &layout.placeholders[s];
            let frame = ph.frame.unwrap_or((
                info.slide_size.0 / 16,
                info.slide_size.1 * 3 / 4,
                info.slide_size.0 * 7 / 8,
                info.slide_size.1 / 8,
            ));
            let refs: Vec<&Para> = subtitle.iter().collect();
            let needed = fit::paras_height(
                &refs,
                fit::usable_width_pt(frame, ph.insets),
                &|p| fit::para_size(p, ph, &ctx.typo),
                &|p| fit::measure_of(ph, p.bold),
                &|p| fit::indent_pt(p, Some(ph), &ctx.typo),
            );
            let avail = fit::usable_height_pt(frame, ph.insets);
            let mut grown = None;
            if needed > avail {
                // Grow down to the slide's bottom margin, then up towards the title:
                // never over it, never off the slide, never smaller type.
                let want = (needed * fit::EMU_PER_PT) as i64 + ph.insets.1 + ph.insets.3;
                let bottom = info.slide_size.1 - info.slide_size.1 / 30;
                let below_title = fit::title_index(layout)
                    .and_then(|t| layout.placeholders[t].frame)
                    .filter(|t| t.1 < frame.1)
                    .map_or(frame.1, |t| t.1 + t.3 + info.slide_size.1 / 60);
                // Under the layout's own drawing first (a band, a rule above the
                // subtitle); over it only when nothing else holds the lines.
                let below_drawing = layout
                    .decorations
                    .iter()
                    .filter(|d| d.1 + d.3 <= frame.1 + info.slide_size.1 / 40)
                    .filter(|d| d.0 < frame.0 + frame.2 && frame.0 < d.0 + d.2)
                    .map(|d| d.1 + d.3 + info.slide_size.1 / 60)
                    .max()
                    .unwrap_or(below_title)
                    .max(below_title)
                    .min(frame.1);
                let ceiling = if bottom - want >= below_drawing {
                    below_drawing
                } else {
                    below_title
                };
                let top = frame.1.min((bottom - want).max(ceiling));
                if top + want <= bottom {
                    grown = Some((frame.0, top, frame.2, want.max(frame.3)));
                    plan.reasons
                        .push("the subtitle box grows to hold every line".into());
                } else {
                    plan.fits = false;
                    plan.reasons.push("the subtitle does not fit".into());
                }
            }
            plan.placed.push(Placed::Text {
                ph: s,
                frame: grown,
                paras: subtitle.to_vec(),
            });
        }
        None => {
            let frame = fit::stage(layout, info);
            let size = ctx.typo.eff(24.0);
            plan.placed.push(Placed::Free {
                frame,
                paras: subtitle.to_vec(),
                size,
                levels: [size; 9],
                center: false,
            });
        }
    }
    plan
}

/// Describe a slide's content in words, for the judge.
fn describe_content(c: &Content) -> String {
    let mut paras = 0;
    let mut words = 0;
    let mut tables = Vec::new();
    let mut code = 0;
    let mut pictures = Vec::new();
    for u in c.flows.iter().flatten() {
        match u {
            Unit::Para(p) => {
                paras += 1;
                words += p.text().split_whitespace().count();
            }
            Unit::Table(t) => tables.push(format!(
                "a table of {} rows and {} columns",
                t.rows.len() + 1,
                t.cols()
            )),
            Unit::Code(l) => code += l.len(),
            Unit::Picture(p) => pictures.push(if p.aspect > 1.3 {
                "a landscape picture"
            } else if p.aspect < 0.77 {
                "a portrait picture"
            } else {
                "a squarish picture"
            }),
        }
    }
    let mut parts = Vec::new();
    if c.columns {
        parts.push(format!("{} columns", c.flows.len()));
    }
    if paras > 0 {
        parts.push(format!(
            "{paras} paragraph(s) or bullet(s), {words} words in all"
        ));
    }
    parts.extend(tables);
    if code > 0 {
        parts.push(format!("a code listing of {code} lines"));
    }
    for p in pictures {
        parts.push(p.to_string());
    }
    if parts.is_empty() {
        "nothing but its title".into()
    } else {
        parts.join("; ")
    }
}

/// Describe a layout in words, for the judge.
fn describe_layout(index: usize, ctx: &Ctx) -> String {
    let info = ctx.info;
    let layout = &info.layouts[index];
    let (w, h) = (info.slide_size.0 as f32, info.slide_size.1 as f32);
    let place = |f: (i64, i64, i64, i64)| {
        let cx = (f.0 as f32 + f.2 as f32 / 2.0) / w;
        let cy = (f.1 as f32 + f.3 as f32 / 2.0) / h;
        let horiz = if f.2 as f32 > 0.7 * w {
            "full width"
        } else if cx < 0.4 {
            "on the left"
        } else if cx > 0.6 {
            "on the right"
        } else {
            "centred"
        };
        let vert = if cy < 0.35 {
            "at the top"
        } else if cy > 0.65 {
            "at the bottom"
        } else {
            "in the middle"
        };
        let share = (f.2 as f32 * f.3 as f32) / (w * h);
        let size = if share > 0.4 {
            "large"
        } else if share > 0.15 {
            "medium"
        } else {
            "small"
        };
        format!("{size}, {horiz}, {vert}")
    };
    let mut parts = vec![ctx.archetypes[index].label().to_string()];
    for &b in &fit::bodies(layout, info) {
        let ph = &layout.placeholders[b];
        if let Some(f) = ph.frame {
            let size = ctx.typo.eff(ph.size_pt(0));
            let lines = (fit::usable_height_pt(f, ph.insets) / (size * fit::LINE_SPACING)).floor();
            parts.push(format!(
                "a text box ({}) holding about {lines:.0} lines at {size:.0} pt",
                place(f)
            ));
        }
    }
    for &p in &fit::pictures(layout, info) {
        if let Some(f) = layout.placeholders[p].frame {
            parts.push(format!("a picture box ({})", place(f)));
        }
    }
    parts.join("; ")
}

/// Rank `candidates` for `content`; ask the judge between near-equals.
fn rank(
    content: &Content,
    candidates: &[usize],
    ctx: &Ctx,
    slide_index: usize,
    total: usize,
) -> Option<(Plan, String)> {
    // (plan, geometric score): the canonical bonus ranks, the geometry decides what
    // counts as a near-tie.
    let mut plans: Vec<(Plan, f32)> = candidates
        .iter()
        .map(|&i| {
            let mut p = fit::plan(content, i, ctx.info, &ctx.typo);
            let geometric = p.score;
            if ctx.canonical(i) {
                // The designer's own choice for this kind of slide: Python md2star's
                // "first seen per archetype", with the template's usage first.
                p.score += CANONICAL_BONUS;
            }
            (p, geometric)
        })
        .collect();
    plans.sort_by(|(a, _), (b, _)| {
        b.fits
            .cmp(&a.fits)
            .then(b.score.total_cmp(&a.score))
            .then(a.layout.cmp(&b.layout))
    });
    let (best, best_geometric) = plans.first()?.clone();
    if !best.fits {
        return Some((best, "nothing fits; the closest layout".into()));
    }
    let why = format!(
        "best geometric fit ({})",
        best.reasons.first().map_or("occupancy", String::as_str)
    );
    if let Some(judge) = ctx.judge {
        const NEAR: f32 = 0.15;
        let near: Vec<&Plan> = plans
            .iter()
            .filter(|(p, g)| p.fits && (best_geometric - g).abs() <= NEAR)
            .map(|(p, _)| p)
            .take(4)
            .collect();
        if near.len() >= 2 {
            let facts = SlideFacts {
                index: slide_index as u32,
                total: total as u32,
                title: content.title.clone(),
                description: describe_content(content),
            };
            let options: Vec<LayoutFacts> = near
                .iter()
                .map(|p| LayoutFacts {
                    name: ctx.info.layouts[p.layout].name.clone(),
                    description: describe_layout(p.layout, ctx),
                })
                .collect();
            if let Some(choice) = super::judge::validate(judge.choose(&facts, &options), &options) {
                if let Some(p) = near
                    .iter()
                    .find(|p| ctx.info.layouts[p.layout].name == choice.layout)
                {
                    return Some((
                        (*p).clone(),
                        format!("model's pick among near-equals: {}", choice.reason),
                    ));
                }
            }
        }
    }
    Some((best, why))
}

/// The plan for a layout the author or the role asked for: built as asked, and when
/// the layout has no body box for the slide's sentence, the sentence is set on the
/// stage under the title rather than dropped.
fn pinned_plan(slide: &Slide, role: &Role, content: &Content, i: usize, ctx: &Ctx) -> Plan {
    let title_only = slide.title_only
        || matches!(role, Role::Cover | Role::Section | Role::Closing) && content.is_empty();
    if title_only {
        return title_only_plan(&subtitle_paras(slide), &content.title, i, ctx);
    }
    let plan = fit::plan(content, i, ctx.info, &ctx.typo);
    if plan.fits || content.columns || content.is_empty() {
        return plan;
    }
    let layout = &ctx.info.layouts[i];
    let only_text = content.flows.iter().flatten().all(Unit::is_para);
    if only_text && fit::bodies(layout, ctx.info).is_empty() {
        let paras = stage_paras(content);
        let sizes: &[f32] = match role {
            Role::BigNumber => &[96.0, 80.0, 66.0, 54.0, 44.0],
            Role::Statement | Role::Quote => &[44.0, 40.0, 36.0, 32.0, 28.0],
            _ => &[28.0, 24.0, 20.0, 18.0],
        };
        if let Some(p) = fit::stage_plan(&content.title, &paras, sizes, i, ctx.info, &ctx.typo) {
            return p;
        }
    }
    plan
}

/// A slide's paragraphs as the stage sets them: no markers, no levels.
fn stage_paras(content: &Content) -> Vec<Para> {
    content
        .flows
        .iter()
        .flatten()
        .filter_map(|u| match u {
            Unit::Para(p) => Some(Para {
                bullet: content::Bullet::None,
                level: 0,
                ..p.clone()
            }),
            _ => None,
        })
        .collect()
}

/// The plan for `content` playing `role`, and why.
fn choose(
    slide: &Slide,
    role: &Role,
    content: &Content,
    ctx: &Ctx,
    total: usize,
    warnings: &mut Vec<Warning>,
) -> (Plan, String, bool) {
    let title = content.title.as_str();
    // 1. An explicit layout name.
    if let Role::Named(name) = role {
        match ctx.by_name(name) {
            Some(i) => {
                if ctx.info.layouts[i].name != *name {
                    warnings.push(Warning::new(
                        "layout-hint-case",
                        format!(
                            "slide {}: {{layout=\"{name}\"}} matched \"{}\" ignoring case",
                            slide.index, ctx.info.layouts[i].name
                        ),
                    ));
                }
                let plan = pinned_plan(slide, role, content, i, ctx);
                return (plan, "explicit layout in the Markdown".into(), true);
            }
            None => {
                let names: Vec<&str> = ctx.info.layouts.iter().map(|l| l.name.as_str()).collect();
                warnings.push(Warning::new(
                    "layout-hint-unknown",
                    format!(
                        "slide {}: {{layout=\"{name}\"}} is not a layout of this template (it has: {})",
                        slide.index,
                        names.join(", ")
                    ),
                ));
            }
        }
    }
    // 2. A role the template names.
    for name in role.layout_names() {
        if let Some(i) = ctx.by_name(name) {
            let plan = pinned_plan(slide, role, content, i, ctx);
            return (
                plan,
                format!("the template's {} layout", ctx.info.layouts[i].name),
                true,
            );
        }
    }
    // 3. Roles the geometry recognises.
    let by_geometry: &[Archetype] = match role {
        Role::Cover => &[Archetype::Cover],
        Role::Section => &[Archetype::Section, Archetype::TitleOnly],
        Role::Closing => &[Archetype::Closing, Archetype::Cover, Archetype::TitleOnly],
        Role::Photo => &[Archetype::Photo, Archetype::Picture],
        _ => &[],
    };
    if slide.title_only || matches!(role, Role::Photo) {
        for a in by_geometry {
            if let Some(i) = ctx.by_archetype(*a) {
                let plan = pinned_plan(slide, role, content, i, ctx);
                return (
                    plan,
                    format!("a {} layout for the {}", a.label(), role.label()),
                    true,
                );
            }
        }
    }
    // 4. A statement, a big number, a quotation: set large on the stage.
    if matches!(role, Role::Statement | Role::BigNumber | Role::Quote) && !content.columns {
        if let Some(stage) = ctx.by_archetype(Archetype::TitleOnly) {
            let paras = stage_paras(content);
            let only_text = content.flows.iter().flatten().all(Unit::is_para);
            let sizes: &[f32] = match role {
                Role::BigNumber => &[96.0, 80.0, 66.0, 54.0, 44.0],
                _ => &[44.0, 40.0, 36.0, 32.0, 28.0],
            };
            if only_text && !paras.is_empty() {
                if let Some(plan) =
                    fit::stage_plan(title, &paras, sizes, stage, ctx.info, &ctx.typo)
                {
                    return (
                        plan,
                        format!("a {} set large under the title", role.label()),
                        true,
                    );
                }
            }
        }
    }
    // 5. Everything else: ranked.
    let candidates = ctx.automatic();
    match rank(content, &candidates, ctx, slide.index, total) {
        Some((plan, why)) => (plan, why, false),
        None => {
            let plan = fit::plan(content, 0, ctx.info, &ctx.typo);
            (
                plan,
                "the template's first layout (nothing else)".into(),
                false,
            )
        }
    }
}

/// The content of `slide` as flows of units.
pub(crate) fn slide_content(slide: &Slide, store: &content::ImageStore) -> Content {
    let title = super::content::printed(&slide.title).trim().to_string();
    if slide.title_only {
        return Content {
            title,
            flows: vec![],
            columns: false,
        };
    }
    match content::columns(&slide.body) {
        Some(cols) => {
            // Blocks outside the columns div (an intro line) go before the first column.
            let outside: Vec<Block> = slide
                .body
                .iter()
                .filter(|b| !matches!(b, Block::Div { attr, .. } if attr.has_class("columns")))
                .cloned()
                .collect();
            let mut flows: Vec<Vec<Unit>> = cols.iter().map(|c| content::units(c, store)).collect();
            if !outside.is_empty() {
                let mut first = content::units(&outside, store);
                first.append(&mut flows[0]);
                flows[0] = first;
            }
            Content {
                title,
                flows,
                columns: true,
            }
        }
        None => Content {
            title,
            flows: vec![content::units(&slide.body, store)],
            columns: false,
        },
    }
}

/// Place one slide: choose its layout and, when nothing holds it, split it into
/// continuation slides. Never shrinks.
pub(crate) fn place(
    slide: &Slide,
    total: usize,
    ctx: &Ctx,
    store: &content::ImageStore,
    label: &str,
    warnings: &mut Vec<Warning>,
) -> Vec<Placement> {
    let role = choose_role(slide, total).unwrap_or(Role::Content);
    let content = slide_content(slide, store);
    let (plan, why, pinned) = choose(slide, &role, &content, ctx, total, warnings);
    if slide.title_only && !plan.fits {
        // A cover or a divider whose lines no title layout holds at this size is set
        // as an ordinary slide instead: its prose becomes its body, nothing is shrunk.
        let mut plain = slide.clone();
        plain.title_only = false;
        plain.subtitle.clear();
        if plain.body.is_empty() {
            plain.body = slide
                .subtitle
                .iter()
                .map(|l| Block::Paragraph(l.clone()))
                .collect();
        }
        return place(&plain, total, ctx, store, label, warnings);
    }
    let one = |plan: Plan, why: String| Placement {
        plan,
        title: slide.title.clone(),
        notes: slide.notes.clone(),
        why,
        role: role.clone(),
    };
    if plan.fits || slide.title_only {
        if !plan.fits {
            warnings.push(Warning::new(
                "slide-overflows",
                format!(
                    "slide {} ({}): {}",
                    slide.index,
                    content.title,
                    plan.reasons.join("; ")
                ),
            ));
        }
        return vec![one(plan, why)];
    }
    let min = ctx
        .typo
        .min_pt
        .map(|m| format!(" at {m:.0} pt"))
        .unwrap_or_default();
    if content.columns || content.flows.len() != 1 {
        warnings.push(Warning::new(
            "slide-overflows",
            format!(
                "slide {} ({}) does not fit its layout{min} and cannot be split automatically (columns): split it by hand",
                slide.index, content.title
            ),
        ));
        return vec![one(plan, why)];
    }
    // Split: greedy, in reading order, against the pinned layout or any layout.
    let chunks = content::split_points(&content.flows[0]);
    let pinned_layout = pinned.then_some(plan.layout);
    let fits_some = |title: &str, units: Vec<Unit>| -> Option<(Plan, String)> {
        let c = Content {
            title: title.to_string(),
            flows: vec![units],
            columns: false,
        };
        match pinned_layout {
            Some(i) => {
                let p = fit::plan(&c, i, ctx.info, &ctx.typo);
                p.fits
                    .then(|| (p, "explicit layout, continued".to_string()))
            }
            None => {
                let (p, w, _) = choose(slide, &Role::Content, &c, ctx, total, &mut Vec::new());
                p.fits.then_some((p, w))
            }
        }
    };
    let cont_title = format!("{} {label}", content.title).trim().to_string();
    let mut groups: Vec<Vec<Vec<Unit>>> = Vec::new();
    for chunk in chunks {
        let title = if groups.len() <= 1 {
            content.title.clone()
        } else {
            cont_title.clone()
        };
        if let Some(last) = groups.last() {
            let mut trial = last.clone();
            trial.push(chunk.clone());
            if fits_some(&title, content::rejoin(&trial)).is_some() {
                if let Some(g) = groups.last_mut() {
                    g.push(chunk);
                }
                continue;
            }
        }
        groups.push(vec![chunk]);
    }
    if groups.len() <= 1 {
        warnings.push(Warning::new(
            "slide-overflows",
            format!(
                "slide {} ({}) does not fit its layout{min} and has a single block to show: shorten it",
                slide.index, content.title
            ),
        ));
        return vec![one(plan, why)];
    }
    warnings.push(Warning::new(
        "slide-split",
        format!(
            "slide {} ({}) does not fit one layout{min}: split into {} slides",
            slide.index,
            content.title,
            groups.len()
        ),
    ));
    let mut out = Vec::new();
    for (k, g) in groups.iter().enumerate() {
        let (title_text, title_inl) = if k == 0 {
            (content.title.clone(), slide.title.clone())
        } else {
            let mut t = slide.title.clone();
            match t.last_mut() {
                Some(Inline::Text(last)) => {
                    last.push(' ');
                    last.push_str(label);
                }
                _ => t.push(Inline::Text(format!(" {label}"))),
            }
            (cont_title.clone(), t)
        };
        let units = content::rejoin(g);
        let (p, w) = fits_some(&title_text, units.clone()).unwrap_or_else(|| {
            let c = Content {
                title: title_text.clone(),
                flows: vec![units],
                columns: false,
            };
            let (p, w, _) = choose(slide, &Role::Content, &c, ctx, total, &mut Vec::new());
            warnings.push(Warning::new(
                "slide-overflows",
                format!(
                    "slide {} ({}): one block does not fit any layout{min}",
                    slide.index, title_text
                ),
            ));
            (p, w)
        });
        out.push(Placement {
            plan: p,
            title: title_inl,
            notes: if k == 0 {
                slide.notes.clone()
            } else {
                Vec::new()
            },
            why: w,
            role: if k == 0 { role.clone() } else { Role::Content },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::read;

    fn slides(md: &str) -> Vec<Slide> {
        segment(&read(md)).unwrap()
    }

    #[test]
    fn headings_and_rules_cut_slides_and_notes_follow_their_slide() {
        let s = slides("# A\n\nx\n\n## B\n\ny\n\n---\n\n::: notes\nlate\n:::\n\n## C\n\nz\n");
        assert_eq!(s.len(), 3);
        assert_eq!(plain_text(&s[1].title), "B");
        assert_eq!(s[1].notes.len(), 1, "notes after the rule stay with B");
        assert_eq!(s[2].index, 3);
    }

    #[test]
    fn a_front_matter_title_no_slide_carries_becomes_the_cover() {
        let s = slides("---\ntitle: Deck\nauthor: Ada\ndate: 2026\n---\n\n## First\n\n- a\n");
        assert_eq!(s.len(), 2);
        assert_eq!(plain_text(&s[0].title), "Deck");
        assert!(s[0].title_only);
        assert_eq!(plain_text(&s[0].subtitle[0]), "Ada · 2026");
        assert_eq!(choose_role(&s[0], 2).unwrap(), Role::Cover);
    }

    #[test]
    fn roles_follow_hints_classes_and_form() {
        let md = "# Cover\n\n# Part {.section}\n\n## N {.bignumber}\n\n**+18 %**\n\n## L {layout=\"Two Content\"}\n\n- a\n\n## S\n\nSimplicity scales.\n\n## Q\n\n> Said.\n\n## F\n\n![Chart](x.png)\n\n## Ce que nous avons appris\n\n- a\n\n## C\n\n- a\n\n# Merci\n";
        let s = slides(md);
        let n = s.len();
        let roles: Vec<Role> = s.iter().map(|x| choose_role(x, n).unwrap()).collect();
        assert_eq!(roles[0], Role::Cover);
        assert_eq!(roles[1], Role::Section);
        assert_eq!(roles[2], Role::BigNumber);
        assert_eq!(roles[3], Role::Named("Two Content".into()));
        assert_eq!(roles[4], Role::Statement);
        assert_eq!(roles[5], Role::Quote);
        assert_eq!(roles[6], Role::Figure);
        assert_eq!(roles[7], Role::Recap);
        assert_eq!(roles[8], Role::Content);
        assert_eq!(roles[9], Role::Closing);
    }

    #[test]
    fn a_section_is_a_level_one_heading_with_nothing_to_place() {
        let s = slides("# Cover\n\n# Part two\n\nIn short.\n\n## Body\n\n- x\n\n# End\n");
        assert!(s[1].title_only);
        assert_eq!(plain_text(&s[1].subtitle[0]), "In short.");
        assert_eq!(choose_role(&s[1], s.len()).unwrap(), Role::Section);
    }

    #[test]
    fn the_continuation_label_follows_the_language() {
        assert_eq!(continuation_label(Some("fr-FR"), ""), "(suite)");
        assert_eq!(continuation_label(Some("en"), "le la les"), "(cont.)");
        assert_eq!(
            continuation_label(None, "la vie est belle et le monde aussi"),
            "(suite)"
        );
        assert_eq!(
            continuation_label(None, "the world and the rest"),
            "(cont.)"
        );
    }
}
