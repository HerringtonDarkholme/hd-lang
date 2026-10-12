//! The HTML pages: the same content as the Markdown pages under the same
//! anchors (`cli.doc.html`), in a minimal self-contained template. Doc
//! text is rendered by a small subset of Markdown (paragraphs, fenced
//! blocks, headings, list items, code spans, links); anything else shows
//! as its text.

use std::fmt::Write as _;

/// Escapes text for HTML content and attribute values.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Inline Markdown: code spans and `[text](target)` links; the rest is
/// escaped text.
pub(crate) fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(['`', '[']) {
        out.push_str(&escape(&rest[..i]));
        rest = &rest[i..];
        if rest.starts_with('`') {
            let ticks = rest.len() - rest.trim_start_matches('`').len();
            let fence = &rest[..ticks];
            if let Some(end) = rest[ticks..].find(fence) {
                let code = rest[ticks..ticks + end].trim();
                let _ = write!(out, "<code>{}</code>", escape(code));
                rest = &rest[ticks + end + ticks..];
                continue;
            }
            out.push_str(&escape(fence));
            rest = &rest[ticks..];
            continue;
        }
        if let Some((label, target, used)) = link(rest) {
            let _ = write!(out, "<a href=\"{}\">{}</a>", escape(target), inline(label));
            rest = &rest[used..];
            continue;
        }
        out.push('[');
        rest = &rest[1..];
    }
    out.push_str(&escape(rest));
    out
}

/// `[label](target)` at the start of `text`: label, target, bytes used.
fn link(text: &str) -> Option<(&str, &str, usize)> {
    let close = text.find("](")?;
    let label = &text[1..close];
    if label.contains('[') || label.contains(']') {
        return None;
    }
    let after = &text[close + 2..];
    let end = after.find(')')?;
    let target = &after[..end];
    if target.contains(char::is_whitespace) {
        return None;
    }
    Some((label, target, close + 2 + end + 1))
}

/// Block Markdown of a documentation text.
pub(crate) fn blocks(md: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<&str> = Vec::new();
    let mut list: Vec<&str> = Vec::new();
    let flush = |out: &mut String, para: &mut Vec<&str>, list: &mut Vec<&str>| {
        if !para.is_empty() {
            let _ = writeln!(out, "<p>{}</p>", inline(&para.join("\n")));
            para.clear();
        }
        if !list.is_empty() {
            out.push_str("<ul>\n");
            for item in list.iter() {
                let _ = writeln!(out, "<li>{}</li>", inline(item));
            }
            out.push_str("</ul>\n");
            list.clear();
        }
    };
    let mut lines = md.lines();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if let Some(info) = t.strip_prefix("```") {
            flush(&mut out, &mut para, &mut list);
            let mut code = String::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                code.push_str(l);
                code.push('\n');
            }
            let class = info
                .split_whitespace()
                .next()
                .map_or_else(String::new, |i| {
                    format!(" class=\"language-{}\"", escape(i))
                });
            let _ = writeln!(out, "<pre><code{class}>{}</code></pre>", escape(&code));
        } else if t.is_empty() {
            flush(&mut out, &mut para, &mut list);
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            if !para.is_empty() {
                flush(&mut out, &mut para, &mut list);
            }
            list.push(item);
        } else if let Some((level, text)) = heading(t) {
            flush(&mut out, &mut para, &mut list);
            let _ = writeln!(out, "<h{level}>{}</h{level}>", inline(text));
        } else {
            if !list.is_empty() {
                flush(&mut out, &mut para, &mut list);
            }
            para.push(t);
        }
    }
    flush(&mut out, &mut para, &mut list);
    out
}

/// An ATX heading: its level and text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.len() - line.trim_start_matches('#').len();
    let text = line[level..].strip_prefix(' ')?;
    (1..=6).contains(&level).then_some((level, text.trim()))
}

/// A whole page around `body`.
pub(crate) fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{}</title>\n<style>\n\
body {{ max-width: 52rem; margin: 2rem auto; padding: 0 1rem; font-family: system-ui, sans-serif; line-height: 1.5; }}\n\
pre {{ background: #f5f5f5; padding: 0.75rem; overflow-x: auto; }}\n\
code {{ font-family: ui-monospace, monospace; }}\n\
@media (prefers-color-scheme: dark) {{ body {{ background: #1b1b1b; color: #ddd; }} pre {{ background: #2a2a2a; }} a {{ color: #8ab4f8; }} }}\n\
</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::{blocks, inline};

    #[test]
    fn subset() {
        assert_eq!(
            inline("a `x < y` [b](c.md) [`d`]"),
            "a <code>x &lt; y</code> <a href=\"c.md\">b</a> [<code>d</code>]"
        );
        assert_eq!(
            blocks("One\ntwo.\n\n- a\n- b\n\n```hd\nf(1)\n```"),
            "<p>One\ntwo.</p>\n<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n<pre><code class=\"language-hd\">f(1)\n</code></pre>\n"
        );
    }
}
