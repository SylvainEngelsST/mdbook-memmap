//! Add configuration without replacing existing user settings.

use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use toml_edit::{value, Array, DocumentMut, Item, Table};

pub fn install(root: &Path) -> Result<bool> {
    let config = root.join("book.toml");
    if config.is_symlink() {
        bail!("refusing to replace symbolic link {}", config.display());
    }
    let text =
        fs::read_to_string(&config).with_context(|| format!("cannot read {}", config.display()))?;
    let mut doc = text.parse::<DocumentMut>().context("invalid book.toml")?;
    let preprocessors = doc
        .entry("preprocessor")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("preprocessor must be a table"))?;
    if preprocessors.contains_key("memmap") {
        if !preprocessors["memmap"].is_table() && !preprocessors["memmap"].is_inline_table() {
            bail!("preprocessor.memmap must be a table");
        }
        return Ok(false);
    }
    let mut settings = Table::new();
    settings["command"] = value("mdbook-memmap");
    let mut roots = Array::new();
    roots.push(".");
    settings["source-roots"] = value(roots);
    let mut renderers = Array::new();
    renderers.push("html");
    settings["renderers"] = value(renderers);
    let mut before = Array::new();
    before.push("links");
    settings["before"] = value(before);
    preprocessors.insert("memmap", Item::Table(settings));
    let temporary = root.join(format!(".book.toml.memmap-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.set_permissions(fs::metadata(&config)?.permissions())?;
        file.write_all(doc.to_string().as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &config)?;
        Ok(())
    })();
    if result.is_err() {
        fs::remove_file(&temporary).context("cannot clean installer temporary file")?;
    }
    result.context("cannot update book.toml")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_preserves_user_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.toml");
        fs::write(&path, "# Keep me\n[book]\ntitle = 'Example'\n").unwrap();
        assert!(install(dir.path()).unwrap());
        let installed = fs::read_to_string(&path).unwrap();
        assert!(installed.contains("# Keep me"));
        assert!(installed.contains("title = 'Example'"));
        assert!(!installed.contains("backend"));
        assert!(!install(dir.path()).unwrap());
        assert_eq!(installed, fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn existing_configuration_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.toml");
        let original = "[preprocessor.memmap]\nsource-roots = ['/released']\n";
        fs::write(&path, original).unwrap();
        assert!(!install(dir.path()).unwrap());
        assert_eq!(original, fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn missing_or_invalid_configuration_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(install(dir.path()).is_err());
        let path = dir.path().join("book.toml");
        for text in [
            "invalid toml",
            "preprocessor = 4",
            "[preprocessor]\nmemmap = true",
        ] {
            fs::write(&path, text).unwrap();
            assert!(install(dir.path()).is_err());
            assert_eq!(text, fs::read_to_string(&path).unwrap());
        }
    }
}
