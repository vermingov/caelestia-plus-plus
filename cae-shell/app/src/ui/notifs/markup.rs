//! A notification's body as runs of styled text.
//!
//! The body is written by whoever sent the notification, and that includes
//! any website the browser lets send them. Nothing in one is ever treated as
//! markup: it is read here into plain runs, and what draws them is given text
//! and four flags. The worst a hostile body can do is look strange.
//!
//! The specification allows a small subset of markup. `<b>`, `<i>`, `<u>`
//! and `<a>` are honoured, `<br>` is a line break, and every other tag is
//! dropped with its text kept.

/// A stretch of a body that is all styled the same way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Where it leads, when it is a link of a kind a notification has any
    /// business containing. Empty otherwise.
    pub href: String,
}

/// How deep in each kind of styling the text currently is. Counted rather
/// than flagged, so that `<b><b>x</b>` is still bold after the first close.
#[derive(Default)]
struct Style {
    bold: usize,
    italic: usize,
    underline: usize,
    links: Vec<String>,
}

/// One tag, as far as this cares about it.
struct Tag<'a> {
    closing: bool,
    name: String,
    attributes: &'a str,
    /// How many bytes of the body it took up.
    length: usize,
}

/// Reads a tag at the start of `text`, which begins with `<`. Anything that
/// is not shaped like one is not one, and stays text.
fn tag(text: &str) -> Option<Tag<'_>> {
    let inside = &text[1..];
    let closing = inside.starts_with('/');
    let named = &inside[usize::from(closing)..];

    let end = named.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(named.len());
    let name = &named[..end];
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) || named[end..].starts_with('_') {
        return None;
    }
    let close = named[end..].find('>')?;
    Some(Tag {
        closing,
        name: name.to_ascii_lowercase(),
        attributes: &named[end..end + close],
        length: 1 + usize::from(closing) + end + close + 1,
    })
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|_| code > 0);
    }
    Some(match name.to_ascii_lowercase().as_str() {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    })
}

/// `&amp;` and its relations as the characters they stand for. One that is
/// not known is left exactly as it was written.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let named = after.find(';').filter(|&end| {
            end > 0 && after[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
        });
        match named.and_then(|end| entity(&after[..end]).map(|decoded| (decoded, end))) {
            Some((decoded, end)) => {
                out.push(decoded);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The address an `<a>` points at, if it is one worth following: a web page
/// or a mail address, and nothing that runs.
fn link(attributes: &str) -> String {
    let lower = attributes.to_ascii_lowercase();
    let Some(at) = lower.find("href") else { return String::new() };
    let Some(value) = attributes[at + 4..].trim_start().strip_prefix('=') else { return String::new() };
    let value = value.trim_start();

    let address = match value.chars().next() {
        Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or_default(),
        _ => value.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or_default(),
    };
    let address = decode(address).trim().to_string();
    let lower = address.to_ascii_lowercase();
    let followable = ["https://", "http://", "mailto:"].iter().any(|scheme| lower.starts_with(scheme));
    if followable { address } else { String::new() }
}

pub fn runs(body: &str) -> Vec<Run> {
    let mut out = Vec::new();
    let mut style = Style::default();
    let mut push = |text: &str, style: &Style| {
        if !text.is_empty() {
            out.push(Run {
                text: decode(text),
                bold: style.bold > 0,
                italic: style.italic > 0,
                underline: style.underline > 0,
                href: style.links.last().cloned().unwrap_or_default(),
            });
        }
    };

    let mut rest = body;
    let mut plain_until = 0;
    while let Some(at) = rest[plain_until..].find('<').map(|at| at + plain_until) {
        let Some(tag) = tag(&rest[at..]) else {
            // An angle bracket that opens nothing is a character like any other.
            plain_until = at + 1;
            continue;
        };
        push(&rest[..at], &style);
        rest = &rest[at + tag.length..];
        plain_until = 0;

        let depth = |count: &mut usize| *count = if tag.closing { count.saturating_sub(1) } else { *count + 1 };
        match tag.name.as_str() {
            "br" => push("\n", &style),
            "a" if tag.closing => drop(style.links.pop()),
            "a" => style.links.push(link(tag.attributes)),
            "b" | "strong" => depth(&mut style.bold),
            "i" | "em" => depth(&mut style.italic),
            "u" => depth(&mut style.underline),
            _ => {}
        }
    }
    push(rest, &style);
    out
}

/// The same body as one line of plain text, for the folded preview.
pub fn plain(body: &str) -> String {
    let text: String = runs(body).into_iter().map(|run| run.text).collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styling_is_carried_and_tags_are_not() {
        let found = runs("Hello <b>bold <i>both</i></b> plain");
        let seen: Vec<(&str, bool, bool)> = found.iter().map(|r| (r.text.as_str(), r.bold, r.italic)).collect();
        assert_eq!(
            seen,
            [("Hello ", false, false), ("bold ", true, false), ("both", true, true), (" plain", false, false)]
        );
    }

    #[test]
    fn only_real_links_become_links() {
        let found = runs(
            r#"<a href="https://example.com">ok</a> <a href="javascript:alert(1)">bad</a> <a href='mailto:a@b.c'>mail</a>"#,
        );
        assert_eq!(found[0].href, "https://example.com");
        assert_eq!(found.iter().find(|r| r.text == "bad").map(|r| r.href.as_str()), Some(""));
        assert_eq!(found.iter().find(|r| r.text == "mail").map(|r| r.href.as_str()), Some("mailto:a@b.c"));
    }

    #[test]
    fn nothing_a_body_contains_survives_as_markup() {
        assert_eq!(plain(r#"<img src=x onerror="pwn()"><script>alert(1)</script>hi"#), "alert(1)hi");
        let found = runs(r#"<img src=x onerror="invoke('run')"><script>alert(1)</script><svg onload=x>hi"#);
        assert!(found.iter().all(|run| !run.text.contains('<')), "a tag got through as text: {found:?}");
    }

    #[test]
    fn entities_are_decoded_and_cannot_smuggle_a_tag_in() {
        assert_eq!(plain("a &amp; b &lt;i&gt; &#39;q&#x27; &unknown;"), "a & b <i> 'q' &unknown;");
        assert!(!runs("&lt;b&gt;not bold&lt;/b&gt;")[0].bold);
    }

    #[test]
    fn line_breaks_are_kept_for_the_body_and_flattened_for_the_preview() {
        let text: String = runs("one<br>two").into_iter().map(|run| run.text).collect();
        assert_eq!(text, "one\ntwo");
        assert_eq!(plain("one<br/>two\n\nthree"), "one two three");
    }

    #[test]
    fn a_stray_closing_tag_never_goes_negative() {
        assert!(!runs("</b></b>text<b>")[0].bold);
        assert!(runs("").is_empty());
    }

    #[test]
    fn an_angle_bracket_that_opens_nothing_is_text() {
        assert_eq!(plain("1 < 2 and a<_b> c"), "1 < 2 and a<_b> c");
        assert_eq!(plain("unfinished <b"), "unfinished <b");
    }
}
