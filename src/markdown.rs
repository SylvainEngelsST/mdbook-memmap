//! Deterministic Markdown compatible with the standalone reference renderer.

use crate::fences::View;
use crate::model::*;
use anyhow::{bail, Result};
use num_bigint::BigUint;

pub enum Source<'a> {
    Ip(&'a Definition),
    System(&'a ResolvedView),
}

struct Block<'a> {
    path: Vec<String>,
    definition: &'a Definition,
    map: &'a MemoryMap,
    base: BigUint,
    allocated: &'a BigUint,
    instances: &'a [Instance],
}

impl Block<'_> {
    fn resets(&self) -> Result<&[ResetType]> {
        match &self.definition.kind {
            Kind::Ip { reset_types, .. } => Ok(reset_types),
            _ => bail!(
                "{} [export-selector] expected IP definition",
                self.definition.origin
            ),
        }
    }
}

fn cell(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('`', "&#96;")
        .replace('\n', "<br>")
}

fn anchor(parts: &[String], prefix: &str) -> String {
    let mut parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    if !prefix.is_empty() {
        parts.insert(0, prefix);
    }
    format!(
        "mm-{}",
        parts
            .iter()
            .map(|part| { format!("{}-{}", part.chars().count(), part.to_lowercase()) })
            .collect::<Vec<_>>()
            .join("-")
    )
}

fn address(value: &BigUint) -> String {
    format!("0x{value:08X}")
}
fn hex(value: &BigUint) -> String {
    format!("0x{value:x}")
}

fn table(lines: &mut Vec<String>, headers: &[&str], rows: Vec<Vec<String>>) {
    lines.push(format!("| {} |", headers.join(" | ")));
    lines.push(format!("| {} |", vec!["---"; headers.len()].join(" | ")));
    for row in rows {
        lines.push(format!("| {} |", row.join(" | ")));
    }
    lines.push(String::new());
}

fn heading(lines: &mut Vec<String>, level: usize, text: &str, path: &[String], prefix: &str) {
    lines.extend([
        format!("<a id=\"{}\"></a>", anchor(path, prefix)),
        String::new(),
        format!("{} {text}", "#".repeat(level)),
        String::new(),
    ]);
}

fn prose(lines: &mut Vec<String>, text: &Option<String>) {
    if let Some(text) = text.as_ref().filter(|text| !text.is_empty()) {
        lines.extend([text.clone(), String::new()]);
    }
}

fn bits(lsb: u64, width: u64) -> String {
    if width == 1 {
        lsb.to_string()
    } else {
        format!("{}:{lsb}", lsb + width - 1)
    }
}

fn reset(field: &Field, name: &str) -> Result<String> {
    let Some(reset) = field.resets.iter().find(|reset| reset.reset_type == name) else {
        return Ok("unspecified".into());
    };
    // A malformed or impractically wide input must not allocate an unbounded bit string.
    if field.width_bits > 65_536 {
        bail!(
            "{} [markdown-width] reset display exceeds 65536 bits",
            field.origin
        );
    }
    let bits: String = (0..field.width_bits)
        .rev()
        .map(|bit| {
            if reset.mask.bit(bit) {
                if reset.value.bit(bit) {
                    '1'
                } else {
                    '0'
                }
            } else {
                '?'
            }
        })
        .collect();
    Ok(format!(
        "`0b{bits}` (value `{}`, mask `{}`)",
        hex(&reset.value),
        hex(&reset.mask)
    ))
}

fn access(value: &Access) -> &'static str {
    match value {
        Access::ReadOnly => "read-only",
        Access::WriteOnly => "write-only",
        Access::ReadWrite => "read-write",
        Access::WriteOnce => "writeOnce",
        Access::ReadWriteOnce => "read-writeOnce",
        Access::NoAccess => "no-access",
    }
}

fn hardware_access(value: &HardwareAccess) -> &'static str {
    match value {
        HardwareAccess::ReadOnly => "read-only",
        HardwareAccess::WriteOnly => "write-only",
        HardwareAccess::ReadWrite => "read-write",
        HardwareAccess::NoAccess => "no-access",
    }
}

fn write_effect(value: &WriteEffect) -> &'static str {
    match value {
        WriteEffect::OneToClear => "oneToClear",
        WriteEffect::OneToSet => "oneToSet",
        WriteEffect::OneToToggle => "oneToToggle",
        WriteEffect::ZeroToClear => "zeroToClear",
        WriteEffect::ZeroToSet => "zeroToSet",
        WriteEffect::ZeroToToggle => "zeroToToggle",
        WriteEffect::Clear => "clear",
        WriteEffect::Set => "set",
    }
}

