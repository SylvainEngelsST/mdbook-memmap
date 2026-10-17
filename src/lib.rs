//! Native memory-map preprocessor for mdBook.
//!
//! This initial package establishes the protocol and authoring contract.
//! Native source loading and rendering are introduced at subsequent review gates.

pub mod fences;
pub mod install;

use anyhow::{bail, Context, Result};
use mdbook_preprocessor::book::{Book, BookItem};
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use semver::{Version, VersionReq};
use std::path::Path;

#[derive(Debug, Default)]
pub struct Memmap;

impl Preprocessor for Memmap {
    fn name(&self) -> &str {
        "memmap"
    }

    fn supports_renderer(&self, renderer: &str) -> Result<bool> {
        Ok(renderer == "html")
    }

    fn run(&self, ctx: &PreprocessorContext, book: Book) -> Result<Book> {
        let version = Version::parse(&ctx.mdbook_version).context("invalid mdBook version")?;
        if !VersionReq::parse(">=0.5.4, <0.6.0")?.matches(&version) {
            bail!("unsupported mdBook {}; expected >=0.5.4, <0.6.0", version);
        }
        if !self.supports_renderer(&ctx.renderer)? {
            bail!("only the html renderer is supported");
        }
        inspect_items(&book.items)?;
        Ok(book)
    }
}

fn inspect_items(items: &[BookItem]) -> Result<()> {
    for item in items {
        if let BookItem::Chapter(chapter) = item {
            let path = chapter
                .source_path
                .as_deref()
                .or(chapter.path.as_deref())
                .unwrap_or(Path::new("<synthetic>"));
            let inclusions = fences::parse(&chapter.content)
                .with_context(|| format!("chapter {}", path.display()))?;
            if let Some(inclusion) = inclusions.first() {
                bail!(
                    "{}:{}: native memory-map rendering is not available at checkpoint 1",
                    path.display(),
                    inclusion.line
                );
            }
            inspect_items(&chapter.sub_items)?;
        }
    }
    Ok(())
}
