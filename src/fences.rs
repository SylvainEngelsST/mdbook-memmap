//! Parse actual Markdown inclusions without touching literal code examples.

use anyhow::{bail, Context, Result};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use serde::Deserialize;
use std::ops::Range;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Summary,
    Registers,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fence {
    pub source: String,
    pub view: View,
    pub map: Option<String>,
    pub address_space: Option<String>,
}

#[derive(Debug)]
pub struct Inclusion {
    pub range: Range<usize>,
    pub line: usize,
    pub fence: Fence,
}

pub fn parse(content: &str) -> Result<Vec<Inclusion>> {
    let mut inclusions = Vec::new();
    let mut current: Option<(usize, String)> = None;
    let mut nested = 0;
    for (event, range) in Parser::new(content).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_) | Tag::Item | Tag::BlockQuote(_)) => nested += 1,
            Event::End(TagEnd::List(_) | TagEnd::Item | TagEnd::BlockQuote(_)) => nested -= 1,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if info.split_whitespace().next() == Some("memmap") =>
            {
                let line = content[..range.start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1;
                if info.as_ref() != "memmap" {
                    bail!("line {line}: fence info must be exactly 'memmap'");
                }
                if nested != 0 {
                    bail!(
                        "line {line}: memmap fences must be top-level, not in lists or blockquotes"
                    );
                }
                current = Some((range.start, String::new()));
            }
            Event::Text(text) => {
                if let Some((_, body)) = &mut current {
                    body.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((start, body)) = current.take() {
                    let line = content[..start].bytes().filter(|b| *b == b'\n').count() + 1;
                    let fence = parse_properties(&body)
                        .with_context(|| format!("line {line}: invalid memmap properties"))?;
                    inclusions.push(Inclusion {
                        range: start..range.end,
                        line,
                        fence,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(inclusions)
}

fn parse_properties(body: &str) -> Result<Fence> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(body)?;
    let properties = value
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("expected an object"))?;
    if properties
        .iter()
        .any(|(key, value)| !key.is_string() || !value.is_string())
    {
        bail!("all property names and values must be strings");
    }
    let fence: Fence = serde_yaml_ng::from_value(value)?;
    if fence.source.trim().is_empty()
        || fence.map.is_some() == fence.address_space.is_some()
        || fence
            .map
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        || fence
            .address_space
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
    {
        bail!("require nonempty source and exactly one nonempty map/address_space");
    }
    Ok(fence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_contract_and_byte_offsets() {
        let text = "é\n~~~memmap\nsource: ../timer.yaml\nview: registers\nmap: csr\n~~~\n\nTail\n";
        let blocks = parse(text).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].line, 2);
        assert_eq!(blocks[0].fence.source, "../timer.yaml");
        assert_eq!(blocks[0].fence.view, View::Registers);
        assert!(text[blocks[0].range.clone()].starts_with("~~~memmap"));
        assert_eq!(&text[blocks[0].range.end..], "\n\nTail\n");
    }

    #[test]
    fn literal_examples_are_not_requests() {
        for text in [
            "````markdown\n```memmap\ninvalid\n```\n````",
            "    ```memmap\n    invalid\n    ```",
            "```mermaid\nflowchart LR\n```",
        ] {
            assert!(parse(text).unwrap().is_empty());
        }
    }

    #[test]
    fn invalid_properties_fail() {
        for body in [
            "source: x\nmap: csr\nview: registers\nmap: other",
            "source: x\nmap: csr\nview: registers\nextra: value",
            "source: 4\nmap: csr\nview: registers",
            "source: x\nmap: csr\naddress_space: cpu\nview: summary",
            "source: x\nview: summary",
            "source: x\nmap: csr\nview: invalid",
            "source: ''\nmap: csr\nview: summary",
            "source: x\nmap: ''\nview: summary",
        ] {
            assert!(
                parse(&format!("```memmap\n{body}\n```\n")).is_err(),
                "{body}"
            );
        }
    }

    #[test]
    fn nested_or_annotated_fences_fail_explicitly() {
        for text in [
            "> ```memmap\n> source: x\n> map: csr\n> view: summary\n> ```",
            "- ```memmap\n  source: x\n  map: csr\n  view: summary\n  ```",
            "```memmap extra\nsource: x\nmap: csr\nview: summary\n```",
        ] {
            assert!(parse(text).is_err());
        }
    }
}
