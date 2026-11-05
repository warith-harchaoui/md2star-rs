//! Does this content fit this layout? Arithmetic, not taste.
//!
//! A port of the deterministic half of md2star's `pptx_fit.py`, sharpened: instead of
//! an average character width, every paragraph is broken into lines with the face's
//! own advance widths ([`super::fonts`], [`super::linebreak`]) at the size the template
//! declares for its list level — resolved through layout and master — or at `--min-pt`
//! when that is larger. Never smaller: md2star-rs does not shrink text, it moves it.
//!
//! [`plan`] decides where every unit of a slide goes on one layout (title placeholder,
//! body placeholders, picture placeholders, or a stacked arrangement inside one box)
//! and measures it there. The plan *is* the placement the writer then emits, so the
//! measurement and the slide cannot disagree. A plan reports whether it fits (nothing
//! overruns its box at the chosen sizes) and a score that ranks the plans that fit:
//!
//! * **occupancy**: SlideGen's `1 − |ρ − 0.55|` (arXiv 2512.04529), ρ the share of the
//!   main text box the text fills — neither marooned nor crammed;
//! * **element count**: pictures matching picture placeholders score, empty picture
//!   placeholders and empty body boxes cost (a hole the reader sees);
//! * **picture size**: a picture contain-fitted into a bigger box scores more, so a
//!   figure goes where it comes out largest;
//! * **columns**: `::: columns` want as many boxes;
//! * **stacking** (text and a picture sharing one box) is allowed but ranks below a
//!   layout that gives each its own box.
//!
//! Default measures, documented: line height 1.2 × the size, a gap of 0.3 lines
//! between paragraphs, a bullet indent of 0.4 in per level, the template's own text
//! insets (0.1 in / 0.05 in when it declares none).

use super::content::{Bullet, Para, Picture, TableUnit, Unit};
use super::fonts::Measure;
use super::linebreak::line_count;
use super::template::{LayoutInfo, Placeholder, TemplateInfo};

/// EMU per point.
pub const EMU_PER_PT: f32 = 12_700.0;
/// EMU per inch.
pub const EMU_PER_IN: i64 = 914_400;
/// Single line spacing as a multiple of the size.
pub const LINE_SPACING: f32 = 1.2;
/// The gap between two paragraphs, in lines.
pub const PARAGRAPH_GAP: f32 = 0.3;
/// The indent of a bulleted paragraph per level, in points (0.4 in).
pub const BULLET_INDENT_PT: f32 = 28.8;
/// Under this size text is unreadable from the back of a room (md2star's audit floor):
/// md2star-rs never sets text smaller, whatever the template declares.
pub const LEGIBLE_MIN_PT: f32 = 12.0;
/// SlideGen's target occupancy.
pub const TARGET_OCCUPANCY: f32 = 0.55;
/// Default size of a code listing (points), raised to `--min-pt`.
pub const CODE_PT: f32 = 14.0;
/// Inner margins of the code panel: 0.25 in left and right, 0.1 in top and bottom.
pub const CODE_INSETS: (i64, i64) = (228_600, 91_440);
/// Cell margins of a table (PowerPoint's defaults: 0.1 in sides, 0.05 in top/bottom).
pub const CELL_INSETS: (i64, i64) = (91_440, 45_720);
/// What each extra line of a wrapped title costs a layout's score.
pub const TITLE_WRAP_COST: f32 = 0.15;
/// Vertical gap between stacked elements in one box (0.1 in).
pub const STACK_GAP: i64 = 91_440;

/// What role a layout plays, read off its name and its placeholder geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archetype {
    /// A centred title (`ctrTitle`) or a layout named like a title slide.
    Cover,
    /// A section divider.
    Section,
    /// A closing slide.
    Closing,
    /// A title and nothing else to fill.
    TitleOnly,
    /// Title and one body box.
    Content,
    /// Several body boxes.
    MultiColumn,
    /// A picture placeholder beside text.
    Picture,
    /// A full-bleed picture (80 % of the slide or more): only on request (`.photo`).
    Photo,
    /// Vertical text: never chosen automatically.
    Vertical,
    /// Nothing to fill.
    Blank,
    /// Brand reference material (palette, logos, icons): never a home for content.
    Reference,
}

impl Archetype {
    /// A word for the archetype (prompts, `templates layouts`).
    pub fn label(self) -> &'static str {
        match self {
            Archetype::Cover => "cover",
            Archetype::Section => "section divider",
            Archetype::Closing => "closing",
            Archetype::TitleOnly => "title only",
            Archetype::Content => "title and content",
            Archetype::MultiColumn => "several text boxes",
            Archetype::Picture => "picture and text",
            Archetype::Photo => "full-bleed photo",
            Archetype::Vertical => "vertical text",
            Archetype::Blank => "blank",
            Archetype::Reference => "brand reference",
        }
    }

    /// Whether ordinary content may land on this layout without being asked for it.
    pub fn automatic(self) -> bool {
        matches!(
            self,
            Archetype::TitleOnly | Archetype::Content | Archetype::MultiColumn | Archetype::Picture
        )
    }
}

