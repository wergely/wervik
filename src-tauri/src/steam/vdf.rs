//! Minimal parser for Valve's text KeyValues ("VDF") format, sufficient for
//! libraryfolders.vdf, appmanifest_*.acf, and localconfig.vdf.

use std::fmt;

/// A VDF node is either a leaf string value or a nested object.
/// Steam VDF allows duplicate keys; we keep all children in insertion order
/// but expose lookup helpers that return the first match.
#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Obj(entries) => Some(entries.as_slice()),
            _ => None,
        }
    }

    /// Case-insensitive lookup of a direct child by key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_obj()?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get_str(key)?.parse().ok()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get_str(key)?.parse().ok()
    }

    /// Iterate direct children as (key, value) pairs.
    pub fn entries(&self) -> impl Iterator<Item = &(String, Value)> {
        self.as_obj().unwrap_or(&[]).iter()
    }

    /// Walk a path of nested object keys, case-insensitively.
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut current = self;
        for key in keys {
            current = current.get(key)?;
        }
        Some(current)
    }
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VDF parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse a full VDF document into a single root object Value.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    let mut chars = input.chars().peekable();
    let mut root: Vec<(String, Value)> = Vec::new();

    loop {
        skip_whitespace_and_comments(&mut chars);
        if chars.peek().is_none() {
            break;
        }
        let key = read_token(&mut chars)?;
        skip_whitespace_and_comments(&mut chars);
        match chars.peek() {
            Some('{') => {
                chars.next();
                let obj = parse_obj_body(&mut chars)?;
                root.push((key, obj));
            }
            Some('"') => {
                let val = read_token(&mut chars)?;
                root.push((key, Value::Str(val)));
            }
            _ => return Err(ParseError("expected value after key".into())),
        }
    }

    Ok(Value::Obj(root))
}

fn parse_obj_body(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, ParseError> {
    let mut entries: Vec<(String, Value)> = Vec::new();
    loop {
        skip_whitespace_and_comments(chars);
        match chars.peek() {
            Some('}') => {
                chars.next();
                return Ok(Value::Obj(entries));
            }
            None => return Err(ParseError("unexpected end of input in object".into())),
            _ => {}
        }
        let key = read_token(chars)?;
        skip_whitespace_and_comments(chars);
        match chars.peek() {
            Some('{') => {
                chars.next();
                let obj = parse_obj_body(chars)?;
                entries.push((key, obj));
            }
            Some('"') => {
                let val = read_token(chars)?;
                entries.push((key, Value::Str(val)));
            }
            _ => return Err(ParseError("expected value after key".into())),
        }
    }
}

fn skip_whitespace_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
    loop {
        while let Some(c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() == Some(&'/') {
            let mut lookahead = chars.clone();
            lookahead.next();
            if lookahead.peek() == Some(&'/') {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
        }
        break;
    }
}

fn read_token(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String, ParseError> {
    skip_whitespace_and_comments(chars);
    match chars.peek() {
        Some('"') => {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => return Ok(s),
                    Some('\\') => match chars.next() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some(other) => s.push(other),
                        None => return Err(ParseError("unterminated escape".into())),
                    },
                    Some(c) => s.push(c),
                    None => return Err(ParseError("unterminated string".into())),
                }
            }
        }
        Some(_) => {
            // Unquoted token (rare, but tolerate it).
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '{' || c == '}' {
                    break;
                }
                s.push(c);
                chars.next();
            }
            if s.is_empty() {
                Err(ParseError("expected token".into()))
            } else {
                Ok(s)
            }
        }
        None => Err(ParseError("unexpected end of input".into())),
    }
}
