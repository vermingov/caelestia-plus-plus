//! Just enough JSON for redwall's UI protocol and rule store.
//!
//! The protocol is six message shapes of flat string/number/bool fields, and
//! the rule file is a map of maps. Pulling a parser crate into a root daemon
//! on the packet path to handle that is a poor trade, so this is the subset,
//! written out.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.is_finite() => Some(*n as u64),
            _ => None,
        }
    }

    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Json::as_str)
    }

    pub fn bool_field(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(Json::as_bool).unwrap_or(default)
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write_to(&mut out);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 9e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{}", n);
                }
            }
            Json::Str(s) => write_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_to(out);
                }
                out.push(']');
            }
            Json::Obj(map) => {
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write_to(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Builds an object from `(key, value)` pairs, for composing protocol messages.
pub fn obj<const N: usize>(fields: [(&str, Json); N]) -> Json {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert(k.to_string(), v);
    }
    Json::Obj(m)
}

pub fn s(v: impl Into<String>) -> Json {
    Json::Str(v.into())
}

pub fn n(v: impl Into<f64>) -> Json {
    Json::Num(v.into())
}

pub fn parse(input: &str) -> Option<Json> {
    let bytes = input.as_bytes();
    let mut p = Parser { b: bytes, i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i == bytes.len() {
        Some(v)
    } else {
        None
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        if self.i < self.b.len() && self.b[self.i] == c {
            self.i += 1;
            Some(())
        } else {
            None
        }
    }

    fn literal(&mut self, word: &str) -> Option<()> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Some(())
        } else {
            None
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.skip_ws();
        match *self.b.get(self.i)? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' => self.literal("true").map(|_| Json::Bool(true)),
            b'f' => self.literal("false").map(|_| Json::Bool(false)),
            b'n' => self.literal("null").map(|_| Json::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.eat(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.eat(b'}').is_some() {
            return Some(Json::Obj(map));
        }
        loop {
            self.skip_ws();
            let k = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            let v = self.value()?;
            map.insert(k, v);
            self.skip_ws();
            if self.eat(b',').is_some() {
                continue;
            }
            self.eat(b'}')?;
            return Some(Json::Obj(map));
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b']').is_some() {
            return Some(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            if self.eat(b',').is_some() {
                continue;
            }
            self.eat(b']')?;
            return Some(Json::Arr(items));
        }
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4)?;
                            self.i += 4;
                            let code = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            // Lone surrogates are not worth pairing up here; the
                            // protocol carries paths and process names
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return None,
                    }
                }
                _ => {
                    // Re-decode UTF-8 from the raw bytes rather than assuming ASCII
                    let start = self.i - 1;
                    let len = utf8_len(c);
                    self.i = start + len;
                    out.push_str(std::str::from_utf8(self.b.get(start..self.i)?).ok()?);
                }
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.i;
        while self.i < self.b.len()
            && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
            .map(Json::Num)
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_protocol_messages() {
        let msg = obj([
            ("t", s("ask")),
            ("id", n(7u32)),
            ("exe", s("/usr/bin/curl")),
            ("port", n(443u32)),
        ]);
        let parsed = parse(&msg.dump()).unwrap();
        assert_eq!(parsed.str_field("t"), Some("ask"));
        assert_eq!(parsed.get("id").unwrap().as_u64(), Some(7));
    }

    #[test]
    fn escapes_and_unicode_survive() {
        let v = s("a\"b\\c\nd\tKøbenhavn");
        let back = parse(&v.dump()).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(parse("{\"a\":1} junk").is_none());
        assert!(parse("{\"a\":}").is_none());
    }

    #[test]
    fn nested_rule_store_parses() {
        let src = r#"{"/usr/bin/x":{"action":"allow","name":"x","added":12}}"#;
        let v = parse(src).unwrap();
        assert_eq!(
            v.get("/usr/bin/x").unwrap().str_field("action"),
            Some("allow")
        );
    }
}
