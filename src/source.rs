//! Marked YAML parsing with the constrained authoring scalar vocabulary.

use crate::model::Origin;
use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::path::Path;
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::{Marker, Scanner, TScalarStyle, Token, TokenType};

#[derive(Debug)]
pub(crate) enum Value {
    Text(String),
    Integer(BigUint),
    Boolean(bool),
    Other,
    Map(BTreeMap<String, Node>),
    List(Vec<Node>),
}

#[derive(Debug)]
pub(crate) struct Node {
    pub value: Value,
    pub origin: Origin,
}

impl Node {
    pub fn error(&self, rule: &str, message: &str) -> anyhow::Error {
        anyhow::anyhow!("{} [{rule}] {message}", self.origin)
    }

    pub fn object(&self, allowed: &[&str]) -> Result<&BTreeMap<String, Node>> {
        let Value::Map(map) = &self.value else {
            return Err(self.error("schema", "expected an object"));
        };
        for (key, node) in map {
            if !allowed.contains(&key.as_str()) {
                return Err(node.error("schema", &format!("unknown property {key:?}")));
            }
        }
        Ok(map)
    }

    pub fn get(&self, key: &str) -> Result<&Node> {
        let Value::Map(map) = &self.value else {
            return Err(self.error("schema", "expected an object"));
        };
        map.get(key)
            .ok_or_else(|| self.error("schema", &format!("missing property {key:?}")))
    }

    pub fn optional(&self, key: &str) -> Option<&Node> {
        if let Value::Map(map) = &self.value {
            map.get(key)
        } else {
            None
        }
    }

    pub fn text(&self) -> Result<String> {
        if let Value::Text(text) = &self.value {
            Ok(text.clone())
        } else {
            Err(self.error("schema", "expected a string"))
        }
    }

    pub fn nonempty(&self) -> Result<String> {
        let text = self.text()?;
        if text.trim().is_empty() {
            return Err(self.error("schema", "expected nonblank string"));
        }
        Ok(text)
    }

    pub fn description(&self) -> Result<Option<String>> {
        self.optional("description").map(Node::text).transpose()
    }

    pub fn integer(&self, positive: bool) -> Result<BigUint> {
        if let Value::Integer(value) = &self.value {
            if !positive || value != &BigUint::from(0u8) {
                return Ok(value.clone());
            }
        }
        Err(self.error(
            "schema",
            if positive {
                "expected positive integer"
            } else {
                "expected nonnegative integer"
            },
        ))
    }

    pub fn width(&self, bytes: bool) -> Result<u64> {
        let width = self
            .integer(true)?
            .to_u64()
            .ok_or_else(|| self.error("schema", "width exceeds checked 64-bit capacity"))?;
        if bytes && width % 8 != 0 {
            return Err(self.error("schema", "width must be a multiple of eight"));
        }
        Ok(width)
    }

    pub fn boolean(&self) -> Result<bool> {
        if let Value::Boolean(value) = self.value {
            Ok(value)
        } else {
            Err(self.error("schema", "expected boolean"))
        }
    }

    pub fn list(&self) -> Result<&[Node]> {
        if let Value::List(items) = &self.value {
            if !items.is_empty() {
                return Ok(items);
            }
        }
        Err(self.error("schema", "expected nonempty array"))
    }

    pub fn items(&self, key: &str) -> Result<&[Node]> {
        match self.optional(key) {
            Some(node) => node.list(),
            None => Ok(&[]),
        }
    }

    pub fn enumeration<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_value(serde_json::Value::String(self.text()?))
            .map_err(|e| self.error("schema", &e.to_string()))
    }

    pub fn name(&self, upper: bool) -> Result<String> {
        let node = self.get("name")?;
        node.identifier(upper)
    }

    pub fn identifier(&self, upper: bool) -> Result<String> {
        let text = self.text()?;
        let valid = text.split('_').all(|part| !part.is_empty())
            && text.bytes().next().is_some_and(|b| {
                if upper {
                    b.is_ascii_uppercase()
                } else {
                    b.is_ascii_lowercase()
                }
            })
            && text.bytes().all(|b| {
                b == b'_'
                    || b.is_ascii_digit()
                    || if upper {
                        b.is_ascii_uppercase()
                    } else {
                        b.is_ascii_lowercase()
                    }
            });
        if !valid {
            return Err(self.error("schema", "identifier violates rds-v1 naming"));
        }
        Ok(text)
    }
}

