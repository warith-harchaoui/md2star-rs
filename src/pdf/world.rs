//! The Typst "world": everything the compiler may read, held in memory.
//!
//! One source file (the generated markup), the images it references under virtual
//! paths, and the fonts embedded in the binary (`typst-assets`: Libertinus Serif,
//! New Computer Modern and its math font, DejaVu Sans Mono). Nothing is read from disk
//! or the network and the clock is fixed, so a document compiles to the same bytes
//! everywhere — a phone has no system font directory to depend on anyway.

use std::collections::HashMap;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration, Smart};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

/// Fonts parsed once per process: parsing the embedded files takes milliseconds that
/// a second conversion should not pay again.
fn fonts() -> &'static (LazyHash<FontBook>, Vec<Font>) {
    static FONTS: OnceLock<(LazyHash<FontBook>, Vec<Font>)> = OnceLock::new();
    FONTS.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        (LazyHash::new(FontBook::from_fonts(&fonts)), fonts)
    })
}

fn library() -> &'static LazyHash<Library> {
    static LIBRARY: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIBRARY.get_or_init(|| LazyHash::new(Library::builder().build()))
}

fn file_id(path: &str) -> Option<FileId> {
    let vpath = VirtualPath::new(path).ok()?;
    Some(FileId::new(RootedPath::new(VirtualRoot::Project, vpath)))
}

struct MemoryWorld {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for MemoryWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &fonts().0
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().1.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // Fixed: `datetime.today()` must not make two runs differ.
        Datetime::from_ymd(2000, 1, 1)
    }
}

/// What a compilation gave: the PDF, or the error messages; the warnings either way.
pub struct Compiled {
    /// The PDF bytes, or the compiler's errors (one line each, with the source line).
    pub pdf: std::result::Result<Vec<u8>, Vec<String>>,
    /// The compiler's warnings.
    pub warnings: Vec<String>,
}

/// Compile Typst `source` with the virtual `files` (path → bytes) to PDF.
pub fn compile(source: &str, files: &[(String, Vec<u8>)]) -> Compiled {
    let Some(main_id) = file_id("/main.typ") else {
        return Compiled {
            pdf: Err(vec!["invalid main path".into()]),
            warnings: Vec::new(),
        };
    };
    let world = MemoryWorld {
        main: Source::new(main_id, source.to_string()),
        files: files
            .iter()
            .filter_map(|(path, bytes)| Some((file_id(path)?, Bytes::new(bytes.clone()))))
            .collect(),
    };
    let warned = typst::compile::<PagedDocument>(&world);
    let warnings = warned
        .warnings
        .iter()
        .map(|d| describe(d, &world))
        .collect();
    let pdf = match warned.output {
        Ok(document) => {
            let options = typst_pdf::PdfOptions {
                // A stable identifier and no timestamp: byte-identical output.
                ident: Smart::Custom("md2star-rs".into()),
                creator: Smart::Custom(Some("md2star-rs".into())),
                timestamp: None,
                ..typst_pdf::PdfOptions::default()
            };
            typst_pdf::pdf(&document, &options)
                .map_err(|errors| errors.iter().map(|d| describe(d, &world)).collect())
        }
        Err(errors) => Err(errors.iter().map(|d| describe(d, &world)).collect()),
    };
    Compiled { pdf, warnings }
}

/// A diagnostic as one line: the message and the generated line it points at.
fn describe(d: &SourceDiagnostic, world: &MemoryWorld) -> String {
    let main = &world.main;
    let line = world
        .range(d.span)
        .and_then(|r| main.lines().byte_to_line(r.start))
        .and_then(|l| main.text().lines().nth(l))
        .map(|l| {
            let l = l.trim();
            let short: String = l.chars().take(120).collect();
            format!(" (in `{short}`)")
        })
        .unwrap_or_default();
    format!("{}{line}", d.message)
}