/// The archetype of `layout` on a canvas of `canvas` EMU.
pub fn archetype(layout: &LayoutInfo, canvas: (i64, i64)) -> Archetype {
    let name = layout.name.to_lowercase();
    let has = |f: fn(&Placeholder) -> bool| layout.placeholders.iter().any(f);
    if ["colour", "color", "palette", "logo", "icon", "brand guide"]
        .iter()
        .any(|h| name.contains(h))
    {
        return Archetype::Reference;
    }
    if layout
        .placeholders
        .iter()
        .any(|p| p.vertical && !p.is_latent())
    {
        return Archetype::Vertical;
    }
    if name.contains("title slide") || has(|p| p.kind == "ctrTitle") {
        return Archetype::Cover;
    }
    if name.contains("section") {
        return Archetype::Section;
    }
    if ["closing", "thank", "merci", "the end"]
        .iter()
        .any(|h| name.contains(h))
    {
        return Archetype::Closing;
    }
    let bodies = layout.placeholders.iter().filter(|p| p.is_body()).count();
    let pics: Vec<&Placeholder> = layout
        .placeholders
        .iter()
        .filter(|p| p.is_picture())
        .collect();
    let canvas_area = (canvas.0 as f64 * canvas.1 as f64).max(1.0);
    if pics.iter().any(|p| {
        p.frame
            .is_some_and(|f| (f.2 as f64 * f.3 as f64) >= 0.8 * canvas_area)
    }) {
        return Archetype::Photo;
    }
    if !pics.is_empty() {
        return Archetype::Picture;
    }
    let has_title = has(Placeholder::is_title);
    match (has_title, bodies) {
        (false, 0) => Archetype::Blank,
        (true, 0) => Archetype::TitleOnly,
        (_, 1) => Archetype::Content,
        _ => Archetype::MultiColumn,
    }
}

/// Sizes and faces: how text is set on this deck.
#[derive(Debug, Clone)]
pub struct Typo {
    /// `--min-pt`.
    pub min_pt: Option<f32>,
    /// The monospaced face for code.
    pub mono_font: String,
    /// The size occupancy is *ranked* at (never set at): the template's own body size
    /// on its canonical content layout. Without it a caption box declared at 12 pt
    /// looks roomier than the content box at 18 pt, and a whole list lands in the
    /// caption. md2star ranks at its 30 pt Kawasaki floor for the same reason.
    pub rank_pt: f32,
}

impl Typo {
    /// The size text declared at `declared` points is actually set at: never under
    /// `--min-pt`, never under the legibility floor.
    pub fn eff(&self, declared: f32) -> f32 {
        declared.max(self.min_pt.unwrap_or(0.0)).max(LEGIBLE_MIN_PT)
    }

    /// The size of a code listing.
    pub fn code_pt(&self) -> f32 {
        self.eff(CODE_PT)
    }

    /// The measure for code.
    pub fn mono(&self) -> Measure {
        Measure::for_family(Some(&self.mono_font), false, true)
    }
}

/// The measure for a placeholder's text.
pub fn measure_of(ph: &Placeholder, bold: bool) -> Measure {
    Measure::for_family(ph.typeface.as_deref(), bold || ph.bold, false)
}

/// The usable width (points) of a box for text, insets removed.
pub fn usable_width_pt(frame: (i64, i64, i64, i64), insets: (i64, i64, i64, i64)) -> f32 {
    ((frame.2 - insets.0 - insets.2) as f32 / EMU_PER_PT).max(1.0)
}

/// The usable height (points) of a box for text, insets removed.
pub fn usable_height_pt(frame: (i64, i64, i64, i64), insets: (i64, i64, i64, i64)) -> f32 {
    ((frame.3 - insets.1 - insets.3) as f32 / EMU_PER_PT).max(0.0)
}

/// The size of a paragraph set in `ph` (effective, points).
pub fn para_size(p: &Para, ph: &Placeholder, typo: &Typo) -> f32 {
    typo.eff(ph.size_pt(p.level))
}

/// A placeholder's `(marL, indent)` at `level`, scaled by how much the floor raised the
/// size: a bullet set larger needs a proportionally wider hanging indent, or the glyph
/// runs into the text.
pub fn scaled_margin(ph: &Placeholder, level: usize, typo: &Typo) -> (i64, i64) {
    let lvl = level.min(8);
    let declared = ph.size_pt(lvl).max(1.0);
    let ratio = typo.eff(declared) / declared;
    let (m, i) = ph.margins[lvl];
    (
        (m as f32 * ratio).round() as i64,
        (i as f32 * ratio).round() as i64,
    )
}

