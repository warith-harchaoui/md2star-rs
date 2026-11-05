//! Shared helpers of the PPTX integration tests: open a produced deck and read its
//! parts, its slides in presentation order, each slide's layout name and notes.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// A deck read back from its bytes.
pub struct Deck {
    pub parts: BTreeMap<String, Vec<u8>>,
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

impl Deck {
    pub fn open(bytes: &[u8]) -> Self {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
        let mut parts = BTreeMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            parts.insert(f.name().to_string(), data);
        }
        Deck { parts }
    }

    pub fn part(&self, name: &str) -> Option<String> {
        self.parts
            .get(name)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// Relationship targets of a part, by id, resolved relative to its folder.
    pub fn rels(&self, part: &str) -> BTreeMap<String, (String, String)> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        let mut out = BTreeMap::new();
        let Some(xml) = self.part(&rels) else {
            return out;
        };
        for tag in xml.split("<Relationship ").skip(1) {
            let id = attr(&format!(" {tag}"), "Id").unwrap().to_string();
            let kind = attr(&format!(" {tag}"), "Type").unwrap().to_string();
            let target = attr(&format!(" {tag}"), "Target").unwrap().to_string();
            let resolved = if tag.contains("TargetMode=\"External\"") {
                target
            } else {
                let mut segs: Vec<&str> = if dir.is_empty() {
                    vec![]
                } else {
                    dir.split('/').collect()
                };
                for s in target.split('/') {
                    match s {
                        ".." => {
                            segs.pop();
                        }
                        "." | "" => {}
                        s => segs.push(s),
                    }
                }
                segs.join("/")
            };
            out.insert(id, (kind, resolved));
        }
        out
    }

    /// Slide parts in presentation order.
    pub fn slide_parts(&self) -> Vec<String> {
        let pres = self.part("ppt/presentation.xml").unwrap();
        let rels = self.rels("ppt/presentation.xml");
        let list = pres
            .split("sldIdLst>")
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default();
        list.split("<p:sldId ")
            .skip(1)
            .map(|tag| {
                let tag = format!(" {tag}");
                let rid = attr(&tag, "r:id").unwrap();
                rels[rid].1.clone()
            })
            .collect()
    }

    pub fn slide_count(&self) -> usize {
        self.slide_parts().len()
    }

    /// The XML of slide `n` (1-based).
    pub fn slide(&self, n: usize) -> String {
        self.part(&self.slide_parts()[n - 1]).unwrap()
    }

    /// The notes of slide `n`, if any.
    pub fn notes(&self, n: usize) -> Option<String> {
        let part = &self.slide_parts()[n - 1];
        let (_, target) = self
            .rels(part)
            .into_values()
            .find(|(k, _)| k.ends_with("/notesSlide"))?;
        self.part(&target)
    }

    /// The layout name of every slide.
    pub fn layout_names(&self) -> Vec<String> {
        self.slide_parts()
            .iter()
            .map(|p| {
                let (_, layout) = self
                    .rels(p)
                    .into_values()
                    .find(|(k, _)| k.ends_with("/slideLayout"))
                    .unwrap();
                let xml = self.part(&layout).unwrap();
                let tag = xml.split("<p:cSld").nth(1).unwrap();
                attr(tag, "name").unwrap().to_string()
            })
            .collect()
    }

    pub fn media(&self) -> Vec<String> {
        self.parts
            .keys()
            .filter(|k| k.starts_with("ppt/media/"))
            .cloned()
            .collect()
    }

    pub fn slide_size(&self) -> (i64, i64) {
        let pres = self.part("ppt/presentation.xml").unwrap();
        let tag = pres.split("<p:sldSz").nth(1).unwrap();
        (
            attr(tag, "cx").unwrap().parse().unwrap(),
            attr(tag, "cy").unwrap().parse().unwrap(),
        )
    }

    /// The `(x, y, cx, cy)` of every picture on slide `n`.
    pub fn picture_frames(&self, n: usize) -> Vec<(i64, i64, i64, i64)> {
        self.slide(n)
            .split("<p:pic>")
            .skip(1)
            .map(|pic| {
                let off = pic.split("<a:off").nth(1).unwrap();
                let ext = pic.split("<a:ext ").nth(1).unwrap();
                (
                    attr(off, "x").unwrap().parse().unwrap(),
                    attr(off, "y").unwrap().parse().unwrap(),
                    attr(&format!(" {ext}"), "cx").unwrap().parse().unwrap(),
                    attr(&format!(" {ext}"), "cy").unwrap().parse().unwrap(),
                )
            })
            .collect()
    }

    /// Every explicit run size on slide `n`, in points.
    pub fn sizes(&self, n: usize) -> Vec<f32> {
        self.slide(n)
            .split(" sz=\"")
            .skip(1)
            .map(|s| s[..s.find('"').unwrap()].parse::<f32>().unwrap() / 100.0)
            .collect()
    }

    /// Every internal relationship target exists; every part has a content type.
    pub fn assert_consistent(&self) {
        let ct = self.part("[Content_Types].xml").unwrap();
        for name in self.parts.keys() {
            if name.ends_with(".rels") {
                let source = name
                    .replace("_rels/", "")
                    .trim_end_matches(".rels")
                    .to_string();
                let source = source.trim_start_matches('/').to_string();
                for (id, (kind, target)) in self.rels(&source) {
                    if kind.ends_with("/hyperlink") {
                        continue;
                    }
                    assert!(
                        self.parts.contains_key(&target),
                        "{name}: {id} points at a missing part {target}"
                    );
                }
                continue;
            }
            if name == "[Content_Types].xml" {
                continue;
            }
            let ext = name.rsplit('.').next().unwrap();
            let typed = ct.contains(&format!("PartName=\"/{name}\""))
                || ct.contains(&format!("Extension=\"{ext}\""));
            assert!(typed, "{name} has no content type");
        }
        for part in self.slide_parts() {
            assert!(
                ct.contains(&format!("PartName=\"/{part}\"")),
                "{part} undeclared"
            );
        }
    }
}
