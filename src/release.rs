//! Consume portable version-1 locks produced by the standalone reference tools.

use crate::{
    model::{Definition, Kind, Vlnv},
    project::Project,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapEntry {
    source: String,
    map: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceEntry {
    source: String,
    address_space: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Entry {
    Map(MapEntry),
    Space(SpaceEntry),
}

impl Entry {
    fn source(&self) -> &str {
        match self {
            Self::Map(entry) => &entry.source,
            Self::Space(entry) => &entry.source,
        }
    }
    fn selector(&self) -> &str {
        match self {
            Self::Map(entry) => &entry.map,
            Self::Space(entry) => &entry.address_space,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub path: String,
    pub sha256: String,
    pub vlnv: Vlnv,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format_version: u32,
    release: String,
    entry: Entry,
    sources: Vec<Pin>,
}

#[derive(Debug)]
pub struct ReleaseLock {
    file: PathBuf,
    digest: String,
    document: Document,
    entry: PathBuf,
    pins: BTreeMap<PathBuf, Pin>,
}

impl ReleaseLock {
    pub fn load(file: &Path) -> Result<Self> {
        let file = file
            .canonicalize()
            .with_context(|| format!("{} [release-lock]", file.display()))?;
        let bytes =
            fs::read(&file).with_context(|| format!("{} [release-lock]", file.display()))?;
        let document: Document = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} [release-lock] invalid JSON lock", file.display()))?;
        if document.format_version != 1
            || document.release.trim().is_empty()
            || document.sources.is_empty()
            || document.entry.source().trim().is_empty()
            || document.entry.selector().trim().is_empty()
            || Path::new(document.entry.source()).is_absolute()
        {
            bail!(
                "{} [release-lock] expected version 1, nonblank release/entry and nonempty sources",
                file.display()
            );
        }
        let directory = file.parent().context("lock has no parent")?;
        let mut pins = BTreeMap::new();
        for pin in &document.sources {
            if pin.path.is_empty()
                || Path::new(&pin.path).is_absolute()
                || pin.sha256.len() != 64
                || !pin
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || [
                    &pin.vlnv.vendor,
                    &pin.vlnv.library,
                    &pin.vlnv.name,
                    &pin.vlnv.version,
                ]
                .iter()
                .any(|text| text.is_empty())
            {
                bail!(
                    "{} [release-lock] invalid relative source path, SHA-256, or VLNV",
                    file.display()
                );
            }
            let path = directory.join(&pin.path).canonicalize().with_context(|| {
                format!(
                    "{} [source-read] cannot resolve pinned source",
                    directory.join(&pin.path).display()
                )
            })?;
            if pins.insert(path.clone(), pin.clone()).is_some() {
                bail!(
                    "{} [release-lock] duplicate canonical source {}",
                    file.display(),
                    path.display()
                );
            }
        }
        let entry = directory
            .join(document.entry.source())
            .canonicalize()
            .with_context(|| format!("{} [release-lock] cannot resolve entry", file.display()))?;
        if !pins.contains_key(&entry) {
            bail!(
                "{} [release-lock] entry source is not pinned",
                file.display()
            );
        }
        Ok(Self {
            file,
            digest: sha256(&bytes),
            document,
            entry,
            pins,
        })
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn release(&self) -> &str {
        &self.document.release
    }
    pub fn pins(&self) -> &BTreeMap<PathBuf, Pin> {
        &self.pins
    }

    pub fn check_bytes(&self, file: &Path, bytes: &[u8]) -> Result<()> {
        if self
            .pins
            .get(file)
            .is_none_or(|pin| pin.sha256 != sha256(bytes))
        {
            bail!(
                "{} [release-pin] source is unpinned or differs from its locked SHA-256",
                file.display()
            );
        }
        Ok(())
    }

    pub fn check_identity(&self, definition: &Definition) -> Result<()> {
        if self
            .pins
            .get(&definition.origin.file)
            .is_none_or(|pin| pin.vlnv != definition.vlnv)
        {
            bail!(
                "{} [release-pin] VLNV differs from its lock",
                definition.origin
            );
        }
        Ok(())
    }

    /// Validate every pin and its transitive references, even if not displayed.
    pub fn verify_inventory(&self, project: &mut Project) -> Result<()> {
        for path in self.pins.keys() {
            project.load(path)?;
        }
        let definition = project.load(&self.entry)?;
        match &self.document.entry {
            Entry::Space(entry) => {
                project.resolve(&self.entry, &entry.address_space)?;
            }
            Entry::Map(entry) => {
                if !matches!(&definition.kind, Kind::Ip { maps, .. } if maps.iter().any(|map| map.name == entry.map))
                {
                    bail!(
                        "{} [release-lock] entry map must select an existing local IP map",
                        self.file.display()
                    );
                }
            }
        }
        self.verify_snapshots(project)?;
        self.verify_unchanged()
    }

    pub fn verify_snapshots(&self, project: &Project) -> Result<()> {
        for (path, bytes) in project.snapshots() {
            self.check_bytes(path, bytes)?;
        }
        for definition in project.definitions() {
            self.check_identity(definition)?;
        }
        Ok(())
    }

    /// Recheck disk bytes immediately before publishing a provenance report.
    pub fn verify_unchanged(&self) -> Result<()> {
        if sha256(
            &fs::read(&self.file)
                .with_context(|| format!("{} [release-pin]", self.file.display()))?,
        ) != self.digest
        {
            bail!(
                "{} [release-pin] release lock changed during the operation",
                self.file.display()
            );
        }
        for (path, pin) in &self.pins {
            let declared = self
                .file
                .parent()
                .context("lock has no parent")?
                .join(&pin.path);
            if declared
                .canonicalize()
                .with_context(|| format!("{} [source-read]", declared.display()))?
                != *path
            {
                bail!(
                    "{} [release-pin] pinned source path changed during the operation",
                    declared.display()
                );
            }
            let bytes =
                fs::read(path).with_context(|| format!("{} [source-read]", path.display()))?;
            self.check_bytes(path, &bytes)?;
        }
        Ok(())
    }
}
