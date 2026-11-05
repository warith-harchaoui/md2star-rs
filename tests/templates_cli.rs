//! `md2star-rs templates` end to end: the bundled templates are listed, exported byte
//! for byte, and their layouts shown as the slide planner reads them.

#![cfg(feature = "cli")]

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_md2star-rs"))
}

fn stdout(args: &[&str]) -> String {
    let out = bin().args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn list_names_both_bundled_templates() {
    let list = stdout(&["templates", "list"]);
    assert!(list.contains("docx\tdefault\t"), "{list}");
    assert!(list.contains("pptx\tdefault\t"), "{list}");
}

#[test]
fn export_writes_the_bundled_bytes() {
    let dir = tempfile::tempdir().unwrap();
    for (format, bytes) in [
        ("docx", md2star_rs::templates::DEFAULT_DOCX),
        ("pptx", md2star_rs::templates::DEFAULT_PPTX),
    ] {
        let path = dir.path().join(format!("t.{format}"));
        stdout(&["templates", "export", format, path.to_str().unwrap()]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{format}");
    }
}

#[test]
fn layouts_shows_roles_and_placeholders() {
    let layouts = stdout(&["templates", "layouts"]);
    assert!(layouts.contains("Title Slide\tcover"), "{layouts}");
    assert!(layouts.contains("ctrTitle idx=0"), "{layouts}");
}
