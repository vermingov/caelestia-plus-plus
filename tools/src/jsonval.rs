//! A JSON tree that remembers the order its keys were written in.
//!
//! The config doctor rewrites the user's `shell.json` in place. Sorting its
//! keys would be a diff on every line of a file someone wrote by hand, so the
//! shared parser — which sorts, rightly, for the daemons — is not what this
//! wants.
//!
//! Numbers keep the text they were parsed from, so a value the doctor never
//! touched comes back out exactly as it went in.

use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num { value: f64, literal: String },
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn int(value: i64) -> Value {
        Value::Num { value: value as f64, literal: value.to_string() }
    }

    pub fn float(value: f64) -> Value {
        Value::Num { value, literal: python_float_repr(value) }
    }

    pub fn string(value: impl Into<String>) -> Value {
        Value::Str(value.into())
    }

    /// The word the shell's schema uses for this shape.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Num { .. } => "number",
            Value::Str(_) => "string",
            Value::Arr(_) => "array",
            Value::Obj(_) => "object",
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, Value)>> {
        match self {
            Value::Obj(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Vec<(String, Value)>> {
        match self {
            Value::Obj(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// Compact, the way a value is quoted inside a diagnostic line.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Two-space indented, the shape Python's `json.dump(indent=2)` writes.
    pub fn dump_indented(&self, indent: usize) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(indent), 0);
        out
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(true) => out.push_str("true"),
            Value::Bool(false) => out.push_str("false"),
            Value::Num { literal, .. } => out.push_str(literal),
            Value::Str(s) => write_string(out, s),
            Value::Arr(items) if items.is_empty() => out.push_str("[]"),
            Value::Obj(pairs) if pairs.is_empty() => out.push_str("{}"),
            Value::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        separator(out, indent);
                    }
                    newline(out, indent, depth + 1);
                    item.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push(']');
            }
            Value::Obj(pairs) => {
                out.push('{');
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        separator(out, indent);
                    }
                    newline(out, indent, depth + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push('}');
            }
        }
    }
}

/// Compact output separates items with `", "`, the way Python's `json.dumps`
/// does by default; indented output puts the space on the next line instead.
fn separator(out: &mut String, indent: Option<usize>) {
    out.push(',');
    if indent.is_none() {
        out.push(' ');
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    match indent {
        Some(width) => {
            out.push('\n');
            for _ in 0..width * depth {
                out.push(' ');
            }
        }
        None => {}
    }
}

/// Non-ASCII goes through as itself, like `ensure_ascii=False`.
fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python's `repr` of a float: shortest round-trip, always with a decimal
/// point or an exponent, and the exponent two digits with a sign.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let shortest = format!("{value:?}");
    let Some((mantissa, exponent)) = shortest.split_once(['e', 'E']) else {
        return shortest;
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let mantissa = mantissa.strip_suffix(".0").unwrap_or(mantissa);
    format!("{mantissa}e{}{:02}", if exponent < 0 { "-" } else { "+" }, exponent.abs())
}

// ---- parsing -------------------------------------------------------------

pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser { bytes: text.as_bytes(), text, i: 0 };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.i != parser.bytes.len() {
        return Err(format!("Extra data: {}", at(text, parser.i)));
    }
    Ok(value)
}

/// The same, after stripping the things people put in config files that JSON
/// does not allow: `//` and `/* */` comments, and a comma before a closing
/// brace or bracket.
pub fn parse_tolerant(text: &str) -> Result<Value, String> {
    parse(&strip_relaxations(text))
}

fn strip_relaxations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    let mut in_string = false;

    while i < bytes.len() {
        let ch = bytes[i];
        if in_string {
            out.push(ch as char);
            if ch == b'\\' && i + 1 < bytes.len() {
                // An escaped character cannot end the string, whatever it is.
                out.push_str(&text[i + 1..i + 1 + char_len(bytes[i + 1])]);
                i += 1 + char_len(bytes[i + 1]);
                continue;
            }
            if ch == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match ch {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b',' => {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j] as char).is_ascii_whitespace() {
                    j += 1;
                }
                if matches!(bytes.get(j), Some(b'}') | Some(b']')) {
                    i += 1; // a trailing comma: drop it
                } else {
                    out.push(',');
                    i += 1;
                }
            }
            _ => {
                let len = char_len(ch);
                out.push_str(&text[i..i + len]);
                i += len;
            }
        }
    }
    // A byte-order mark at the front is not JSON either.
    out.trim_start_matches('\u{feff}').to_string()
}

fn char_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    i: usize,
}

