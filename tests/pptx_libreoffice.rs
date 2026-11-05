//! The decks open in a real presentation program: LibreOffice, headless, converts each
//! one to PDF with one page per slide. A local check only — LibreOffice is a test tool
//! here, never a dependency of the crate — so the test passes silently when `soffice`
//! is not installed (CI) or `MD2STAR_SKIP_SOFFICE` is set.

use std::path::Path;
use std::process::Command;

use md2star_rs::{convert, Format, Options};

fn soffice() -> Option<String> {
    if std::env::var_os("MD2STAR_SKIP_SOFFICE").is_some() {
        return None;
    }
    ["soffice", "libreoffice"]
        .into_iter()
        .find(|bin| {
            Command::new(bin)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(str::to_string)
}

fn pdf_pages(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf).replace("/Type /", "/Type/");
    text.matches("/Type/Page").count() - text.matches("/Type/Pages").count()
}

#[test]
fn every_deck_opens_in_libreoffice_with_one_page_per_slide() {
    let Some(bin) = soffice() else {
        eprintln!("soffice not found: LibreOffice check skipped");
        return;
    };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let mut expected = Vec::new();
    for (name, min) in [
        ("deck", None),
        ("deck", Some(30.0)),
        ("syntax", None),
        ("comprehensive_presentation", Some(30.0)),
        ("images", None),
        ("math", None),
        ("table_zoo", Some(30.0)),
    ] {
        let md = std::fs::read_to_string(fixtures.join(format!("{name}.md"))).unwrap();
        let mut o = Options::default();
        o.base_dir = Some(fixtures.clone());
        o.min_pt = min;
        let out = convert(&md, Format::Pptx, &o).unwrap();
        let slides = md2star_rs::audit::audit(&out.bytes, &Default::default())
            .unwrap()
            .slides;
        let file = format!("{name}-{}.pptx", min.map_or(0, |m| m as u32));
        std::fs::write(dir.path().join(&file), &out.bytes).unwrap();
        expected.push((file, slides));
    }
    let profile = format!("file://{}", dir.path().join("profile").display());
    let mut cmd = Command::new(bin);
    cmd.arg(format!("-env:UserInstallation={profile}"))
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(dir.path());
    for (file, _) in &expected {
        cmd.arg(dir.path().join(file));
    }
    let status = cmd.output().unwrap();
    assert!(status.status.success(), "{status:?}");
    for (file, slides) in expected {
        let pdf = dir.path().join(file.replace(".pptx", ".pdf"));
        let bytes = std::fs::read(&pdf).unwrap_or_else(|_| panic!("{file} did not convert"));
        assert_eq!(pdf_pages(&bytes), slides, "{file}");
    }
}
