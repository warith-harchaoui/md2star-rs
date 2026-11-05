//! Paragraph, character and table styles: the ones the writer uses, their default
//! definitions, and how they are found in (or added to) a reference document.
//!
//! The writer names Pandoc's styles (`BodyText`, `FirstParagraph`, `Compact`, `Title`,
//! `Author`, `ImageCaption`, `Bibliography`, `SourceCode`, `VerbatimChar`, `Hyperlink`…)
//! so a reference document made for Pandoc or md2star — which defines exactly these —
//! styles the output. A style is looked up in the reference by id, then by name (a Word
//! installed in French calls `Heading1` `Titre1` but still names it `heading 1`); a style
//! the reference lacks is added from the definitions below, with the styles it is based
//! on, as Pandoc does. Nothing is ever referenced that `styles.xml` does not define.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A style the writer can use: its id, its name, what it is based on, and its XML.
struct Def {
    id: &'static str,
    name: &'static str,
    based_on: Option<&'static str>,
    xml: &'static str,
}

/// Every style the writer may reference.
const DEFS: &[Def] = &[
    Def {
        id: "Normal",
        name: "Normal",
        based_on: None,
        xml: r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="120" w:line="264" w:lineRule="auto"/></w:pPr></w:style>"#,
    },
    Def {
        id: "BodyText",
        name: "Body Text",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="BodyText"><w:name w:val="Body Text"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:before="120" w:after="120"/></w:pPr></w:style>"#,
    },
    Def {
        id: "FirstParagraph",
        name: "First Paragraph",
        based_on: Some("BodyText"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="FirstParagraph"><w:name w:val="First Paragraph"/><w:basedOn w:val="BodyText"/><w:next w:val="BodyText"/><w:qFormat/></w:style>"#,
    },
    Def {
        id: "Compact",
        name: "Compact",
        based_on: Some("BodyText"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="Compact"><w:name w:val="Compact"/><w:basedOn w:val="BodyText"/><w:qFormat/><w:pPr><w:spacing w:before="36" w:after="36"/></w:pPr></w:style>"#,
    },
    Def {
        id: "Title",
        name: "Title",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="480" w:after="240"/><w:jc w:val="center"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:b/><w:bCs/><w:color w:val="345A8A"/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Subtitle",
        name: "Subtitle",
        based_on: Some("Title"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/><w:basedOn w:val="Title"/><w:next w:val="BodyText"/><w:qFormat/><w:pPr><w:spacing w:before="240" w:after="240"/></w:pPr><w:rPr><w:b w:val="0"/><w:bCs w:val="0"/><w:sz w:val="30"/><w:szCs w:val="30"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Author",
        name: "Author",
        based_on: None,
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="Author"><w:name w:val="Author"/><w:next w:val="BodyText"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:jc w:val="center"/></w:pPr></w:style>"#,
    },
    Def {
        id: "Date",
        name: "Date",
        based_on: None,
        xml: r#"<w:style w:type="paragraph" w:styleId="Date"><w:name w:val="Date"/><w:next w:val="BodyText"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:jc w:val="center"/></w:pPr></w:style>"#,
    },
    Def {
        id: "AbstractTitle",
        name: "Abstract Title",
        based_on: None,
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="AbstractTitle"><w:name w:val="Abstract Title"/><w:next w:val="Abstract"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="300" w:after="0"/><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Abstract",
        name: "Abstract",
        based_on: None,
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="Abstract"><w:name w:val="Abstract"/><w:next w:val="BodyText"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="100" w:after="300"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading1",
        name: "heading 1",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="480" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:b/><w:bCs/><w:color w:val="4F81BD"/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading2",
        name: "heading 2",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="200" w:after="0"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:b/><w:bCs/><w:color w:val="4F81BD"/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading3",
        name: "heading 3",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="200" w:after="0"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:b/><w:bCs/><w:color w:val="4F81BD"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading4",
        name: "heading 4",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="200" w:after="0"/><w:outlineLvl w:val="3"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:bCs/><w:i/><w:color w:val="4F81BD"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading5",
        name: "heading 5",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="200" w:after="0"/><w:outlineLvl w:val="4"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:iCs/><w:color w:val="4F81BD"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Heading6",
        name: "heading 6",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="200" w:after="0"/><w:outlineLvl w:val="5"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:cstheme="majorBidi"/><w:color w:val="4F81BD"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style>"#,
    },
    Def {
        id: "BlockText",
        name: "Block Text",
        based_on: Some("BodyText"),
        xml: r#"<w:style w:type="paragraph" w:styleId="BlockText"><w:name w:val="Block Text"/><w:basedOn w:val="BodyText"/><w:next w:val="BodyText"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:spacing w:before="100" w:after="100"/><w:ind w:firstLine="0" w:left="480" w:right="480"/></w:pPr><w:rPr><w:i/><w:color w:val="595959"/></w:rPr></w:style>"#,
    },
    Def {
        id: "SourceCode",
        name: "Source Code",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="SourceCode"><w:name w:val="Source Code"/><w:basedOn w:val="Normal"/><w:link w:val="VerbatimChar"/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F8F8F8"/><w:wordWrap w:val="off"/><w:spacing w:before="120" w:after="120" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "FootnoteText",
        name: "footnote text",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:spacing w:after="40"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Caption",
        name: "caption",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="0" w:after="120"/></w:pPr><w:rPr><w:i/><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "TableCaption",
        name: "Table Caption",
        based_on: Some("Caption"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="TableCaption"><w:name w:val="Table Caption"/><w:basedOn w:val="Caption"/><w:pPr><w:keepNext/></w:pPr></w:style>"#,
    },
    Def {
        id: "ImageCaption",
        name: "Image Caption",
        based_on: Some("Caption"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="ImageCaption"><w:name w:val="Image Caption"/><w:basedOn w:val="Caption"/><w:pPr><w:jc w:val="center"/></w:pPr></w:style>"#,
    },
    Def {
        id: "Figure",
        name: "Figure",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="Figure"><w:name w:val="Figure"/><w:basedOn w:val="Normal"/><w:pPr><w:jc w:val="center"/></w:pPr></w:style>"#,
    },
    Def {
        id: "CaptionedFigure",
        name: "Captioned Figure",
        based_on: Some("Figure"),
        xml: r#"<w:style w:type="paragraph" w:customStyle="1" w:styleId="CaptionedFigure"><w:name w:val="Captioned Figure"/><w:basedOn w:val="Figure"/><w:pPr><w:keepNext/></w:pPr></w:style>"#,
    },
    Def {
        id: "Bibliography",
        name: "Bibliography",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="Bibliography"><w:name w:val="Bibliography"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="120"/><w:ind w:left="480" w:hanging="480"/></w:pPr></w:style>"#,
    },
    Def {
        id: "TOCHeading",
        name: "TOC Heading",
        based_on: Some("Heading1"),
        xml: r#"<w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Heading1"/><w:next w:val="BodyText"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>"#,
    },
    Def {
        id: "TOC1",
        name: "toc 1",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="TOC1"><w:name w:val="toc 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:pPr><w:spacing w:after="100"/></w:pPr></w:style>"#,
    },
    Def {
        id: "TOC2",
        name: "toc 2",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="TOC2"><w:name w:val="toc 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:pPr><w:spacing w:after="100"/><w:ind w:left="220"/></w:pPr></w:style>"#,
    },
    Def {
        id: "TOC3",
        name: "toc 3",
        based_on: Some("Normal"),
        xml: r#"<w:style w:type="paragraph" w:styleId="TOC3"><w:name w:val="toc 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="39"/><w:unhideWhenUsed/><w:pPr><w:spacing w:after="100"/><w:ind w:left="440"/></w:pPr></w:style>"#,
    },
    Def {
        id: "DefaultParagraphFont",
        name: "Default Paragraph Font",
        based_on: None,
        xml: r#"<w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/><w:uiPriority w:val="1"/><w:semiHidden/><w:unhideWhenUsed/></w:style>"#,
    },
    Def {
        id: "VerbatimChar",
        name: "Verbatim Char",
        based_on: None,
        xml: r#"<w:style w:type="character" w:customStyle="1" w:styleId="VerbatimChar"><w:name w:val="Verbatim Char"/><w:link w:val="SourceCode"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="20"/></w:rPr></w:style>"#,
    },
    Def {
        id: "Hyperlink",
        name: "Hyperlink",
        based_on: None,
        xml: r#"<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="4F81BD"/><w:u w:val="single"/></w:rPr></w:style>"#,
    },
    Def {
        id: "FootnoteReference",
        name: "footnote reference",
        based_on: None,
        xml: r#"<w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>"#,
    },
    Def {
        id: "TableNormal",
        name: "Normal Table",
        based_on: None,
        xml: r#"<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#,
    },
    // md2star's table styles (its postprocess injects them the same way): thin grey
    // borders; the small one also sets 8 pt text for wide tables.
    Def {
        id: "MyTable",
        name: "MyTable",
        based_on: None,
        xml: r#"<w:style w:type="table" w:customStyle="1" w:styleId="MyTable"><w:name w:val="MyTable"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:left w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:right w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/></w:tblBorders><w:tblCellMar><w:top w:w="60" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="60" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#,
    },
    Def {
        id: "MyTableSmall",
        name: "MyTableSmall",
        based_on: None,
        xml: r#"<w:style w:type="table" w:customStyle="1" w:styleId="MyTableSmall"><w:name w:val="MyTableSmall"/><w:pPr><w:rPr><w:sz w:val="16"/><w:szCs w:val="16"/></w:rPr></w:pPr><w:rPr><w:sz w:val="16"/><w:szCs w:val="16"/></w:rPr><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:left w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:right w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="9E9E9E"/></w:tblBorders><w:tblCellMar><w:top w:w="30" w:type="dxa"/><w:left w:w="72" w:type="dxa"/><w:bottom w:w="30" w:type="dxa"/><w:right w:w="72" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>"#,
    },
];

fn def(id: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.id == id)
}

/// The style ids a `styles.xml` defines, and its names (lower case) → ids.
#[derive(Debug, Default)]
pub(super) struct StyleSheet {
    ids: BTreeSet<String>,
    by_name: HashMap<String, String>,
    /// Styles referenced by the output, by the writer's id.
    used: BTreeSet<&'static str>,
    /// Paragraph styles named by a `custom-style` attribute that the sheet lacks.
    custom: BTreeMap<String, String>,
}

impl StyleSheet {
    /// Read the style ids and names of a `styles.xml`.
    pub(super) fn read(xml: &str) -> Self {
        let mut sheet = StyleSheet::default();
        let mut rest = xml;
        while let Some(at) = rest.find("<w:style ") {
            rest = &rest[at..];
            let end = rest.find("</w:style>").unwrap_or(rest.len());
            let block = &rest[..end];
            if let Some(id) = attr_value(block, "w:styleId") {
                sheet.ids.insert(id.to_string());
                if let Some(name_at) = block.find("<w:name ") {
                    if let Some(name) = attr_value(&block[name_at..], "w:val") {
                        sheet
                            .by_name
                            .entry(name.to_ascii_lowercase())
                            .or_insert_with(|| id.to_string());
                    }
                }
            }
            rest = &rest[end.max(1)..];
        }
        sheet
    }

    /// The id to write for the writer's style `id`: the reference's own when it defines
    /// it (by id, else by name), ours otherwise — then added by [`StyleSheet::inject`].
    pub(super) fn id(&mut self, id: &'static str) -> String {
        if self.ids.contains(id) {
            return id.to_string();
        }
        if let Some(d) = def(id) {
            if let Some(found) = self.by_name.get(&d.name.to_ascii_lowercase()) {
                return found.clone();
            }
        }
        self.used.insert(id);
        id.to_string()
    }

    /// The id for a `custom-style="Name"` div: a style of ours, the sheet's own (by id or
    /// name), or a new paragraph style based on `Body Text`, as Pandoc creates one.
    pub(super) fn custom(&mut self, name: &str) -> String {
        if let Some(d) = DEFS
            .iter()
            .find(|d| d.id == name || d.name.eq_ignore_ascii_case(name))
        {
            return self.id(d.id);
        }
        if self.ids.contains(name) {
            return name.to_string();
        }
        if let Some(found) = self.by_name.get(&name.to_ascii_lowercase()) {
            return found.clone();
        }
        let id: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
        let id = if id.is_empty() {
            "Custom".to_string()
        } else {
            id
        };
        let base = self.id("BodyText");
        self.custom.entry(id.clone()).or_insert_with(|| {
            format!(
                "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"{}\"><w:name w:val=\"{}\"/><w:basedOn w:val=\"{}\"/><w:qFormat/></w:style>",
                super::xml::escape_attr(&id),
                super::xml::escape_attr(name),
                super::xml::escape_attr(&base)
            )
        });
        id
    }

    /// Add to `styles_xml` every used style it lacks, with the styles they are based on.
    pub(super) fn inject(&self, styles_xml: &str) -> String {
        let mut missing: BTreeMap<&'static str, &'static str> = BTreeMap::new();
        let mut queue: Vec<&'static str> = self.used.iter().copied().collect();
        while let Some(id) = queue.pop() {
            if self.ids.contains(id) || missing.contains_key(id) {
                continue;
            }
            let Some(d) = def(id) else { continue };
            if self.by_name.contains_key(&d.name.to_ascii_lowercase()) {
                continue;
            }
            missing.insert(id, d.xml);
            if let Some(base) = d.based_on {
                queue.push(base);
            }
            // A linked character/paragraph style pair travels together.
            if id == "SourceCode" {
                queue.push("VerbatimChar");
            }
        }
        if missing.is_empty() && self.custom.is_empty() {
            return styles_xml.to_string();
        }
        // In DEFS order, so the output is stable and readable.
        let mut added: String = DEFS
            .iter()
            .filter(|d| missing.contains_key(d.id))
            .map(|d| d.xml)
            .collect();
        for xml in self.custom.values() {
            added.push_str(xml);
        }
        match styles_xml.rfind("</w:styles>") {
            Some(at) => format!("{}{added}{}", &styles_xml[..at], &styles_xml[at..]),
            None => styles_xml.to_string(),
        }
    }
}

/// The value of `name="…"` in a tag.
pub(super) fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let at = tag.find(&pat)? + pat.len();
    let end = tag[at..].find('"')? + at;
    Some(&tag[at..end])
}

/// Set the document language in a reference `styles.xml`'s run defaults.
pub(super) fn set_language(styles_xml: &str, lang: &str) -> String {
    let lang_tag = format!("<w:lang w:val=\"{}\"/>", super::xml::escape_attr(lang));
    let Some(defaults_at) = styles_xml.find("<w:rPrDefault>") else {
        return styles_xml.to_string();
    };
    let defaults_end = styles_xml[defaults_at..]
        .find("</w:rPrDefault>")
        .map_or(styles_xml.len(), |e| e + defaults_at);
    let section = &styles_xml[defaults_at..defaults_end];
    if let Some(lang_at) = section.find("<w:lang ") {
        let start = defaults_at + lang_at;
        let end = styles_xml[start..]
            .find("/>")
            .map_or(start, |e| start + e + 2);
        return format!("{}{lang_tag}{}", &styles_xml[..start], &styles_xml[end..]);
    }
    if let Some(close) = section.find("</w:rPr>") {
        let at = defaults_at + close;
        return format!("{}{lang_tag}{}", &styles_xml[..at], &styles_xml[at..]);
    }
    if let Some(empty) = section.find("<w:rPr/>") {
        let at = defaults_at + empty;
        return format!(
            "{}<w:rPr>{lang_tag}</w:rPr>{}",
            &styles_xml[..at],
            &styles_xml[at + "<w:rPr/>".len()..]
        );
    }
    styles_xml.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_definition_is_named_and_based_on_a_defined_style() {
        for d in DEFS {
            assert!(
                d.xml.contains(&format!("w:styleId=\"{}\"", d.id)),
                "{}",
                d.id
            );
            assert!(
                d.xml.contains(&format!("<w:name w:val=\"{}\"/>", d.name)),
                "{}",
                d.id
            );
            if let Some(base) = d.based_on {
                assert!(def(base).is_some(), "{} is based on unknown {base}", d.id);
                assert!(
                    d.xml.contains(&format!("<w:basedOn w:val=\"{base}\"/>")),
                    "{}",
                    d.id
                );
            }
        }
    }

    #[test]
    fn a_style_is_found_by_name_in_a_localised_template() {
        let xml = r#"<w:styles><w:style w:type="paragraph" w:styleId="Titre1"><w:name w:val="heading 1"/></w:style></w:styles>"#;
        let mut sheet = StyleSheet::read(xml);
        assert_eq!(sheet.id("Heading1"), "Titre1");
        assert_eq!(sheet.inject(xml), xml, "nothing to add");
    }

    #[test]
    fn missing_styles_are_injected_with_their_bases() {
        let xml = r#"<w:styles><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;
        let mut sheet = StyleSheet::read(xml);
        assert_eq!(sheet.id("FirstParagraph"), "FirstParagraph");
        let out = sheet.inject(xml);
        assert!(out.contains("w:styleId=\"FirstParagraph\""));
        assert!(
            out.contains("w:styleId=\"BodyText\""),
            "its base comes along"
        );
        assert_eq!(
            out.matches("w:styleId=\"Normal\"").count(),
            1,
            "Normal not duplicated"
        );
        assert!(out.ends_with("</w:styles>"));
    }

    #[test]
    fn language_is_set_in_the_run_defaults() {
        let with = r#"<w:docDefaults><w:rPrDefault><w:rPr><w:lang w:val="en-US"/></w:rPr></w:rPrDefault></w:docDefaults>"#;
        assert!(set_language(with, "fr-FR").contains("<w:lang w:val=\"fr-FR\"/>"));
        assert!(!set_language(with, "fr-FR").contains("en-US"));
        let without = r#"<w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>"#;
        assert!(set_language(without, "fr-FR")
            .contains("<w:sz w:val=\"22\"/><w:lang w:val=\"fr-FR\"/></w:rPr>"));
    }
}