/// Python's `json` reports where it gave up as line, column and offset, and
/// the shell shows that message to the user — so this says it the same way.
fn at(text: &str, position: usize) -> String {
    let before = &text[..position.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let column = before.len() - before.rfind('\n').map_or(0, |at| at + 1) + 1;
    format!("line {line} column {column} (char {position})")
}

impl<'a> Parser<'a> {
    fn skip_whitespace(&mut self) {
        while self.i < self.bytes.len() && (self.bytes[self.i] as char).is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.bytes.get(self.i) {
            None => Err(format!("Expecting value: {}", at(self.text, self.i))),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.text[self.i..].starts_with(word) {
            self.i += word.len();
            return Ok(value);
        }
        Err(format!("Expecting value: {}", at(self.text, self.i)))
    }

    fn object(&mut self) -> Result<Value, String> {
        self.i += 1;
        let mut pairs = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Value::Obj(pairs));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.i) != Some(&b':') {
                return Err(format!("Expecting ':' delimiter: {}", at(self.text, self.i)));
            }
            self.i += 1;
            self.skip_whitespace();
            let value = self.value()?;
            // A repeated key keeps its first position and its last value, as
            // a dict comprehension would.
            match pairs.iter_mut().find(|(k, _): &&mut (String, Value)| *k == key) {
                Some(existing) => existing.1 = value,
                None => pairs.push((key, value)),
            }
            self.skip_whitespace();
            match self.bytes.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Obj(pairs));
                }
                _ => return Err(format!("Expecting ',' delimiter: {}", at(self.text, self.i))),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.i += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.bytes.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return Err(format!("Expecting ',' delimiter: {}", at(self.text, self.i))),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bytes.get(self.i) != Some(&b'"') {
            return Err(format!("Expecting property name enclosed in double quotes: {}", at(self.text, self.i)));
        }
        let start = self.i;
        self.i += 1;
        let mut out = String::new();
        while let Some(ch) = self.bytes.get(self.i) {
            match ch {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let escape = *self.bytes.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => return Err(format!("bad escape \\{}", other as char)),
                    }
                }
                _ => {
                    let len = char_len(*ch);
                    out.push_str(&self.text[self.i..self.i + len]);
                    self.i += len;
                }
            }
        }
        Err(format!("Unterminated string starting at: {}", at(self.text, start)))
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let code = self.hex4()?;
        // A high surrogate is only half a character; the low half follows.
        if (0xd800..0xdc00).contains(&code) && self.text[self.i..].starts_with("\\u") {
            self.i += 2;
            let low = self.hex4()?;
            if (0xdc00..0xe000).contains(&low) {
                let combined = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                return char::from_u32(combined).ok_or_else(|| "bad surrogate pair".to_string());
            }
            return Err("bad surrogate pair".to_string());
        }
        char::from_u32(code).ok_or_else(|| "bad \\u escape".to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.i + 4;
        let digits = self.text.get(self.i..end).ok_or("short \\u escape")?;
        self.i = end;
        u32::from_str_radix(digits, 16).map_err(|_| "bad \\u escape".to_string())
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        if self.bytes.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        let mut is_int = true;
        while let Some(ch) = self.bytes.get(self.i) {
            match ch {
                b'0'..=b'9' => self.i += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_int = false;
                    self.i += 1;
                }
                _ => break,
            }
        }
        let literal = &self.text[start..self.i];
        let value: f64 = literal.parse().map_err(|_| format!("Expecting value: {}", at(self.text, start)))?;
        // An exponent is written back the way Python would print it, not the
        // way it was typed; everything else keeps its own text.
        let literal = if is_int || !literal.contains(['e', 'E']) {
            literal.to_string()
        } else {
            python_float_repr(value)
        };
        Ok(Value::Num { value, literal })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_come_back_in_the_order_they_were_written() {
        let value = parse(r#"{"zebra": 1, "apple": 2, "mid": 3}"#).unwrap();
        let keys: Vec<&str> = value.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["zebra", "apple", "mid"]);
    }

    #[test]
    fn a_number_keeps_the_text_it_was_written_with() {
        let value = parse(r#"{"a": 12, "b": 12.0, "c": 0.50, "d": -3}"#).unwrap();
        assert_eq!(value.dump(), r#"{"a": 12, "b": 12.0, "c": 0.50, "d": -3}"#);
    }

    #[test]
    fn indented_output_is_the_shape_python_writes() {
        let value = parse(r#"{"a":1,"b":[1,2],"c":{},"d":[],"e":{"f":"g"}}"#).unwrap();
        assert_eq!(
            value.dump_indented(2),
            "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2\n  ],\n  \"c\": {},\n  \"d\": [],\n  \"e\": {\n    \"f\": \"g\"\n  }\n}"
        );
    }

    #[test]
    fn comments_and_trailing_commas_are_forgiven_only_by_the_tolerant_parser() {
        let text = "{\n // a comment\n \"a\": 1, /* inline */\n \"b\": [1, 2,],\n}";
        assert!(parse(text).is_err());
        let value = parse_tolerant(text).unwrap();
        assert_eq!(value.dump(), r#"{"a": 1, "b": [1, 2]}"#);
    }

    #[test]
    fn a_comma_inside_a_string_is_not_a_trailing_comma() {
        let value = parse_tolerant(r#"{"a": "one, two,", "b": "// not a comment"}"#).unwrap();
        assert_eq!(value.get("a").unwrap().as_str(), Some("one, two,"));
        assert_eq!(value.get("b").unwrap().as_str(), Some("// not a comment"));
    }

    #[test]
    fn escapes_and_non_ascii_survive_a_round_trip() {
        let value = parse(r#"{"a": "quote\" slash\\ tab\t", "b": "café", "c": "😀"}"#).unwrap();
        assert_eq!(value.get("b").unwrap().as_str(), Some("café"));
        assert_eq!(value.get("c").unwrap().as_str(), Some("😀"));
        assert_eq!(value.dump(), r#"{"a": "quote\" slash\\ tab\t", "b": "café", "c": "😀"}"#);
    }

    #[test]
    fn floats_print_the_way_python_prints_them() {
        assert_eq!(python_float_repr(1.0), "1.0");
        assert_eq!(python_float_repr(1.5), "1.5");
        assert_eq!(python_float_repr(1e20), "1e+20");
        assert_eq!(python_float_repr(1e-7), "1e-07");
    }
}
