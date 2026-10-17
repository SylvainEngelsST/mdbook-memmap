//! Load and locally validate authoring version 1 definitions.

use crate::model::*;
use crate::source::{self, Node};
use anyhow::{Context, Result};
use std::path::Path;

pub fn load_definition(path: &Path) -> Result<Definition> {
    let file = path
        .canonicalize()
        .with_context(|| format!("{} [source-read]", path.display()))?;
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("{} [source-read]", file.display()))?;
    parse_definition(&file, &text)
}

pub(crate) fn parse_definition(file: &Path, text: &str) -> Result<Definition> {
    let root = source::parse(file, text)?;
    let kind = root.get("kind")?.text()?;
    let common = [
        "schema_version",
        "naming_profile",
        "kind",
        "vlnv",
        "description",
    ];
    let extra = match kind.as_str() {
        "ip" => &["maps", "reset_types"][..],
        "system" => &["address_spaces"][..],
        _ => return Err(root.error("schema", "kind must be ip or system")),
    };
    root.object(&[common.as_slice(), extra].concat())?;
    if root.get("schema_version")?.integer(false)? != 1u8.into()
        || root.get("naming_profile")?.text()? != "rds-v1"
    {
        return Err(root.error(
            "schema",
            "require schema_version 1 and naming_profile rds-v1",
        ));
    }
    let identity = root.get("vlnv")?;
    identity.object(&["vendor", "library", "name", "version"])?;
    let definition = Definition {
        vlnv: Vlnv {
            vendor: identity.get("vendor")?.nonempty()?,
            library: identity.get("library")?.nonempty()?,
            name: identity.get("name")?.nonempty()?,
            version: identity.get("version")?.nonempty()?,
        },
        description: root.description()?,
        origin: root.origin.clone(),
        kind: if kind == "ip" {
            Kind::Ip {
                maps: root
                    .get("maps")?
                    .list()?
                    .iter()
                    .map(memory_map)
                    .collect::<Result<_>>()?,
                reset_types: root
                    .items("reset_types")?
                    .iter()
                    .map(|node| {
                        node.object(&["name", "description"])?;
                        Ok(ResetType {
                            name: node.name(true)?,
                            description: node.description()?,
                            origin: node.origin.clone(),
                        })
                    })
                    .collect::<Result<_>>()?,
            }
        } else {
            Kind::System {
                address_spaces: root
                    .get("address_spaces")?
                    .list()?
                    .iter()
                    .map(address_space)
                    .collect::<Result<_>>()?,
            }
        },
    };
    crate::validation::validate(&definition)?;
    Ok(definition)
}

fn memory_map(node: &Node) -> Result<MemoryMap> {
    node.object(&[
        "name",
        "description",
        "usage",
        "size_bytes",
        "data_width_bits",
        "access",
        "registers",
    ])?;
    let usage = node
        .optional("usage")
        .map(Node::enumeration)
        .transpose()?
        .unwrap_or(Usage::Register);
    let access = node.optional("access").map(Node::enumeration).transpose()?;
    match usage {
        Usage::Register => {
            node.get("registers")?.list()?;
            if access.is_some() {
                return Err(node.error("schema", "register maps cannot define access"));
            }
        }
        Usage::Memory | Usage::Reserved => {
            if node.optional("registers").is_some() {
                return Err(node.error("schema", "non-register maps cannot define registers"));
            }
            if (usage == Usage::Memory) != access.is_some() {
                return Err(node.error(
                    "schema",
                    "only memory maps require access; reserved maps cannot define access",
                ));
            }
        }
    }
    Ok(MemoryMap {
        name: node.name(false)?,
        description: node.description()?,
        usage: usage.clone(),
        size_bytes: node.get("size_bytes")?.integer(true)?,
        data_width_bits: node.get("data_width_bits")?.width(true)?,
        access: if usage == Usage::Reserved {
            Some(Access::NoAccess)
        } else {
            access
        },
        registers: node
            .items("registers")?
            .iter()
            .map(register)
            .collect::<Result<_>>()?,
        origin: node.origin.clone(),
    })
}

fn register(node: &Node) -> Result<Register> {
    node.object(&["name", "description", "offset", "width_bits", "fields"])?;
    Ok(Register {
        name: node.name(true)?,
        description: node.description()?,
        offset: node.get("offset")?.integer(false)?,
        width_bits: node.get("width_bits")?.width(true)?,
        fields: node
            .get("fields")?
            .list()?
            .iter()
            .map(field)
            .collect::<Result<_>>()?,
        origin: node.origin.clone(),
    })
}

fn field(node: &Node) -> Result<Field> {
    node.object(&[
        "name",
        "description",
        "lsb",
        "width_bits",
        "access",
        "hwAccess",
        "volatile",
        "modified_write_value",
        "read_action",
        "resets",
    ])?;
    use num_traits::ToPrimitive;
    let lsb = node
        .get("lsb")?
        .integer(false)?
        .to_u64()
        .ok_or_else(|| node.error("schema", "lsb exceeds checked 64-bit capacity"))?;
    Ok(Field {
        name: node.name(true)?,
        description: node.description()?,
        lsb,
        width_bits: node.get("width_bits")?.width(false)?,
        access: node.get("access")?.enumeration()?,
        hw_access: node.get("hwAccess")?.enumeration()?,
        volatile: node.get("volatile")?.boolean()?,
        modified_write_value: node
            .optional("modified_write_value")
            .map(Node::enumeration)
            .transpose()?,
        read_action: node
            .optional("read_action")
            .map(Node::enumeration)
            .transpose()?,
        resets: node
            .items("resets")?
            .iter()
            .map(|reset| {
                reset.object(&["resetType", "value", "mask"])?;
                Ok(Reset {
                    reset_type: reset.get("resetType")?.identifier(true)?,
                    value: reset.get("value")?.integer(false)?,
                    mask: reset.get("mask")?.integer(false)?,
                    origin: reset.origin.clone(),
                })
            })
            .collect::<Result<_>>()?,
        origin: node.origin.clone(),
    })
}

fn address_space(node: &Node) -> Result<AddressSpace> {
    node.object(&[
        "name",
        "description",
        "address_width_bits",
        "data_width_bits",
        "size_bytes",
        "instances",
    ])?;
    Ok(AddressSpace {
        name: node.name(false)?,
        description: node.description()?,
        address_width_bits: node.get("address_width_bits")?.width(false)?,
        data_width_bits: node.get("data_width_bits")?.width(true)?,
        size_bytes: node.get("size_bytes")?.integer(true)?,
        instances: node
            .get("instances")?
            .list()?
            .iter()
            .map(|instance| {
                instance.object(&["name", "description", "source", "mappings"])?;
                Ok(Instance {
                    name: instance.name(true)?,
                    description: instance.description()?,
                    source: instance.get("source")?.nonempty()?,
                    mappings: instance
                        .get("mappings")?
                        .list()?
                        .iter()
                        .map(|mapping| {
                            mapping.object(&["target", "base", "size_bytes"])?;
                            Ok(Mapping {
                                target: mapping.get("target")?.identifier(false)?,
                                base: mapping.get("base")?.integer(false)?,
                                size_bytes: mapping.get("size_bytes")?.integer(true)?,
                                origin: mapping.origin.clone(),
                            })
                        })
                        .collect::<Result<_>>()?,
                    origin: instance.origin.clone(),
                })
            })
            .collect::<Result<_>>()?,
        origin: node.origin.clone(),
    })
}
