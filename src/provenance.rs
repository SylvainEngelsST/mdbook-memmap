//! Deterministic native-generator provenance; not a signature or HTML attestation.

use crate::{
    artifacts,
    project::Project,
    release::{sha256, ReleaseLock},
};
use anyhow::{bail, Context, Result};
use mdbook_preprocessor::PreprocessorContext;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

include!(concat!(env!("OUT_DIR"), "/implementation.rs"));

pub fn generator_identity() -> Result<Value> {
    let files: BTreeMap<_, _> = IMPLEMENTATION_FILES
        .iter()
        .map(|(name, bytes)| (*name, sha256(bytes)))
        .collect();
    let executable = std::env::current_exe().context("cannot identify running executable")?;
    Ok(json!({
        "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"),
        "kind": "native-rust",
        "implementation_sha256": sha256(&serde_json::to_vec(&files)?),
        "executable_sha256": sha256(&fs::read(&executable).context("cannot hash running executable")?),
    }))
}

fn destination(
    ctx: &PreprocessorContext,
    destination: &Path,
    lock: &ReleaseLock,
    project: &Project,
    chapters: &BTreeSet<PathBuf>,
) -> Result<PathBuf> {
    let path = ctx.root.join(destination);
    let parent = path
        .parent()
        .context("provenance needs a parent directory")?
        .canonicalize()
        .context("cannot resolve provenance parent")?;
    let path = parent.join(path.file_name().context("provenance needs a filename")?);
    let chapter_root = ctx
        .root
        .join(&ctx.config.book.src)
        .canonicalize()
        .context("cannot resolve book source directory")?;
    let config = ctx.root.canonicalize()?.join("book.toml");
    let config_target = match config.canonicalize() {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("cannot resolve book configuration"),
    };
    if path.is_symlink()
        || project
            .source_roots()
            .iter()
            .any(|root| path.starts_with(root))
        || path.starts_with(chapter_root)
        || chapters.contains(&path)
        || path == lock.file()
        || path == config
        || config_target.as_ref() == Some(&path)
        || path == std::env::current_exe()?.canonicalize()?
    {
        bail!("{} [provenance-output] provenance cannot overwrite inputs, source trees, configuration, or the executable", path.display());
    }
    Ok(path)
}

pub(crate) fn publish(
    ctx: &PreprocessorContext,
    output: &Path,
    lock: &ReleaseLock,
    project: &Project,
    selections: Vec<Value>,
    chapters: &BTreeSet<PathBuf>,
) -> Result<()> {
    if selections.is_empty() {
        bail!("[release-lock] locked book must contain at least one memmap inclusion");
    }
    let output = destination(ctx, output, lock, project, chapters)?;
    lock.verify_snapshots(project)?;
    let mut pins: Vec<_> = lock.pins().values().collect();
    pins.sort_by(|left, right| left.path.cmp(&right.path));
    let report = json!({
        "format_version": 2,
        "adapter": {"name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")},
        "mdbook_version": ctx.mdbook_version,
        "source_roots": project.source_roots(),
        "inputs": {
            "format_version": 1, "release": lock.release(), "release_lock_sha256": lock.digest(),
            "sources": pins, "generator": generator_identity()?, "selections": selections,
        }
    });
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    lock.verify_unchanged()?;
    artifacts::write_atomic(&output, &bytes).context("cannot write native provenance")
}
