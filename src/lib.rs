//! Native memory-map preprocessor for mdBook.
//!
//! YAML loading, validation, hierarchy resolution, and Markdown rendering run
//! directly in this process, without a Python runtime or backend subprocess.

mod artifacts;
pub mod fences;
pub mod install;
pub mod loader;
pub mod markdown;
pub mod model;
pub mod project;
pub mod provenance;
pub mod release;
mod source;
mod validation;

use anyhow::{bail, Context, Result};
use mdbook_preprocessor::book::{Book, BookItem};
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Config {
    source_roots: Vec<PathBuf>,
    optional: Option<bool>,
    release_lock: Option<PathBuf>,
    provenance: Option<PathBuf>,
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
    let settings: Config =
        serde_json::from_value(value).context("invalid preprocessor.memmap configuration")?;
    if settings.source_roots.is_empty() {
        bail!("source-roots must contain at least one allowed directory");
    }
    if settings.optional == Some(true) {
        bail!("optional=true is unsupported: memory-map validation must fail the book build");
    }
    if settings.release_lock.is_some() != settings.provenance.is_some() {
        bail!("release-lock and provenance must be configured together");
    }
    if settings
        .release_lock
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
        || settings
            .provenance
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
    {
        bail!("release-lock and provenance must be nonempty paths");
    }
    Ok(Some(settings))
}

#[derive(Default)]
struct Build {
    project: Option<project::Project>,
    chapter_id: usize,
    release_lock: Option<Arc<release::ReleaseLock>>,
    selections: Vec<serde_json::Value>,
    protected_chapters: std::collections::BTreeSet<PathBuf>,
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
        let mut build = Build::default();
        if let Some(settings) = &settings {
            if let Some(file) = &settings.release_lock {
                let lock = Arc::new(release::ReleaseLock::load(&ctx.root.join(file))?);
                let mut project = project::Project::with_release_lock(
                    &settings
                        .source_roots
                        .iter()
                        .map(|root| ctx.root.join(root))
                        .collect::<Vec<_>>(),
                    Arc::clone(&lock),
                )?;
                lock.verify_inventory(&mut project)?;
                build.project = Some(project);
                build.release_lock = Some(lock);
            }
        }
        process_items(&mut book.items, ctx, settings.as_ref(), &mut build)?;
        if let Some(lock) = &build.release_lock {
            provenance::publish(
                ctx,
                settings
                    .as_ref()
                    .and_then(|settings| settings.provenance.as_deref())
                    .context("locked build needs provenance output")?,
                lock,
                build
                    .project
                    .as_ref()
                    .context("locked build needs source project")?,
                build.selections,
                &build.protected_chapters,
            )?;
        }
        Ok(book)
    }
}

fn process_items(
    items: &mut [BookItem],
    ctx: &PreprocessorContext,
    settings: Option<&Config>,
    build: &mut Build,
) -> Result<()> {
    for item in items {
        if let BookItem::Chapter(chapter) = item {
            build.chapter_id += 1;
            let path = chapter
                .source_path
                .as_deref()
                .or(chapter.path.as_deref())
                .unwrap_or(Path::new("<synthetic>"));
            let inclusions = fences::parse(&chapter.content)
                .with_context(|| format!("chapter {}", path.display()))?;
            if build.release_lock.is_some() {
                let file = ctx.root.join(&ctx.config.book.src).join(path);
                match file.canonicalize() {
                    Ok(path) => {
                        build.protected_chapters.insert(path);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("cannot resolve chapter {}", file.display()))
                    }
                }
            }
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
                let project = match &mut build.project {
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
                    let prefix = format!("memmap-{}-{}", build.chapter_id, index + 1);
                    let (fragment, source) = render_inclusion(
                        project,
                        &directory.join(&inclusion.fence.source),
                        &inclusion.fence,
                        &prefix,
                    )
                    .with_context(|| {
                        format!(
                            "{}:{}: memmap inclusion {}",
                            source.display(),
                            inclusion.line,
                            index + 1
                        )
                    })?;
                    if let Some(lock) = &build.release_lock {
                        let pin = lock
                            .pins()
                            .get(&source)
                            .context("rendered source must be pinned")?;
                        let mut selection = serde_json::json!({
                            "id": format!("chapter-{}-fence-{}", build.chapter_id, index + 1),
                            "source": pin.path, "source_roots": project.source_roots(),
                            "view": match inclusion.fence.view {
                                fences::View::Summary => "summary", fences::View::Registers => "registers",
                            },
                            "anchor_prefix": prefix, "output_sha256": release::sha256(fragment.as_bytes()),
                        });
                        if let Some(map) = &inclusion.fence.map {
                            selection["map"] = serde_json::json!(map);
                        }
                        if let Some(space) = &inclusion.fence.address_space {
                            selection["address_space"] = serde_json::json!(space);
                        }
                        build.selections.push(selection);
                    }
                    output.push_str(&chapter.content[end..inclusion.range.start]);
                    output.push('\n');
                    output.push_str(&fragment);
                    output.push('\n');
                    end = inclusion.range.end;
                }
                output.push_str(&chapter.content[end..]);
                chapter.content = output;
            }
            process_items(&mut chapter.sub_items, ctx, settings, build)?;
        }
    }
    Ok(())
}

fn render_inclusion(
    project: &mut project::Project,
    file: &Path,
    fence: &fences::Fence,
    prefix: &str,
) -> Result<(String, PathBuf)> {
    let definition = project.load(file)?;
    let fragment = match &definition.kind {
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
    }?;
    Ok((fragment, definition.origin.file.clone()))
}
