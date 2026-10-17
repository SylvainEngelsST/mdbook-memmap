use crate::model::*;
use anyhow::{bail, Result};
use num_bigint::BigUint;
use std::collections::BTreeSet;

fn unique<'a>(items: impl Iterator<Item = (&'a str, &'a Origin)>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (name, origin) in items {
        if !seen.insert(name) {
            bail!("{origin} [duplicate-name] duplicate name {name:?}");
        }
    }
    Ok(())
}

fn disjoint(mut intervals: Vec<(BigUint, BigUint, &Origin)>) -> Result<()> {
    intervals.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    for pair in intervals.windows(2) {
        if pair[1].0 < pair[0].1 {
            bail!("{} [overlap] overlaps {}", pair[1].2, pair[0].2);
        }
    }
    Ok(())
}

pub(crate) fn validate(definition: &Definition) -> Result<()> {
    match &definition.kind {
        Kind::Ip { reset_types, maps } => {
            unique(
                reset_types
                    .iter()
                    .map(|item| (item.name.as_str(), &item.origin)),
            )?;
            unique(maps.iter().map(|item| (item.name.as_str(), &item.origin)))?;
            let resets: BTreeSet<_> = reset_types.iter().map(|reset| &reset.name).collect();
            for map in maps {
                unique(
                    map.registers
                        .iter()
                        .map(|item| (item.name.as_str(), &item.origin)),
                )?;
                for register in &map.registers {
                    let size = BigUint::from(register.width_bits / 8);
                    if &register.offset % &size != BigUint::from(0u8) {
                        bail!(
                            "{} [register-alignment] offset must align to {size} bytes",
                            register.origin
                        );
                    }
                    if &register.offset + &size > map.size_bytes {
                        bail!(
                            "{} [register-bounds] register extends beyond map",
                            register.origin
                        );
                    }
                    unique(
                        register
                            .fields
                            .iter()
                            .map(|item| (item.name.as_str(), &item.origin)),
                    )?;
                    for field in &register.fields {
                        if field
                            .lsb
                            .checked_add(field.width_bits)
                            .is_none_or(|end| end > register.width_bits)
                        {
                            bail!(
                                "{} [field-bounds] field extends beyond register",
                                field.origin
                            );
                        }
                        if matches!(
                            field.hw_access,
                            HardwareAccess::ReadWrite | HardwareAccess::WriteOnly
                        ) && !field.volatile
                        {
                            bail!(
                                "{} [schema] hardware-writable fields must be volatile",
                                field.origin
                            );
                        }
                        if field.modified_write_value.is_some()
                            && !matches!(
                                field.access,
                                Access::WriteOnly
                                    | Access::ReadWrite
                                    | Access::WriteOnce
                                    | Access::ReadWriteOnce
                            )
                        {
                            bail!(
                                "{} [schema] write effect requires software write access",
                                field.origin
                            );
                        }
                        if field.read_action.is_some()
                            && !matches!(
                                field.access,
                                Access::ReadOnly | Access::ReadWrite | Access::ReadWriteOnce
                            )
                        {
                            bail!(
                                "{} [schema] read effect requires software read access",
                                field.origin
                            );
                        }
                        let mut seen = BTreeSet::new();
                        for reset in &field.resets {
                            if !resets.contains(&reset.reset_type) {
                                bail!(
                                    "{} [reset-reference] undeclared reset {}",
                                    reset.origin,
                                    reset.reset_type
                                );
                            }
                            if !seen.insert(&reset.reset_type) {
                                bail!(
                                    "{} [reset-duplicate] repeated reset {}",
                                    reset.origin,
                                    reset.reset_type
                                );
                            }
                            if reset.value.bits() > field.width_bits
                                || reset.mask.bits() > field.width_bits
                            {
                                bail!(
                                    "{} [reset-width] value and mask must fit field",
                                    reset.origin
                                );
                            }
                        }
                    }
                    disjoint(
                        register
                            .fields
                            .iter()
                            .map(|field| {
                                (
                                    BigUint::from(field.lsb),
                                    BigUint::from(field.lsb) + field.width_bits,
                                    &field.origin,
                                )
                            })
                            .collect(),
                    )?;
                }
                disjoint(
                    map.registers
                        .iter()
                        .map(|register| {
                            (
                                register.offset.clone(),
                                &register.offset + register.width_bits / 8,
                                &register.origin,
                            )
                        })
                        .collect(),
                )?;
            }
        }
        Kind::System { address_spaces } => {
            unique(
                address_spaces
                    .iter()
                    .map(|item| (item.name.as_str(), &item.origin)),
            )?;
            for space in address_spaces {
                if (&space.size_bytes - 1u8).bits() > space.address_width_bits {
                    bail!(
                        "{} [address-width] range exceeds address width",
                        space.origin
                    );
                }
                unique(
                    space
                        .instances
                        .iter()
                        .map(|item| (item.name.as_str(), &item.origin)),
                )?;
                let mut allocations = Vec::new();
                for instance in &space.instances {
                    let mut targets = BTreeSet::new();
                    for mapping in &instance.mappings {
                        if !targets.insert(&mapping.target) {
                            bail!(
                                "{} [mapping-duplicate] aliases are unsupported",
                                mapping.origin
                            );
                        }
                        let end = &mapping.base + &mapping.size_bytes;
                        if end > space.size_bytes {
                            bail!(
                                "{} [mapping-bounds] allocation extends beyond address space",
                                mapping.origin
                            );
                        }
                        allocations.push((mapping.base.clone(), end, &mapping.origin));
                    }
                }
                disjoint(allocations)?;
            }
        }
    }
    Ok(())
}
