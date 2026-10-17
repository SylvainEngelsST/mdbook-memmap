//! Native memory-map preprocessor for mdBook.
//!
//! YAML loading, validation, hierarchy resolution, and Markdown rendering run
//! directly in this process, without a Python runtime or backend subprocess.

pub mod fences;
pub mod install;
pub mod loader;
pub mod markdown;
pub mod model;
pub mod project;
mod source;
mod validation;

use anyhow::{bail, Context, Result};
use mdbook_preprocessor::book::{Book, BookItem};
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Config {
    source_roots: Vec<PathBuf>,
    optional: Option<bool>,
    #[serde(rename = "command")]
    _command: Option<String>,
    #[serde(rename = "before")]
    _before: Option<Vec<String>>,
    #[serde(rename = "after")]
    _after: Option<Vec<String>>,
    #[serde(rename = "renderers")]
    _renderers: Option<Vec<String>>,
}

fn configuration(ctx: &PreprocessorContext) -> Result<Option<Config>> {
    let value = ctx.config.get::<serde_json::Value>("preprocessor.memmap")?;
    let Some(value) = value else {
        return Ok(None);
    };
    for key in ["backend", "backend-args", "backend-timeout-seconds"] {
        if value.get(key).is_some() {
            bail!("obsolete setting {key:?}: native mdbook-memmap does not use a backend; remove backend settings");
        }
    }
    for key in ["release-lock", "provenance"] {
        if value.get(key).is_some() {
            bail!("{key:?} is not yet supported: release-lock/provenance compatibility is planned for checkpoint 4; do not use this build for qualified release books");
        }
    }
    let settings: Config =
        serde_json::from_value(value).context("invalid preprocessor.memmap configuration")?;
    if settings.source_roots.is_empty() {
        bail!("source-roots must contain at least one allowed directory");
    }
    if settings.optional == Some(true) {
        bail!("optional=true is unsupported: memory-map validation must fail the book build");
    }
    Ok(Some(settings))
}

#[derive(Debug, Default)]
pub struct Memmap;

impl Preprocessor for Memmap {
    fn name(&self) -> &str {
        "memmap"
    }

    fn supports_renderer(&self, renderer: &str) -> Result<bool> {
        Ok(renderer == "html")
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let version = Version::parse(&ctx.mdbook_version).context("invalid mdBook version")?;
        if !VersionReq::parse(">=0.5.4, <0.6.0")?.matches(&version) {
            bail!("unsupported mdBook {}; expected >=0.5.4, <0.6.0", version);
        }
        if !self.supports_renderer(&ctx.renderer)? {
            bail!("only the html renderer is supported");
        }
        let settings = configuration(ctx)?;
        let mut project = None;
        let mut chapter_id = 0;
        process_items(
            &mut book.items,
            ctx,
            settings.as_ref(),
            &mut project,
            &mut chapter_id,
        )?;
        Ok(book)
    }
}

fn process_items(
    items: &mut [BookItem],
    ctx: &PreprocessorContext,
    settings: Option<&Config>,
    project: &mut Option<project::Project>,
    chapter_id: &mut usize,
) -> Result<()> {
    for item in items {
        if let BookItem::Chapter(chapter) = item {
            *chapter_id += 1;
            let path = chapter
                .source_path
                .as_deref()
                .or(chapter.path.as_deref())
                .unwrap_or(Path::new("<synthetic>"));
            let inclusions = fences::parse(&chapter.content)
                .with_context(|| format!("chapter {}", path.display()))?;
            if !inclusions.is_empty() {
                let first_line = inclusions[0].line;
                let location = format!("{}:{first_line}", path.display());
                let settings = settings.with_context(|| {
                    format!("{location}: [preprocessor.memmap] with source-roots is required")
                })?;
                if chapter.source_path.is_none() && chapter.path.is_none() {
                    bail!("{location}: an active inclusion needs a chapter source_path or path");
                }
                let source = ctx
                    .root
                    .join(&ctx.config.book.src)
                    .join(path)
                    .canonicalize()
                    .with_context(|| format!("{location}: cannot resolve chapter source"))?;
                let directory = source.parent().context("chapter has no parent directory")?;
                let project = match &mut *project {
                    Some(project) => project,
                    slot @ None => slot.insert(
                        project::Project::new(
                            &settings
                                .source_roots
                                .iter()
                                .map(|root| ctx.root.join(root))
                                .collect::<Vec<_>>(),
                        )
                        .with_context(|| format!("{location}: invalid source roots"))?,
                    ),
                };
                let mut output = String::new();
                let mut end = 0;
                for (index, inclusion) in inclusions.iter().enumerate() {
                    let fragment = render_inclusion(
                        project,
                        &directory.join(&inclusion.fence.source),
                        &inclusion.fence,
                        &format!("memmap-{}-{}", chapter_id, index + 1),
                    )
                    .with_context(|| {
                        format!(
                            "{}:{}: memmap inclusion {}",
                            source.display(),
                            inclusion.line,
                            index + 1
                        )
                    })?;
                    output.push_str(&chapter.content[end..inclusion.range.start]);
                    output.push('\n');
                    output.push_str(&fragment);
                    output.push('\n');
                    end = inclusion.range.end;
                }
                output.push_str(&chapter.content[end..]);
                chapter.content = output;
            }
            process_items(&mut chapter.sub_items, ctx, settings, project, chapter_id)?;
        }
    }
    Ok(())
}

fn render_inclusion(
    project: &mut project::Project,
    file: &Path,
    fence: &fences::Fence,
    prefix: &str,
) -> Result<String> {
    let definition = project.load(file)?;
    match &definition.kind {
        model::Kind::Ip { .. } => {
            let map = fence
                .map
                .as_deref()
                .context("[export-selector] IP input requires map, not address_space")?;
            markdown::render(
                markdown::Source::Ip(&definition),
                Some(map),
                fence.view,
                prefix,
            )
        }
        model::Kind::System { .. } => {
            let space = fence
                .address_space
                .as_deref()
                .context("[export-selector] system input requires address_space, not map")?;
            let view = project.resolve(file, space)?;
            markdown::render(markdown::Source::System(&view), None, fence.view, prefix)
        }
    }
}
