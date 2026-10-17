use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn invoke(arguments: &[&str], input: Option<Value>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdbook-memmap"))
        .args(arguments)
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
    assert!(errors.contains("not available before checkpoint 3"));
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