fn usage(value: &Usage) -> &'static str {
    match value {
        Usage::Register => "register",
        Usage::Memory => "memory",
        Usage::Reserved => "reserved",
    }
}

fn field_rows(block: &Block<'_>, register: &Register, prefix: &str) -> Result<Vec<Vec<String>>> {
    let resets = block.resets()?;
    let reserved = |lsb, width| {
        let mut row = vec![format!("`{}`", bits(lsb, width)), "Reserved".into()];
        row.extend(vec!["unspecified".into(); 4 + resets.len()]);
        row
    };
    let mut fields: Vec<_> = register.fields.iter().collect();
    fields.sort_by_key(|field| std::cmp::Reverse(field.lsb));
    let mut end = register.width_bits;
    let mut rows = Vec::new();
    for field in fields {
        let field_end = field.lsb + field.width_bits;
        if field_end < end {
            rows.push(reserved(field_end, end - field_end));
        }
        let mut path = block.path.clone();
        path.extend([register.name.clone(), field.name.clone()]);
        let mut effects = Vec::new();
        if let Some(effect) = &field.modified_write_value {
            effects.push(format!("write: {}", write_effect(effect)));
        }
        if let Some(effect) = &field.read_action {
            effects.push(format!(
                "read: {}",
                match effect {
                    ReadEffect::Clear => "clear",
                    ReadEffect::Set => "set",
                }
            ));
        }
        let mut row = vec![
            format!("`{}`", bits(field.lsb, field.width_bits)),
            format!("[`{}`](#{})", field.name, anchor(&path, prefix)),
            access(&field.access).into(),
            hardware_access(&field.hw_access).into(),
            field.volatile.to_string(),
            cell(&if effects.is_empty() {
                "none".into()
            } else {
                effects.join("; ")
            }),
        ];
        for reset_type in resets {
            row.push(reset(field, &reset_type.name)?);
        }
        rows.push(row);
        end = field.lsb;
    }
    if end != 0 {
        rows.push(reserved(0, end));
    }
    Ok(rows)
}