fn scalar(text: String, style: TScalarStyle) -> Value {
    if style != TScalarStyle::Plain {
        return Value::Text(text);
    }
    match text.as_str() {
        "true" | "True" | "TRUE" => return Value::Boolean(true),
        "false" | "False" | "FALSE" => return Value::Boolean(false),
        "" | "~" | "null" | "Null" | "NULL" => return Value::Other,
        _ => {}
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(&text);
    let (digits, radix) = unsigned
        .strip_prefix("0x")
        .map_or((unsigned, 10), |s| (s, 16));
    if !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix)) {
        if let Some(value) = BigUint::parse_bytes(digits.as_bytes(), radix) {
            if text.starts_with('-') && value != BigUint::from(0u8) {
                return Value::Other;
            }
            return Value::Integer(value);
        }
    }
    static FLOAT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    if FLOAT
        .get_or_init(|| {
            regex::Regex::new(r"^[-+]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?$")
                .expect("constant scalar pattern")
        })
        .is_match(&text)
        || matches!(
            text.to_ascii_lowercase().as_str(),
            ".inf" | "+.inf" | "-.inf" | ".nan"
        )
    {
        Value::Other
    } else {
        Value::Text(text)
    }
}

struct Reader<'a> {
    parser: Parser<std::str::Chars<'a>>,
    file: &'a Path,
}

impl Reader<'_> {
    fn next(&mut self) -> Result<(Event, Marker)> {
        self.parser
            .next_token()
            .with_context(|| format!("{} [yaml-syntax]", self.file.display()))
    }

    fn node(&mut self, event: Event, mark: Marker, path: String, depth: usize) -> Result<Node> {
        let origin = Origin {
            file: self.file.into(),
            line: mark.line(),
            column: mark.col() + 1,
            path,
        };
        if depth > 128 {
            bail!("{origin} [yaml-depth] nesting exceeds 128");
        }
        let value = match event {
            Event::Scalar(text, style, _, _) => scalar(text, style),
            Event::SequenceStart(_, _) => {
                let mut items = Vec::new();
                loop {
                    let (event, mark) = self.next()?;
                    if event == Event::SequenceEnd {
                        break;
                    }
                    items.push(self.node(
                        event,
                        mark,
                        format!("{}[{}]", origin.path, items.len()),
                        depth + 1,
                    )?);
                }
                Value::List(items)
            }
            Event::MappingStart(_, _) => {
                let mut items = BTreeMap::new();
                loop {
                    let (event, mark) = self.next()?;
                    if event == Event::MappingEnd {
                        break;
                    }
                    let key_origin = Origin {
                        line: mark.line(),
                        column: mark.col() + 1,
                        ..origin.clone()
                    };
                    let key = self.node(event, mark, origin.path.clone(), depth + 1)?;
                    let Value::Text(key) = key.value else {
                        bail!("{key_origin} [yaml-key] mapping keys must be strings");
                    };
                    let path = format!("{}.{}", origin.path, key);
                    if key == "<<" {
                        bail!("{key_origin} [yaml-merge] YAML merge keys unsupported");
                    }
                    if items.contains_key(&key) {
                        bail!("{key_origin} {path} [yaml-duplicate] duplicate key {key:?}");
                    }
                    let (event, mark) = self.next()?;
                    items.insert(key, self.node(event, mark, path, depth + 1)?);
                }
                Value::Map(items)
            }
            _ => bail!("{origin} [yaml-syntax] expected scalar, sequence, or mapping"),
        };
        Ok(Node { value, origin })
    }
}

pub(crate) fn parse(file: &Path, text: &str) -> Result<Node> {
    let mut scanner = Scanner::new(text.chars());
    for Token(mark, token) in scanner.by_ref() {
        let rule = match token {
            TokenType::Anchor(_) | TokenType::Alias(_) | TokenType::Tag(_, _) => {
                Some("yaml-feature")
            }
            TokenType::VersionDirective(1, 2) => None,
            TokenType::VersionDirective(_, _) | TokenType::TagDirective(_, _) => {
                Some("yaml-version")
            }
            _ => None,
        };
        if let Some(rule) = rule {
            bail!(
                "{}:{}:{} $ [{rule}] unsupported YAML feature or directive",
                file.display(),
                mark.line(),
                mark.col() + 1
            );
        }
    }
    if let Some(error) = scanner.get_error() {
        bail!("{} [yaml-syntax] {error}", file.display());
    }
    let mut reader = Reader {
        parser: Parser::new(text.chars()),
        file,
    };
    if reader.next()?.0 != Event::StreamStart {
        bail!("{} [yaml-syntax] missing stream", file.display());
    }
    if reader.next()?.0 != Event::DocumentStart {
        bail!("{} [yaml-empty] expected one document", file.display());
    }
    let (event, mark) = reader.next()?;
    let node = reader.node(event, mark, "$".into(), 0)?;
    if reader.next()?.0 != Event::DocumentEnd || reader.next()?.0 != Event::StreamEnd {
        bail!(
            "{} [yaml-syntax] expected exactly one document",
            file.display()
        );
    }
    Ok(node)
}
