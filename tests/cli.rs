use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn invoke(arguments: &[&str], input: Option<Value>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdbook-memmap"))
        .args(arguments)
        .env("PATH", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(&input).unwrap().as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}

fn input(content: &str) -> Value {
    json!([{
        "root": "/book",
        "config": {"book": {"title": "Test", "src": "src"}},
        "renderer": "html",
        "mdbook_version": "0.5.4"
    }, {
        "items": [{"Chapter": {
            "name": "Page", "content": content, "number": [1],
            "sub_items": [], "path": "page.md", "source_path": "page.md",
            "parent_names": []
        }}]
    }])
}

#[test]
fn help_version_and_renderer_queries() {
    for arg in ["--help", "--version"] {
        assert!(invoke(&[arg], None).status.success());
    }
    for (renderer, supported) in [("html", true), ("pdf", false)] {
        let output = invoke(&["supports", renderer], None);
        assert_eq!(output.status.success(), supported);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn protocol_preserves_books_without_active_fences() {
    let input = input("# Page\n\n````markdown\n```memmap\nliteral\n```\n````\n");
    let output = invoke(&[], Some(input.clone()));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        input[1]
    );
}

#[test]
fn active_fences_fail_without_partial_book_json() {
    let output = invoke(
        &[],
        Some(input(
            "```memmap\nsource: timer.yaml\nmap: csr\nview: registers\n```\n",
        )),
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("page.md:1"));
    assert!(errors.contains("source-roots is required"));
}

#[test]
fn malformed_input_and_incompatible_versions_fail() {
    for version in ["0.4.52", "0.6.0", "bad"] {
        let mut input = input("# Page");
        input[0]["mdbook_version"] = json!(version);
        let output = invoke(&[], Some(input));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = invoke(&[], Some(json!({"invalid": true})));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn binary_installer_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("book.toml");
    std::fs::write(&file, "[book]\ntitle = 'Example'\n").unwrap();
    let args = ["install", dir.path().to_str().unwrap()];
    assert!(invoke(&args, None).status.success());
    let first = std::fs::read(&file).unwrap();
    assert!(invoke(&args, None).status.success());
    assert_eq!(first, std::fs::read(&file).unwrap());
}

fn native_book(content: &str) -> (tempfile::TempDir, Value) {
    let dir = tempfile::tempdir().unwrap();
    for folder in ["src/nested", "maps/ips", "maps/systems"] {
        std::fs::create_dir_all(dir.path().join(folder)).unwrap();
    }
    for (name, text) in [
        ("ips/timer.yaml", include_str!("fixtures/ips/timer.yaml")),
        (
            "systems/timer_cluster.yaml",
            include_str!("fixtures/systems/timer_cluster.yaml"),
        ),
        (
            "systems/mosaic_example.yaml",
            include_str!("fixtures/systems/mosaic_example.yaml"),
        ),
    ] {
        std::fs::write(dir.path().join("maps").join(name), text).unwrap();
    }
    std::fs::write(dir.path().join("src/page.md"), content).unwrap();
    std::fs::write(dir.path().join("src/nested/page.md"), content).unwrap();
    let mut book = input(content);
    book[0]["root"] = json!(dir.path());
    book[0]["config"]["preprocessor"]["memmap"] = json!({
        "source-roots": ["maps"], "command": "mdbook-memmap",
        "renderers": ["html"], "before": ["links"], "after": [], "optional": false
    });
    (dir, book)
}

fn ip_fence(source: &str, view: &str) -> String {
    format!("```memmap\nsource: {source}\nmap: csr\nview: {view}\n```\n")
}

fn succeeded(book: Value) -> Value {
    let output = invoke(&[], Some(book));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn native_protocol_renders_without_python_or_backend_on_path() {
    let content = format!(
        "Pr\u{e9}face\n\n{}After.\n",
        ip_fence("../maps/ips/timer.yaml", "registers")
    );
    let (_dir, mut book) = native_book(&content);
    book[1]["items"][0]["Chapter"]["path"] = json!("different-output-name.md");
    let mut actual = succeeded(book.clone());
    let rendered = actual["items"][0]["Chapter"]["content"].as_str().unwrap();
    assert!(rendered.starts_with("Pr\u{e9}face\n\n"));
    assert!(rendered.ends_with("After.\n"));
    assert!(!rendered.contains("```memmap"));
    assert!(rendered.contains(
        include_str!("reference/timer-registers-prefixed.md")
            .replace("memmap-2-2", "memmap-1-1")
            .as_str()
    ));
    actual["items"][0]["Chapter"]["content"] = json!(content);
    assert_eq!(
        actual, book[1],
        "all chapter/book metadata must be preserved"
    );
}

#[test]
fn nested_chapters_and_repeated_views_have_stable_distinct_namespaces() {
    let literal = "````markdown\n```memmap\nliteral\n```\n````\n";
    let (_dir, mut book) = native_book(literal);
    let mut child = book[1]["items"][0].clone();
    child["Chapter"]["path"] = json!("nested/page.md");
    child["Chapter"]["source_path"] = json!("nested/page.md");
    child["Chapter"]["parent_names"] = json!(["Page"]);
    let content = format!(
        "{}\n{}\n~~~memmap\nsource: ../../maps/systems/mosaic_example.yaml\naddress_space: cpu\nview: registers\n~~~\n",
        ip_fence("../../maps/ips/timer.yaml", "summary"),
        ip_fence("../../maps/ips/timer.yaml", "registers")
    );
    child["Chapter"]["content"] = json!(content);
    book[1]["items"][0]["Chapter"]["sub_items"] = json!([child]);
    let output = succeeded(book);
    assert_eq!(output["items"][0]["Chapter"]["content"], literal);
    let output = output["items"][0]["Chapter"]["sub_items"][0]["Chapter"]["content"]
        .as_str()
        .unwrap();
    for number in 1..=3 {
        assert!(output.contains(&format!("10-memmap-2-{number}")));
    }
    assert!(output.contains("0x40006000"));
    assert!(output.contains("0x40005000"));
    let ids: Vec<_> = regex::Regex::new(r#"<a id="([^"]+)">"#)
        .unwrap()
        .captures_iter(output)
        .map(|capture| capture[1].to_owned())
        .collect();
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len()
    );
}

#[test]
fn strict_native_configuration_rejects_obsolete_unsafe_and_unknown_settings() {
    let (_dir, book) = native_book(&ip_fence("../maps/ips/timer.yaml", "summary"));
    for (key, value, expected) in [
        ("backend", json!("python"), "obsolete setting"),
        ("backend-args", json!([]), "obsolete setting"),
        ("backend-timeout-seconds", json!(60), "obsolete setting"),
        ("optional", json!(true), "optional=true"),
        ("source-roots", json!([]), "at least one"),
        ("source-roots", json!("maps"), "invalid preprocessor.memmap"),
        ("source-roots", json!(["missing"]), "invalid source roots"),
        ("release-lock", json!("lock.json"), "checkpoint 4"),
        ("provenance", json!("provenance.json"), "checkpoint 4"),
        ("typo", json!(true), "unknown field"),
    ] {
        let mut input = book.clone();
        input[0]["config"]["preprocessor"]["memmap"][key] = value;
        let output = invoke(&[], Some(input));
        assert!(!output.status.success(), "{key}");
        assert!(output.stdout.is_empty(), "{key}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{key}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn any_failed_inclusion_aborts_without_partial_json() {
    let (dir, book) = native_book(&ip_fence("../maps/ips/timer.yaml", "summary"));
    std::fs::write(
        dir.path().join("outside.yaml"),
        include_str!("fixtures/ips/timer.yaml"),
    )
    .unwrap();
    for fence in [
        ip_fence("../maps/ips/missing.yaml", "summary"),
        ip_fence("../outside.yaml", "summary"),
        ip_fence("../maps/ips/timer.yaml", "summary").replace("map: csr", "map: missing"),
        ip_fence("../maps/ips/timer.yaml", "summary").replace("map: csr", "address_space: cpu"),
        ip_fence("../maps/systems/mosaic_example.yaml", "summary"),
        "```memmap\nsource: ../maps/systems/mosaic_example.yaml\naddress_space: missing\nview: registers\n```\n".into(),
        "```memmap\nsource: ../maps/ips/timer.yaml\nmap: csr\nview: summary\nunexpected: true\n```\n".into(),
    ] {
        let mut input = book.clone();
        input[1]["items"][0]["Chapter"]["content"] = json!(format!(
            "{}\n{fence}", ip_fence("../maps/ips/timer.yaml", "summary")
        ));
        let output = invoke(&[], Some(input));
        assert!(!output.status.success(), "{fence}");
        assert!(output.stdout.is_empty(), "{fence}");
        assert!(!output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("page.md"));
    }

    let mut input = book.clone();
    input[0]["renderer"] = json!("pdf");
    let output = invoke(&[], Some(input));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn chapter_path_fallback_works_but_synthetic_or_missing_sources_fail() {
    let (_dir, mut book) = native_book(&ip_fence("../maps/ips/timer.yaml", "summary"));
    book[1]["items"][0]["Chapter"]["source_path"] = Value::Null;
    assert!(succeeded(book.clone())["items"][0]["Chapter"]["content"]
        .as_str()
        .unwrap()
        .contains("## IP Memory Maps"));
    for path in [Value::Null, json!("missing.md")] {
        book[1]["items"][0]["Chapter"]["path"] = path;
        let output = invoke(&[], Some(book.clone()));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn chapter_symlinks_use_the_actual_source_directory() {
    let content = ip_fence("../../maps/ips/timer.yaml", "summary");
    let (dir, book) = native_book(&content);
    std::fs::create_dir_all(dir.path().join("physical/content")).unwrap();
    std::fs::rename(
        dir.path().join("src/page.md"),
        dir.path().join("physical/content/page.md"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        "../physical/content/page.md",
        dir.path().join("src/page.md"),
    )
    .unwrap();
    assert!(succeeded(book)["items"][0]["Chapter"]["content"]
        .as_str()
        .unwrap()
        .contains("## IP Memory Maps"));
}
