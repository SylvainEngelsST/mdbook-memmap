use mdbook_memmap::{
    fences::View,
    loader::load_definition,
    markdown::{self, Source},
    project::Project,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Case {
    name: String,
    source: String,
    map: Option<String>,
    address_space: Option<String>,
    view: View,
    prefix: String,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn matches_markdown_regression_fixtures_byte_for_byte() {
    let root = fixtures();
    let mut project = Project::new();
    let cases: Vec<Case> = serde_json::from_str(include_str!("reference/cases.json")).unwrap();
    assert_eq!(cases.len(), 14);
    for case in cases {
        let file = root.join(&case.source);
        let output = if let Some(space) = case.address_space {
            let system = project.resolve(&file, &space).unwrap();
            markdown::render(Source::System(&system), None, case.view, &case.prefix).unwrap()
        } else {
            let ip = project.load(&file).unwrap();
            markdown::render(
                Source::Ip(&ip),
                case.map.as_deref(),
                case.view,
                &case.prefix,
            )
            .unwrap()
        };
        let reference = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/reference/{}.md", case.name)),
        )
        .unwrap();
        assert_eq!(output, reference, "reference case {}", case.name);
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
    }
}

#[test]
fn all_generated_links_resolve_and_anchors_are_unique() {
    let root = fixtures();
    let mut project = Project::new();
    let system = project
        .resolve(&root.join("systems/mosaic_example.yaml"), "cpu")
        .unwrap();
    let html =
        markdown::render(Source::System(&system), None, View::Registers, "memmap-1-1").unwrap();
    let anchors: Vec<_> = regex::Regex::new(r#"<a id="([^"]+)"></a>"#)
        .unwrap()
        .captures_iter(&html)
        .map(|capture| capture[1].to_owned())
        .collect();
    let unique: std::collections::BTreeSet<_> = anchors.iter().collect();
    assert_eq!(unique.len(), anchors.len());
    assert_eq!(anchors.len(), 10);
    for link in regex::Regex::new(r"\]\(#([^)]+)\)")
        .unwrap()
        .captures_iter(&html)
    {
        assert!(unique.contains(&link[1].to_string()), "{}", &link[1]);
    }
}

#[test]
fn detail_rejects_memory_reserved_and_missing_maps_and_bad_prefixes() {
    let ip = load_definition(&fixtures().join("ips/rendering.yaml")).unwrap();
    for map in [None, Some("ram"), Some("reserved")] {
        assert!(format!(
            "{:#}",
            markdown::render(Source::Ip(&ip), map, View::Registers, "").unwrap_err()
        )
        .contains("[markdown-view]"));
    }
    assert!(markdown::render(Source::Ip(&ip), Some("missing"), View::Summary, "").is_err());
    for prefix in ["-start", "Upper", "has space", "\"", "é"] {
        assert!(format!(
            "{:#}",
            markdown::render(Source::Ip(&ip), Some("csr"), View::Summary, prefix).unwrap_err()
        )
        .contains("[anchor-prefix]"));
    }
}

#[test]
fn absent_reset_types_are_not_invented_and_field_gaps_are_reserved() {
    let mut ip = load_definition(&fixtures().join("ips/rendering.yaml")).unwrap();
    if let mdbook_memmap::model::Kind::Ip { reset_types, maps } = &mut ip.kind {
        reset_types.clear();
        for map in maps {
            for register in &mut map.registers {
                for field in &mut register.fields {
                    field.resets.clear();
                }
            }
        }
    }
    let output = markdown::render(Source::Ip(&ip), Some("csr"), View::Registers, "").unwrap();
    assert!(!output.contains("Reset types:"));
    assert!(!output.contains("Reset HARD"));
    assert!(output
        .contains("| `7:4` | Reserved | unspecified | unspecified | unspecified | unspecified |"));
    assert!(output.contains("0x10"));
}

#[test]
fn impractically_large_reset_display_fails_explicitly() {
    let mut ip = load_definition(&fixtures().join("ips/timer.yaml")).unwrap();
    if let mdbook_memmap::model::Kind::Ip { maps, .. } = &mut ip.kind {
        maps[0].size_bytes = 16384u32.into();
        maps[0].registers.truncate(1);
        maps[0].registers[0].width_bits = 65536;
        maps[0].registers[0].fields[0].width_bits = 65536;
    }
    let boundary = markdown::render(Source::Ip(&ip), Some("csr"), View::Registers, "").unwrap();
    let displayed_bits = regex::Regex::new(r"`0b([?01]+)`").unwrap();
    let resets: Vec<_> = displayed_bits.captures_iter(&boundary).collect();
    assert_eq!(resets.len(), 2);
    assert!(resets.iter().all(|reset| reset[1].len() == 65536));
    if let mdbook_memmap::model::Kind::Ip { maps, .. } = &mut ip.kind {
        maps[0].registers[0].width_bits = 65544;
        maps[0].registers[0].fields[0].width_bits = 65537;
    }
    assert!(markdown::render(Source::Ip(&ip), Some("csr"), View::Summary, "").is_ok());
    assert!(format!(
        "{:#}",
        markdown::render(Source::Ip(&ip), Some("csr"), View::Registers, "").unwrap_err()
    )
    .contains("[markdown-width]"));
}
