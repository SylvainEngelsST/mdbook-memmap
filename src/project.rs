//! Canonical-root restricted, per-build source snapshot and hierarchy resolver.

use crate::{loader, model::*};
use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Project {
    roots: Vec<PathBuf>,
    definitions: BTreeMap<PathBuf, Arc<Definition>>,
    identities: BTreeMap<Vlnv, PathBuf>,
    snapshots: BTreeMap<PathBuf, Vec<u8>>,
}

impl Project {
    pub fn new(roots: &[PathBuf]) -> Result<Self> {
        if roots.is_empty() {
            bail!("[source-root] at least one source root required");
        }
        let roots = roots
            .iter()
            .map(|root| {
                let path = root
                    .canonicalize()
                    .with_context(|| format!("{} [source-root]", root.display()))?;
                if !path.is_dir() {
                    bail!("{} [source-root] root is not a directory", path.display());
                }
                Ok(path)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            roots,
            definitions: BTreeMap::new(),
            identities: BTreeMap::new(),
            snapshots: BTreeMap::new(),
        })
    }

    pub fn definitions(&self) -> impl Iterator<Item = &Arc<Definition>> {
        self.definitions.values()
    }

    pub fn snapshots(&self) -> &BTreeMap<PathBuf, Vec<u8>> {
        &self.snapshots
    }

    pub fn load(&mut self, file: &Path) -> Result<Arc<Definition>> {
        let result = self.load_inner(file, &mut Vec::new());
        if result.is_err() {
            self.definitions.clear();
            self.identities.clear();
            self.snapshots.clear();
        }
        result
    }

    fn canonical(&self, file: &Path) -> Result<PathBuf> {
        let path = file
            .canonicalize()
            .with_context(|| format!("{} [source-read]", file.display()))?;
        if !self.roots.iter().any(|root| path.starts_with(root)) {
            bail!(
                "{} [source-root] source outside configured roots",
                path.display()
            );
        }
        Ok(path)
    }