/// Where a paragraph's text starts, in points from the box's inner left edge: the
/// writer's own margins (prose: 0.5 in per level; a free box's bullet: 0.375 in per
/// level) or the placeholder's, scaled.
pub fn indent_pt(p: &Para, ph: Option<&Placeholder>, typo: &Typo) -> f32 {
    match (ph, p.bullet) {
        (_, Bullet::None | Bullet::Task(_)) => 36.0 * p.level as f32,
        (Some(ph), _) => {
            let (m, _) = scaled_margin(ph, p.level, typo);
            if m > 0 {
                m as f32 / EMU_PER_PT
            } else {
                BULLET_INDENT_PT * (p.level as f32 + 1.0)
            }
        }
        (None, _) => 27.0 * (p.level as f32 + 1.0),
    }
}

/// The width left for text in a box `width_pt` wide once `indent` points are taken.
pub fn para_width(width_pt: f32, indent: f32) -> f32 {
    (width_pt - indent).max(width_pt * 0.3)
}

/// Points of height `paras` need in a text box `width_pt` wide, each set at
/// `size_of(para)`, starting `indent_of(para)` points in.
pub fn paras_height(
    paras: &[&Para],
    width_pt: f32,
    size_of: &dyn Fn(&Para) -> f32,
    measure_of: &dyn Fn(&Para) -> Measure,
    indent_of: &dyn Fn(&Para) -> f32,
) -> f32 {
    let mut h = 0.0;
    for (k, p) in paras.iter().enumerate() {
        let size = size_of(p);
        let width = para_width(width_pt, indent_of(p));
        let lines = line_count(&p.text(), size, width, &measure_of(p));
        h += lines as f32 * size * LINE_SPACING;
        if k > 0 {
            h += PARAGRAPH_GAP * size * LINE_SPACING;
        }
    }
    h
}

/// The column widths (EMU) of a table `width` EMU wide: proportional to each column's
/// longest cell, none under a sixth of an even share.
pub fn column_widths(t: &TableUnit, width: i64) -> Vec<i64> {
    let cols = t.cols().max(1);
    let mut weight = vec![1.0f32; cols];
    for row in std::iter::once(&t.headers).chain(t.rows.iter()) {
        for (c, cell) in row.iter().enumerate() {
            let len = super::content::printed(cell).chars().count() as f32;
            weight[c] = weight[c].max(len.min(60.0));
        }
    }
    let even = 1.0 / cols as f32;
    let total: f32 = weight.iter().sum();
    let mut shares: Vec<f32> = weight.iter().map(|w| (w / total).max(even / 6.0)).collect();
    let norm: f32 = shares.iter().sum();
    for s in &mut shares {
        *s /= norm;
    }
    let mut widths: Vec<i64> = shares
        .iter()
        .map(|s| (*s * width as f32).floor() as i64)
        .collect();
    let used: i64 = widths.iter().sum();
    if let Some(last) = widths.last_mut() {
        *last += width - used;
    }
    widths
}

/// The height (EMU) of each row of `t` set at `size` points in a table `width` EMU wide.
pub fn row_heights(t: &TableUnit, width: i64, size: f32, measure: &Measure) -> Vec<i64> {
    let widths = column_widths(t, width);
    std::iter::once(&t.headers)
        .chain(t.rows.iter())
        .map(|row| {
            let lines = row
                .iter()
                .enumerate()
                .map(|(c, cell)| {
                    let w = (widths.get(c).copied().unwrap_or(width) - 2 * CELL_INSETS.0) as f32
                        / EMU_PER_PT;
                    line_count(&super::content::printed(cell), size, w.max(1.0), measure)
                })
                .max()
                .unwrap_or(1)
                .max(1);
            (lines as f32 * size * LINE_SPACING * EMU_PER_PT) as i64 + 2 * CELL_INSETS.1
        })
        .collect()
}

/// The height (EMU) a code listing needs in a panel `width` EMU wide.
pub fn code_height(lines: &[String], width: i64, typo: &Typo) -> i64 {
    let size = typo.code_pt();
    let w = ((width - 2 * CODE_INSETS.0) as f32 / EMU_PER_PT).max(1.0);
    let measure = typo.mono();
    let n: usize = lines
        .iter()
        .map(|l| line_count(l, size, w, &measure))
        .sum::<usize>()
        .max(1);
    (n as f32 * size * LINE_SPACING * EMU_PER_PT) as i64 + 2 * CODE_INSETS.1
}

/// Contain-fit a picture of aspect `aspect` into `frame`, centred.
pub fn contain(aspect: f64, frame: (i64, i64, i64, i64)) -> (i64, i64, i64, i64) {
    let (x, y, w, h) = frame;
    let (w, h) = (w.max(1) as f64, h.max(1) as f64);
    let (pw, ph) = if w / h > aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    let (pw, ph) = (pw.round() as i64, ph.round() as i64);
    (
        x + (w as i64 - pw) / 2,
        y + (h as i64 - ph) / 2,
        pw.max(1),
        ph.max(1),
    )
}

