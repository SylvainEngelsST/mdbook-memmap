use anyhow::{bail, Context, Result};
use std::{fs, io::Write, path::Path};

pub(crate) fn write_atomic(destination: &Path, bytes: &[u8]) -> Result<()> {
    if destination.is_symlink() {
        bail!(
            "refusing to replace symbolic link {}",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .context("output needs a parent directory")?;
    let mut file = tempfile::Builder::new()
        .prefix(".mdbook-memmap-")
        .tempfile_in(parent)
        .with_context(|| format!("cannot create temporary output in {}", parent.display()))?;
    match fs::metadata(destination) {
        Ok(metadata) => file.as_file().set_permissions(metadata.permissions())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot inspect existing output"),
    }
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    if destination.is_symlink() {
        bail!(
            "refusing to replace symbolic link {}",
            destination.display()
        );
    }
    file.persist(destination)
        .map_err(|error| error.error)
        .with_context(|| format!("cannot publish {}", destination.display()))?;
    Ok(())
}
