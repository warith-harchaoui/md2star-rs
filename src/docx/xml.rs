//! XML escaping for the hand-written WordprocessingML.

/// Escape text content (`&`, `<`, `>`), dropping the control characters XML 1.0 forbids.
pub(crate) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Escape an attribute value (text escaping plus `"`).
pub(crate) fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(escape_text("a<b & c>\u{1}"), "a&lt;b &amp; c&gt;");
        assert_eq!(escape_attr("say \"hi\""), "say &quot;hi&quot;");
    }
}