    fn load_inner(&mut self, file: &Path, active: &mut Vec<PathBuf>) -> Result<Arc<Definition>> {
        let path = self.canonical(file)?;
        if active.contains(&path) {
            bail!(
                "{} [hierarchy-cycle] cycle through {active:?}",
                path.display()
            );
        }
        if let Some(definition) = self.definitions.get(&path) {
            return Ok(Arc::clone(definition));
        }
        if active.len() >= 128 {
            bail!(
                "{} [hierarchy-depth] exceeds 128 source levels",
                path.display()
            );
        }
        let bytes =
            std::fs::read(&path).with_context(|| format!("{} [source-read]", path.display()))?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("{} [source-read] invalid UTF-8", path.display()))?;
        let definition = Arc::new(loader::parse_definition(&path, text)?);
        if let Some(other) = self.identities.get(&definition.vlnv) {
            if other != &path {
                bail!(
                    "{} [vlnv-conflict] identity already assigned to {}",
                    definition.origin,
                    other.display()
                );
            }
        }
        self.identities
            .insert(definition.vlnv.clone(), path.clone());
        active.push(path.clone());
        if let Kind::System { address_spaces } = &definition.kind {
            for space in address_spaces {
                for instance in &space.instances {
                    let child = self
                        .load_inner(
                            &path
                                .parent()
                                .context("source has no parent")?
                                .join(&instance.source),
                            active,
                        )
                        .with_context(|| {
                            format!("{} referenced source {}", instance.origin, instance.source)
                        })?;
                    for mapping in &instance.mappings {
                        let size = match &child.kind {
                            Kind::Ip { maps, .. } => maps
                                .iter()
                                .find(|item| item.name == mapping.target)
                                .map(|item| &item.size_bytes),
                            Kind::System { address_spaces } => address_spaces
                                .iter()
                                .find(|item| item.name == mapping.target)
                                .map(|item| &item.size_bytes),
                        }
                        .with_context(|| {
                            format!(
                                "{} [mapping-target] unknown target {}",
                                mapping.origin, mapping.target
                            )
                        })?;
                        if size > &mapping.size_bytes {
                            bail!(
                                "{} [target-bounds] target needs {size} bytes, allocation has {}",
                                mapping.origin,
                                mapping.size_bytes
                            );
                        }
                    }
                }
            }
        }
        active.pop();
        self.snapshots.insert(path.clone(), bytes);
        self.definitions.insert(path, Arc::clone(&definition));
        Ok(definition)
    }

    pub fn resolve(&mut self, file: &Path, name: &str) -> Result<ResolvedView> {
        let definition = self.load(file)?;
        let Kind::System { address_spaces } = &definition.kind else {
            bail!(
                "{} [view-kind] resolution requires a system definition",
                definition.origin
            );
        };
        let space = address_spaces
            .iter()
            .find(|space| space.name == name)
            .with_context(|| {
                format!(
                    "{} [view-name] unknown address space {name}",
                    definition.origin
                )
            })?
            .clone();
        let mut blocks = Vec::new();
        self.walk(&definition, &space, &BigUint::from(0u8), &[], &mut blocks)?;
        check_flat_names(&blocks)?;
        Ok(ResolvedView {
            definition,
            address_space: space,
            blocks,
        })
    }

    fn walk(
        &self,
        definition: &Definition,
        space: &AddressSpace,
        base: &BigUint,
        chain: &[Instance],
        blocks: &mut Vec<ResolvedBlock>,
    ) -> Result<()> {
        for instance in &space.instances {
            let child_path = self.canonical(
                &definition
                    .origin
                    .file
                    .parent()
                    .context("source has no parent")?
                    .join(&instance.source),
            )?;
            let child = self
                .definitions
                .get(&child_path)
                .context("source snapshot missing definition")?;
            let mut instances = chain.to_vec();
            instances.push(instance.clone());
            for mapping in &instance.mappings {
                let absolute = base + &mapping.base;
                match &child.kind {
                    Kind::Ip { maps, .. } => {
                        let map = maps
                            .iter()
                            .find(|map| map.name == mapping.target)
                            .context("validated map missing")?;
                        for register in &map.registers {
                            if (&absolute + &register.offset) % (register.width_bits / 8)
                                != BigUint::from(0u8)
                            {
                                bail!(
                                    "{} [resolved-alignment] misaligns register {}",
                                    mapping.origin,
                                    register.name
                                );
                            }
                        }
                        blocks.push(ResolvedBlock {
                            definition: Arc::clone(child),
                            memory_map: map.clone(),
                            instances: instances.clone(),
                            base: absolute,
                            mapping: mapping.clone(),
                        });
                    }
                    Kind::System { address_spaces } => {
                        let target = address_spaces
                            .iter()
                            .find(|space| space.name == mapping.target)
                            .context("validated address space missing")?;
                        self.walk(child, target, &absolute, &instances, blocks)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn reserve(
    names: &mut BTreeMap<String, Vec<String>>,
    identity: Vec<String>,
    origin: &Origin,
) -> Result<()> {
    let name = identity.join("_");
    if let Some(previous) = names.get(&name) {
        if previous != &identity {
            bail!("{origin} [flattened-name] ambiguous name {name}");
        }
    }
    names.insert(name, identity);
    Ok(())
}

fn check_flat_names(blocks: &[ResolvedBlock]) -> Result<()> {
    let mut names = BTreeMap::new();
    let mut resets = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for block in blocks {
        let mut identity = block.instance_path();
        identity.push(block.memory_map.name.clone());
        if !identities.insert(identity.clone()) {
            bail!("{} [resolved-alias] repeated map", block.mapping.origin);
        }
        reserve(&mut names, identity, &block.mapping.origin)?;
        if let Kind::Ip { reset_types, .. } = &block.definition.kind {
            for reset in reset_types {
                let mut identity = block.instance_path();
                identity.push(reset.name.clone());
                reserve(&mut resets, identity, &block.mapping.origin)?;
            }
        }
    }
    Ok(())
}