fn details(
    lines: &mut Vec<String>,
    block: &Block<'_>,
    register: &Register,
    prefix: &str,
) -> Result<()> {
    let mut path = block.path.clone();
    path.push(register.name.clone());
    heading(lines, 3, &register.name, &path, prefix);
    lines.extend([
        format!(
            "Offset: `{}`; address: `{}`; width: {} bits.",
            hex(&register.offset),
            address(&(&block.base + &register.offset)),
            register.width_bits
        ),
        String::new(),
    ]);
    prose(lines, &register.description);
    let mut headers: Vec<String> = [
        "Bits",
        "Field",
        "SW access",
        "HW access",
        "Volatile",
        "Side effects",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    headers.extend(
        block
            .resets()?
            .iter()
            .map(|reset| format!("Reset {}", reset.name)),
    );
    table(
        lines,
        &headers.iter().map(String::as_str).collect::<Vec<_>>(),
        field_rows(block, register, prefix)?,
    );
    for field in &register.fields {
        let mut path = path.clone();
        path.push(field.name.clone());
        heading(lines, 4, &field.name, &path, prefix);
        prose(lines, &field.description);
    }
    Ok(())
}

/// Render a locally validated IP selection or resolved system view.
///
/// Inputs must retain the invariants established by [`crate::loader`] and
/// [`crate::project`]; this is not a substitute for source validation.
///
/// The legacy generated comment and anchor encoding are retained deliberately
/// so native output can be checked byte-for-byte against the reference exporter.
pub fn render(
    source: Source<'_>,
    map_name: Option<&str>,
    view: View,
    prefix: &str,
) -> Result<String> {
    if !prefix.is_empty()
        && (!prefix
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    {
        bail!("[anchor-prefix] expected lowercase ASCII letters, digits and hyphens");
    }
    let mut lines = vec![
        "<!-- Generated by rds-memmap. Do not edit. -->".into(),
        String::new(),
    ];
    let blocks = match source {
        Source::Ip(definition) => {
            let Kind::Ip { maps, .. } = &definition.kind else {
                bail!(
                    "{} [export-selector] IP rendering requires IP input",
                    definition.origin
                );
            };
            lines.extend(["## IP Memory Maps".into(), String::new()]);
            prose(&mut lines, &definition.description);
            let maps: Vec<_> = maps
                .iter()
                .filter(|map| map_name.is_none_or(|name| name == map.name))
                .collect();
            if maps.is_empty() {
                bail!("{} [map-name] unknown map {map_name:?}", definition.origin);
            }
            maps.into_iter()
                .map(|map| Block {
                    path: vec![map.name.clone()],
                    definition,
                    map,
                    base: BigUint::from(0u8),
                    allocated: &map.size_bytes,
                    instances: &[],
                })
                .collect::<Vec<_>>()
        }
        Source::System(system) => {
            if map_name.is_some() {
                bail!(
                    "{} [export-selector] map selector cannot be used with system view",
                    system.definition.origin
                );
            }
            lines.extend([
                format!("## System Memory Map: {}", system.address_space.name),
                String::new(),
                "Flattened reference view; not a bus-connected system design.".into(),
                String::new(),
            ]);
            prose(&mut lines, &system.definition.description);
            prose(&mut lines, &system.address_space.description);
            system
                .blocks
                .iter()
                .map(|block| {
                    let mut path = block.instance_path();
                    path.push(block.memory_map.name.clone());
                    Block {
                        path,
                        definition: &block.definition,
                        map: &block.memory_map,
                        base: block.base.clone(),
                        allocated: &block.mapping.size_bytes,
                        instances: &block.instances,
                    }
                })
                .collect()
        }
    };
    for block in &blocks {
        if block.map.size_bytes == BigUint::from(0u8) {
            bail!(
                "{} [markdown-view] map size must be positive",
                block.map.origin
            );
        }
        if view == View::Registers && block.map.usage != Usage::Register {
            bail!(
                "{} [markdown-view] register detail view requires register-usage maps only",
                block.map.origin
            );
        }
    }
    table(
        &mut lines,
        &[
            "Map",
            "Base",
            "End (inclusive)",
            "Implemented bytes",
            "Allocated bytes",
            "Usage",
        ],
        blocks
            .iter()
            .map(|block| {
                vec![
                    format!(
                        "[`{}`](#{})",
                        cell(&block.path.join(".")),
                        anchor(&block.path, prefix)
                    ),
                    format!("`{}`", address(&block.base)),
                    format!(
                        "`{}`",
                        address(&(&block.base + &block.map.size_bytes - 1u8))
                    ),
                    block.map.size_bytes.to_string(),
                    block.allocated.to_string(),
                    usage(&block.map.usage).into(),
                ]
            })
            .collect(),
    );
    for block in &blocks {
        heading(&mut lines, 2, &block.path.join("."), &block.path, prefix);
        let identity = &block.definition.vlnv;
        lines.extend([
            format!(
                "IP: `{}:{}:{}:{}`; data width: {} bits.",
                cell(&identity.vendor),
                cell(&identity.library),
                cell(&identity.name),
                cell(&identity.version),
                block.map.data_width_bits
            ),
            String::new(),
        ]);
        prose(&mut lines, &block.map.description);
        if !block.instances.is_empty() {
            prose(&mut lines, &block.definition.description);
            for (index, instance) in block.instances.iter().enumerate() {
                if instance
                    .description
                    .as_ref()
                    .is_some_and(|text| !text.is_empty())
                {
                    lines.extend([
                        format!("Instance {}:", block.path[..index + 1].join(".")),
                        String::new(),
                    ]);
                    prose(&mut lines, &instance.description);
                }
            }
        }
        if let Some(access_value) = &block.map.access {
            lines.extend([
                format!("SW access: `{}`.", access(access_value)),
                String::new(),
            ]);
        }
        let resets = block.resets()?;
        if !resets.is_empty() {
            lines.extend(["Reset types:".into(), String::new()]);
            for reset in resets {
                lines.push(format!(
                    "- **{}**{}",
                    reset.name,
                    reset
                        .description
                        .as_ref()
                        .filter(|text| !text.is_empty())
                        .map(|text| format!(": {}", cell(text)))
                        .unwrap_or_default()
                ));
            }
            lines.extend([
                String::new(),
                "Reset `?` bits and missing entries are unspecified, not retained.".into(),
                String::new(),
            ]);
        }
        if !block.map.registers.is_empty() {
            table(
                &mut lines,
                &[
                    "Register",
                    "Offset",
                    "Address",
                    "Width (bits)",
                    "Description",
                ],
                block
                    .map
                    .registers
                    .iter()
                    .map(|register| {
                        let mut path = block.path.clone();
                        path.push(register.name.clone());
                        vec![
                            if view == View::Registers {
                                format!("[`{}`](#{})", register.name, anchor(&path, prefix))
                            } else {
                                format!("`{}`", register.name)
                            },
                            format!("`{}`", hex(&register.offset)),
                            format!("`{}`", address(&(&block.base + &register.offset))),
                            register.width_bits.to_string(),
                            cell(register.description.as_deref().unwrap_or("")),
                        ]
                    })
                    .collect(),
            );
        }
        if view == View::Registers {
            for register in &block.map.registers {
                details(&mut lines, block, register, prefix)?;
            }
        }
    }
    Ok(lines.join("\n").trim_end().to_owned() + "\n")
}