/// One thing placed on a slide.
#[derive(Debug, Clone, PartialEq)]
pub enum Placed {
    /// Paragraphs written into a layout placeholder (index into the layout's
    /// placeholders); `frame` is set when the box had to grow (title-only slides).
    Text {
        /// The placeholder.
        ph: usize,
        /// An explicit box, when it differs from the layout's.
        frame: Option<(i64, i64, i64, i64)>,
        /// The paragraphs.
        paras: Vec<Para>,
    },
    /// Paragraphs in a free text box at a fixed size (stacked content, statements).
    Free {
        /// The box.
        frame: (i64, i64, i64, i64),
        /// The paragraphs.
        paras: Vec<Para>,
        /// The size of level 0 (deeper levels keep the template's step down).
        size: f32,
        /// The size step of each level below the first, from the template.
        levels: [f32; 9],
        /// Centred (a statement) or flush left.
        center: bool,
    },
    /// A table.
    Table {
        /// Its frame.
        frame: (i64, i64, i64, i64),
        /// The rows.
        table: TableUnit,
        /// Text size.
        size: f32,
    },
    /// A code panel.
    Code {
        /// Its frame.
        frame: (i64, i64, i64, i64),
        /// The lines.
        lines: Vec<String>,
    },
    /// A picture, contain-fitted.
    Picture {
        /// Its frame (already fitted).
        frame: (i64, i64, i64, i64),
        /// The picture.
        pic: Picture,
    },
}

/// Where a slide's content goes on one layout, and how well it sits there.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Index of the layout in the template.
    pub layout: usize,
    /// The title placeholder (index into the layout's placeholders).
    pub title_ph: Option<usize>,
    /// The title's box, when it had to differ from the layout's.
    pub title_frame: Option<(i64, i64, i64, i64)>,
    /// Everything else.
    pub placed: Vec<Placed>,
    /// Nothing overruns its box at the sizes it is set at.
    pub fits: bool,
    /// Rank among the plans that fit (higher is better).
    pub score: f32,
    /// Why, in words.
    pub reasons: Vec<String>,
    /// How full the main text box is (ρ), for the record.
    pub occupancy: f32,
}

/// The slide content a plan places.
#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    /// The title as printed (empty: untitled).
    pub title: String,
    /// One flow, or one per column.
    pub flows: Vec<Vec<Unit>>,
    /// The flows are columns.
    pub columns: bool,
}

impl Content {
    /// Whether the slide carries nothing but its title.
    pub fn is_empty(&self) -> bool {
        self.flows.iter().all(Vec::is_empty)
    }
}

fn frame_of(ph: &Placeholder, info: &TemplateInfo) -> (i64, i64, i64, i64) {
    ph.frame.unwrap_or_else(|| {
        let (w, h) = info.slide_size;
        (w / 16, h / 4, w * 7 / 8, h * 5 / 8)
    })
}

fn area(f: (i64, i64, i64, i64)) -> f64 {
    f.2.max(0) as f64 * f.3.max(0) as f64
}

/// The title placeholder of a layout, if any.
pub fn title_index(layout: &LayoutInfo) -> Option<usize> {
    layout
        .placeholders
        .iter()
        .position(|p| p.kind == "ctrTitle")
        .or_else(|| layout.placeholders.iter().position(|p| p.kind == "title"))
}

/// Body placeholders (not vertical), largest first.
pub fn bodies(layout: &LayoutInfo, info: &TemplateInfo) -> Vec<usize> {
    let mut v: Vec<usize> = (0..layout.placeholders.len())
        .filter(|&i| layout.placeholders[i].is_body() && !layout.placeholders[i].vertical)
        .collect();
    v.sort_by(|&a, &b| {
        area(frame_of(&layout.placeholders[b], info))
            .total_cmp(&area(frame_of(&layout.placeholders[a], info)))
            .then(a.cmp(&b))
    });
    v
}

/// Picture placeholders, largest first.
pub fn pictures(layout: &LayoutInfo, info: &TemplateInfo) -> Vec<usize> {
    let mut v: Vec<usize> = (0..layout.placeholders.len())
        .filter(|&i| layout.placeholders[i].is_picture())
        .collect();
    v.sort_by(|&a, &b| {
        area(frame_of(&layout.placeholders[b], info))
            .total_cmp(&area(frame_of(&layout.placeholders[a], info)))
            .then(a.cmp(&b))
    });
    v
}

/// Does the title fit its box? Returns (fits, needed_pt, available_pt).
/// Whether the title fits its box: `(fits, lines, needed height in points)`.
fn title_fits(
    title: &str,
    ph: &Placeholder,
    info: &TemplateInfo,
    typo: &Typo,
) -> (bool, usize, f32) {
    if title.is_empty() {
        return (true, 0, 0.0);
    }
    let frame = frame_of(ph, info);
    let size = typo.eff(ph.size_pt(0));
    let w = usable_width_pt(frame, ph.insets);
    let lines = line_count(title, size, w, &measure_of(ph, false));
    let needed = lines as f32 * size * LINE_SPACING;
    // A title can always take the slack its box was drawn with: titles are anchored
    // and padded generously, and a one-line overshoot of the inset is not an overrun.
    (needed <= (frame.3 as f32 / EMU_PER_PT), lines, needed)
}

