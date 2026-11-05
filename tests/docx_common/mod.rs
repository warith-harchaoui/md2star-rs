//! Shared helpers of the DOCX integration tests: Word documents written by hand, part
//! by part, so a test does not depend on our own writer (or on any other one) to build
//! the reference document or the input it reads.

#![allow(dead_code)]

use std::io::{Cursor, Write};

const W: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";

/// Build a zip from named parts.
pub fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

/// A Word document with the given paragraph styles (`(id, name)`), page size in twips
/// and body XML.
pub fn word_document(styles: &[(&str, &str)], page: (u32, u32), body: &str) -> Vec<u8> {
    let styles: String = styles
        .iter()
        .map(|(id, name)| {
            format!("<w:style w:type=\"paragraph\" w:styleId=\"{id}\"><w:name w:val=\"{name}\"/><w:rPr><w:b/><w:sz w:val=\"48\"/></w:rPr></w:style>")
        })
        .collect();
    let (w, h) = page;
    let document = format!("<?xml version=\"1.0\"?><w:document {W}><w:body>{body}<w:sectPr><w:pgSz w:w=\"{w}\" w:h=\"{h}\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\"/></w:sectPr></w:body></w:document>");
    let styles = format!("<?xml version=\"1.0\"?><w:styles {W}>{styles}</w:styles>");
    let doc_rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";
    package(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
    ])
}

/// A reference document: A4, the given styles, an empty body.
pub fn reference(styles: &[(&str, &str)]) -> Vec<u8> {
    word_document(styles, (11906, 16838), "")
}

/// A Word document whose body is one inline picture (`png`, `cx`×`cy` EMU).
pub fn document_with_picture(png: &[u8], cx: u64, cy: u64) -> Vec<u8> {
    let body = format!(
        "<w:p><w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"1\" name=\"Picture 1\"/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"image1.png\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rId2\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    );
    let document = format!(
        "<?xml version=\"1.0\"?><w:document {W}><w:body>{body}<w:sectPr/></w:body></w:document>"
    );
    let doc_rels = "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/></Relationships>";
    package(&[
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/media/image1.png", png),
    ])
}