/// A title box grown upward (towards the top margin, keeping its bottom edge where the
/// designer put it) until `needed` points fit; `None` when even that is not enough.
fn grown_title(ph: &Placeholder, needed: f32, info: &TemplateInfo) -> Option<(i64, i64, i64, i64)> {
    let frame = frame_of(ph, info);
    let want = (needed * EMU_PER_PT) as i64;
    let top_margin = info.slide_size.1 / 40;
    let bottom = frame.1 + frame.3;
    let top = bottom - want;
    (top >= top_margin && top < frame.1).then_some((frame.0, top, frame.2, want))
}

/// Lay a mixed sequence of units out top to bottom inside `frame`. Returns the placed
/// elements and whether they fit.
pub fn stack(
    units: &[Unit],
    frame: (i64, i64, i64, i64),
    ph: &Placeholder,
    typo: &Typo,
) -> (Vec<Placed>, bool) {
    let (x, y, w, h) = frame;
    let inner_w = (w - ph.insets.0 - ph.insets.2).max(1);
    let inner_x = x + ph.insets.0;
    // Group consecutive paragraphs into one text box each.
    let mut groups: Vec<Vec<&Unit>> = Vec::new();
    for u in units {
        match (groups.last_mut(), u) {
            (Some(g), Unit::Para(_)) if g.last().is_some_and(|l| l.is_para()) => g.push(u),
            _ => groups.push(vec![u]),
        }
    }
    let levels: [f32; 9] = std::array::from_fn(|l| typo.eff(ph.size_pt(l)));
    let mut fixed: Vec<i64> = Vec::new();
    let mut pictures = 0usize;
    for g in &groups {
        match g[0] {
            Unit::Para(_) => {
                let paras: Vec<&Para> = g
                    .iter()
                    .filter_map(|u| match u {
                        Unit::Para(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                let width_pt = inner_w as f32 / EMU_PER_PT - 14.4;
                let hh = paras_height(
                    &paras,
                    width_pt,
                    &|p| levels[p.level.min(8)],
                    &|p| measure_of(ph, p.bold),
                    &|p| indent_pt(p, None, typo),
                );
                fixed.push((hh * EMU_PER_PT) as i64 + 91_440);
            }
            Unit::Table(t) => {
                let size = levels[0];
                fixed.push(
                    row_heights(t, inner_w, size, &measure_of(ph, false))
                        .iter()
                        .sum(),
                );
            }
            Unit::Code(lines) => fixed.push(code_height(lines, inner_w, typo)),
            Unit::Picture(_) => {
                pictures += 1;
                fixed.push(0);
            }
        }
    }
    let gaps = STACK_GAP * (groups.len().saturating_sub(1) as i64);
    let used: i64 = fixed.iter().sum::<i64>() + gaps;
    let left = h - used;
    let per_picture = if pictures > 0 {
        left / pictures as i64
    } else {
        0
    };
    let mut fits = used <= h;
    if pictures > 0 && per_picture < (h / 5).max(EMU_PER_IN * 3 / 4) {
        fits = false;
    }
    let mut out = Vec::new();
    let mut cy = y;
    for (g, hh) in groups.iter().zip(fixed) {
        match g[0] {
            Unit::Para(_) => {
                let paras: Vec<Para> = g
                    .iter()
                    .filter_map(|u| match u {
                        Unit::Para(p) => Some(p.clone()),
                        _ => None,
                    })
                    .collect();
                out.push(Placed::Free {
                    frame: (inner_x, cy, inner_w, hh),
                    paras,
                    size: levels[0],
                    levels,
                    center: false,
                });
                cy += hh;
            }
            Unit::Table(t) => {
                out.push(Placed::Table {
                    frame: (inner_x, cy, inner_w, hh),
                    table: t.clone(),
                    size: levels[0],
                });
                cy += hh;
            }
            Unit::Code(lines) => {
                out.push(Placed::Code {
                    frame: (inner_x, cy, inner_w, hh),
                    lines: lines.clone(),
                });
                cy += hh;
            }
            Unit::Picture(p) => {
                let slot = (inner_x, cy, inner_w, per_picture.max(1));
                out.push(Placed::Picture {
                    frame: contain(p.aspect, slot),
                    pic: p.clone(),
                });
                cy += per_picture.max(1);
            }
        }
        cy += STACK_GAP;
    }
    (out, fits)
}

/// Place `units` into one body placeholder: straight into the placeholder when they
/// are all paragraphs, a table or a listing alone at the top of the box, stacked
/// otherwise. Returns (placed, fits, occupancy).
fn fill_box(
    units: &[Unit],
    ph_index: usize,
    layout: &LayoutInfo,
    info: &TemplateInfo,
    typo: &Typo,
) -> (Vec<Placed>, bool, f32) {
    let ph = &layout.placeholders[ph_index];
    let frame = frame_of(ph, info);
    let avail = usable_height_pt(frame, ph.insets).max(1.0);
    if units.iter().all(Unit::is_para) {
        let paras: Vec<&Para> = units
            .iter()
            .filter_map(|u| match u {
                Unit::Para(p) => Some(p),
                _ => None,
            })
            .collect();
        let width = usable_width_pt(frame, ph.insets);
        let needed = paras_height(
            &paras,
            width,
            &|p| para_size(p, ph, typo),
            &|p| measure_of(ph, p.bold),
            &|p| indent_pt(p, Some(ph), typo),
        );
        let ranked = paras_height(
            &paras,
            width,
            &|p| para_size(p, ph, typo).max(typo.rank_pt),
            &|p| measure_of(ph, p.bold),
            &|p| indent_pt(p, Some(ph), typo),
        );
        return (
            vec![Placed::Text {
                ph: ph_index,
                frame: None,
                paras: paras.into_iter().cloned().collect(),
            }],
            needed <= avail,
            ranked / avail,
        );
    }
    if let [Unit::Picture(p)] = units {
        return (
            vec![Placed::Picture {
                frame: contain(p.aspect, frame),
                pic: p.clone(),
            }],
            true,
            TARGET_OCCUPANCY,
        );
    }
    let (placed, fits) = stack(units, frame, ph, typo);
    let used: i64 = placed
        .iter()
        .map(|p| match p {
            Placed::Free { frame, .. }
            | Placed::Table { frame, .. }
            | Placed::Code { frame, .. }
            | Placed::Picture { frame, .. } => frame.3,
            Placed::Text { .. } => 0,
        })
        .sum();
    (placed, fits, used as f32 / frame.3.max(1) as f32)
}

/// Plan `content` on layout number `index`.
pub fn plan(content: &Content, index: usize, info: &TemplateInfo, typo: &Typo) -> Plan {
    let layout = &info.layouts[index];
    let canvas_area = area((0, 0, info.slide_size.0, info.slide_size.1)).max(1.0);
    let mut p = Plan {
        layout: index,
        title_ph: title_index(layout),
        title_frame: None,
        placed: Vec::new(),
        fits: true,
        score: 0.0,
        reasons: Vec::new(),
        occupancy: 0.0,
    };
    let mut score = 0.0f32;
    if !content.title.is_empty() {
        match p.title_ph {
            None => {
                p.fits = false;
                p.reasons.push("no title placeholder".into());
            }
            Some(t) => {
                let ph = &layout.placeholders[t];
                let (ok, lines, needed) = title_fits(&content.title, ph, info, typo);
                if lines > 1 {
                    // A title broken over lines reads worse than one that is not.
                    score -= TITLE_WRAP_COST * (lines - 1) as f32;
                }
                if !ok {
                    match grown_title(ph, needed, info) {
                        Some(f) => {
                            p.title_frame = Some(f);
                            p.reasons.push("the title box grows upward".into());
                        }
                        None => {
                            p.fits = false;
                            p.reasons.push("the title does not fit its box".into());
                        }
                    }
                }
            }
        }
    }
    let body_ix = bodies(layout, info);
    let pic_ix = pictures(layout, info);
    if content.columns {
        // The N largest boxes, read left to right.
        let mut boxes: Vec<usize> = body_ix.iter().chain(pic_ix.iter()).copied().collect();
        boxes.sort_by(|&a, &b| {
            area(frame_of(&layout.placeholders[b], info))
                .total_cmp(&area(frame_of(&layout.placeholders[a], info)))
                .then(a.cmp(&b))
        });
        let n = content.flows.len();
        if boxes.len() < n {
            p.fits = false;
            p.reasons
                .push(format!("{n} columns but {} boxes", boxes.len()));
            score -= 0.2;
        } else {
            boxes.truncate(n);
            boxes.sort_by_key(|&b| {
                let f = frame_of(&layout.placeholders[b], info);
                (f.0, f.1)
            });
            score += 0.35;
            let mut occ = Vec::new();
            for (flow, &b) in content.flows.iter().zip(&boxes) {
                let ph = &layout.placeholders[b];
                if ph.is_picture() {
                    let only_pics = flow.iter().all(|u| matches!(u, Unit::Picture(_)));
                    if !only_pics {
                        p.fits = false;
                        p.reasons.push("text in a picture placeholder".into());
                        continue;
                    }
                    let (placed, _) = stack(flow, frame_of(ph, info), ph, typo);
                    p.placed.extend(placed);
                    continue;
                }
                let (placed, fits, o) = fill_box(flow, b, layout, info, typo);
                if !fits {
                    p.fits = false;
                    p.reasons.push("a column overruns its box".into());
                }
                occ.push(o);
                p.placed.extend(placed);
            }
            p.occupancy = if occ.is_empty() {
                TARGET_OCCUPANCY
            } else {
                occ.iter().sum::<f32>() / occ.len() as f32
            };
            let unused = body_ix.iter().filter(|b| !boxes.contains(b)).count()
                + pic_ix.iter().filter(|b| !boxes.contains(b)).count();
            score -= 0.4 * unused as f32;
        }
    } else {
        let flow: &[Unit] = content.flows.first().map(Vec::as_slice).unwrap_or(&[]);
        let pics: Vec<&Unit> = flow
            .iter()
            .filter(|u| matches!(u, Unit::Picture(_)))
            .collect();
        let others: Vec<Unit> = flow
            .iter()
            .filter(|u| !matches!(u, Unit::Picture(_)))
            .cloned()
            .collect();
        let mut free_bodies: Vec<usize> = body_ix.clone();
        let text_box = if others.is_empty() {
            None
        } else {
            (!free_bodies.is_empty()).then(|| free_bodies.remove(0))
        };
        // Pictures: picture placeholders first, then spare body boxes, then the text
        // box (stacked under the text).
        let mut pic_boxes = pic_ix.clone();
        let mut stacked: Vec<Unit> = Vec::new();
        let mut used_pics = 0;
        for u in &pics {
            let Unit::Picture(pic) = u else { continue };
            if !pic_boxes.is_empty() {
                let b = pic_boxes.remove(0);
                let f = contain(pic.aspect, frame_of(&layout.placeholders[b], info));
                score += 0.6 * (area(f) / canvas_area) as f32;
                p.placed.push(Placed::Picture {
                    frame: f,
                    pic: pic.clone(),
                });
                used_pics += 1;
            } else if !free_bodies.is_empty() {
                let b = free_bodies.remove(0);
                let f = contain(pic.aspect, frame_of(&layout.placeholders[b], info));
                score += 0.6 * (area(f) / canvas_area) as f32;
                p.placed.push(Placed::Picture {
                    frame: f,
                    pic: pic.clone(),
                });
            } else {
                stacked.push((*u).clone());
            }
        }
        if !pics.is_empty() && used_pics == pics.len() && used_pics == pic_ix.len() {
            score += 0.3;
            p.reasons.push(format!(
                "picture placeholders match the {used_pics} image(s)"
            ));
        }
        score -= 0.2 * pic_boxes.len() as f32;
        if !pic_boxes.is_empty() {
            p.reasons.push(format!(
                "{} picture placeholder(s) would stay empty",
                pic_boxes.len()
            ));
        }
        match text_box {
            Some(b) => {
                let mut units = others.clone();
                if !stacked.is_empty() {
                    score -= 0.3;
                    p.reasons.push("text and picture share one box".into());
                    units.append(&mut stacked);
                }
                let (placed, fits, occ) = fill_box(&units, b, layout, info, typo);
                if !fits {
                    p.fits = false;
                    p.reasons
                        .push(format!("the text needs {:.0} % of its box", occ * 100.0));
                }
                p.occupancy = occ;
                p.placed.extend(placed);
            }
            None if !others.is_empty() => {
                p.fits = false;
                p.reasons.push("text but no body placeholder".into());
            }
            None => {
                p.occupancy = if pics.is_empty() {
                    0.0
                } else {
                    TARGET_OCCUPANCY
                };
            }
        }
        if !stacked.is_empty() {
            p.fits = false;
            p.reasons.push("a picture has nowhere to go".into());
        }
        if !free_bodies.is_empty() {
            score -= 0.6 * free_bodies.len() as f32;
            p.reasons.push(format!(
                "{} body box(es) would stay empty",
                free_bodies.len()
            ));
        }
    }
    if !content.is_empty() || !content.title.is_empty() {
        score += 1.0 - (p.occupancy - TARGET_OCCUPANCY).abs();
    }
    p.score = score;
    p
}

/// The free region under a layout's title, above the footer band: where a statement,
/// a big number or a quotation is set when the template has no layout for it.
pub fn stage(layout: &LayoutInfo, info: &TemplateInfo) -> (i64, i64, i64, i64) {
    let (w, h) = info.slide_size;
    let margin_x = w / 16;
    let top = title_index(layout)
        .and_then(|t| layout.placeholders[t].frame)
        .map(|f| f.1 + f.3 + h / 40)
        .unwrap_or(h / 8);
    let bottom = info.footer_top.min(h * 92 / 100) - h / 40;
    let x = title_index(layout)
        .and_then(|t| layout.placeholders[t].frame)
        .map(|f| f.0)
        .unwrap_or(margin_x);
    let width = title_index(layout)
        .and_then(|t| layout.placeholders[t].frame)
        .map(|f| f.2)
        .unwrap_or(w - 2 * margin_x);
    (x, top, width, (bottom - top).max(h / 6))
}

/// Set `paras` large in the stage of `layout`: the largest of `sizes` (each raised to
/// the floors) at which they fit. Returns the plan, or `None` when even the smallest
/// does not fit.
pub fn stage_plan(
    content_title: &str,
    paras: &[Para],
    sizes: &[f32],
    index: usize,
    info: &TemplateInfo,
    typo: &Typo,
) -> Option<Plan> {
    let layout = &info.layouts[index];
    let frame = stage(layout, info);
    let measure = Measure::for_family(info.theme_fonts.0.as_deref(), true, false);
    let w = (frame.2 - 182_880) as f32 / EMU_PER_PT;
    let refs: Vec<&Para> = paras.iter().collect();
    for &s in sizes {
        let size = typo.eff(s);
        let needed = paras_height(&refs, w, &|_| size, &|_| measure.clone(), &|_| 0.0);
        if needed <= (frame.3 - 91_440) as f32 / EMU_PER_PT {
            let (ok, _, _) = title_index(layout)
                .map(|t| title_fits(content_title, &layout.placeholders[t], info, typo))
                .unwrap_or((content_title.is_empty(), 0, 0.0));
            if !ok {
                return None;
            }
            return Some(Plan {
                layout: index,
                title_ph: title_index(layout),
                title_frame: None,
                placed: vec![Placed::Free {
                    frame,
                    paras: paras.to_vec(),
                    size,
                    levels: [size; 9],
                    center: true,
                }],
                fits: true,
                score: 2.0,
                reasons: vec![format!("set at {size:.0} pt on the stage under the title")],
                occupancy: TARGET_OCCUPANCY,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pptx::template::inspect;

    fn info() -> TemplateInfo {
        inspect(crate::templates::DEFAULT_PPTX).unwrap()
    }

    fn typo(min: Option<f32>) -> Typo {
        Typo {
            min_pt: min,
            mono_font: "Consolas".into(),
            rank_pt: 0.0,
        }
    }

    fn bullets(n: usize, words: usize) -> Content {
        let item = vec!["word"; words].join(" ");
        Content {
            title: "Title".into(),
            flows: vec![(0..n)
                .map(|_| {
                    Unit::Para(Para {
                        bullet: Bullet::Char,
                        ..Para::plain(vec![crate::ast::Inline::Text(item.clone())])
                    })
                })
                .collect()],
            columns: false,
        }
    }

    fn index(info: &TemplateInfo, name: &str) -> usize {
        info.layouts.iter().position(|l| l.name == name).unwrap()
    }

    #[test]
    fn archetypes_come_from_names_and_geometry() {
        let info = info();
        let a = |n: &str| archetype(&info.layouts[index(&info, n)], info.slide_size);
        assert_eq!(a("Title Slide"), Archetype::Cover);
        assert_eq!(a("Section Header"), Archetype::Section);
        assert_eq!(a("Title and Content"), Archetype::Content);
        assert_eq!(a("Two Content"), Archetype::MultiColumn);
        assert_eq!(a("Title Only"), Archetype::TitleOnly);
        assert_eq!(a("Blank"), Archetype::Blank);
        assert_eq!(a("Picture with Caption"), Archetype::Picture);
        assert_eq!(a("Title and Vertical Text"), Archetype::Vertical);
    }

    #[test]
    fn a_floor_never_lowers_a_size_and_twelve_is_the_least() {
        let t = typo(Some(30.0));
        assert_eq!(t.eff(18.0), 30.0);
        assert_eq!(t.eff(40.0), 40.0);
        assert_eq!(typo(None).eff(9.0), LEGIBLE_MIN_PT);
    }

    #[test]
    fn three_short_bullets_fit_and_thirty_long_ones_do_not() {
        let info = info();
        let i = index(&info, "Title and Content");
        assert!(plan(&bullets(3, 4), i, &info, &typo(Some(30.0))).fits);
        let crowded = plan(&bullets(30, 12), i, &info, &typo(Some(30.0)));
        assert!(!crowded.fits);
        assert!(crowded.reasons.iter().any(|r| r.contains("of its box")));
    }

    #[test]
    fn a_floor_makes_the_same_text_need_more_room() {
        let info = info();
        let i = index(&info, "Title and Content");
        let c = bullets(5, 10);
        let at18 = plan(&c, i, &info, &typo(None));
        let at30 = plan(&c, i, &info, &typo(Some(30.0)));
        assert!(at30.occupancy > at18.occupancy);
    }

    #[test]
    fn one_flow_on_two_boxes_costs_an_empty_box() {
        let info = info();
        let c = bullets(3, 4);
        let one = plan(&c, index(&info, "Title and Content"), &info, &typo(None));
        let two = plan(&c, index(&info, "Two Content"), &info, &typo(None));
        assert!(one.score > two.score);
    }

    #[test]
    fn a_picture_is_contained_in_its_box() {
        assert_eq!(contain(2.0, (0, 0, 100, 100)), (0, 25, 100, 50));
        assert_eq!(contain(0.5, (0, 0, 100, 100)), (25, 0, 50, 100));
    }

    #[test]
    fn table_columns_share_the_width_exactly() {
        let t = TableUnit {
            headers: vec![vec![crate::ast::Inline::Text("Region".into())]; 3],
            rows: vec![],
            aligns: vec![],
        };
        let w = column_widths(&t, 1_000_001);
        assert_eq!(w.iter().sum::<i64>(), 1_000_001);
    }
}
