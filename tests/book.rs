use std::path::Path;
use std::process::Command;

/// Run explicitly with MDBOOK pointing to mdBook 0.5.4.
#[test]
#[ignore = "requires the external mdBook 0.5.4 executable; run with --ignored"]
fn real_book_has_complete_tables_links_and_reproducible_html_without_python() {
    let mdbook = std::env::var_os("MDBOOK").expect("set MDBOOK to the mdBook 0.5.4 executable");
    let version = Command::new(&mdbook).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "mdbook v0.5.4"
    );
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_mdbook-memmap"))
        .parent()
        .unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut pages = Vec::new();
    for build in ["first", "second"] {
        let destination = output.path().join(build);
        let result = Command::new(&mdbook)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["build", "examples/book", "--dest-dir"])
            .arg(&destination)
            // Neither Python nor a legacy backend can be found on this PATH.
            .env("PATH", binary_dir)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&result.stderr).contains("WARN"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let literal = std::fs::read_to_string(destination.join("contract.html")).unwrap();
        assert!(literal.contains("language-markdown"));
        assert!(literal.contains("source: ../maps/ips/timer.yaml"));
        pages.push(std::fs::read_to_string(destination.join("memory-maps.html")).unwrap());
    }
    assert_html(&pages);
}

#[test]
#[ignore = "requires the external mdBook 0.5.4 executable; run with --ignored"]
fn clean_locked_books_bind_exact_reference_fragments_and_reject_modified_inventory() {
    let mdbook = std::env::var_os("MDBOOK").expect("set MDBOOK to the mdBook 0.5.4 executable");
    let binary_dir = Path::new(env!("CARGO_BIN_EXE_mdbook-memmap"))
        .parent()
        .unwrap();
    let workspace = tempfile::tempdir().unwrap();
    for relative in [
        "examples/book/src/SUMMARY.md",
        "examples/book/src/contract.md",
        "examples/book/src/memory-maps.md",
        "examples/locked-book/book.toml",
        "tests/fixtures/ips/timer.yaml",
        "tests/fixtures/ips/rendering.yaml",
        "tests/fixtures/systems/timer_cluster.yaml",
        "tests/fixtures/systems/mosaic_example.yaml",
        "tests/fixtures/systems/rendering.yaml",
        "tests/reference/combined.lock.json",
    ] {
        let destination = workspace.path().join(relative);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(relative),
            destination,
        )
        .unwrap();
    }
    let book = workspace.path().join("examples/locked-book");
    let provenance = book.join("memmap-provenance.json");
    let run = |destination: &Path| {
        Command::new(&mdbook)
            .args(["build"])
            .arg(&book)
            .arg("--dest-dir")
            .arg(destination)
            .env("PATH", binary_dir)
            .output()
            .unwrap()
    };
    let mut artifacts = Vec::new();
    for name in ["clean-first", "clean-second"] {
        let output = workspace.path().join(name);
        let result = run(&output);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!String::from_utf8_lossy(&result.stderr).contains("WARN"));
        artifacts.push((
            std::fs::read(output.join("memory-maps.html")).unwrap(),
            std::fs::read(&provenance).unwrap(),
        ));
    }
    assert_eq!(
        artifacts[0], artifacts[1],
        "HTML and provenance bytes must be reproducible"
    );
    let report: serde_json::Value = serde_json::from_slice(&artifacts[0].1).unwrap();
    assert_eq!(report["format_version"], 2);
    assert_eq!(report["inputs"]["sources"].as_array().unwrap().len(), 5);
    let selections = report["inputs"]["selections"].as_array().unwrap();
    assert_eq!(selections.len(), 4);
    let references = [
        include_str!("reference/system-summary-prefixed.md").replace("memmap-1-1", "memmap-2-1"),
        include_str!("reference/timer-registers-prefixed.md").to_owned(),
        include_str!("reference/system-registers-prefixed.md").replace("memmap-1-2", "memmap-2-3"),
        include_str!("reference/rich-registers.md").replace("memmap-3-1", "memmap-2-4"),
    ];
    for (index, reference) in references.iter().enumerate() {
        assert_eq!(
            selections[index]["id"],
            format!("chapter-2-fence-{}", index + 1)
        );
        assert_eq!(
            selections[index]["output_sha256"],
            mdbook_memmap::release::sha256(reference.as_bytes())
        );
    }
    // This pinned system is not displayed, but still belongs to the release.
    std::fs::write(
        workspace
            .path()
            .join("tests/fixtures/systems/rendering.yaml"),
        b"# changed unused pin\n",
    )
    .unwrap();
    let failed_output = workspace.path().join("modified-input");
    let failed = run(&failed_output);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("[release-pin]"));
    assert!(!failed_output.join("memory-maps.html").exists());
    assert_eq!(std::fs::read(&provenance).unwrap(), artifacts[0].1);
}

fn assert_html(pages: &[String]) {
    assert_eq!(
        pages[0], pages[1],
        "two independent book builds must produce identical page bytes"
    );
    let page = &pages[0];
    assert!(!page.contains("language-memmap"));
    for address in ["0x40006000", "0x40006004", "0x40005000", "0x40005004"] {
        assert!(page.contains(address), "{address}");
    }
    assert!(page.contains("0b?01"));
    assert!(page.contains("0b???"));
    assert!(page.contains("0x80000000000000000001"));
    assert!(page.contains("write: zeroToToggle; read: clear"));
    assert!(page.contains("A | B &amp; &lt;em&gt;tag&lt;/em&gt; `code`<br>next line"));
    assert!(page.contains("<em>tag</em>"));
    for table in regex::Regex::new(r"(?s)<table>.*?</table>")
        .unwrap()
        .find_iter(page)
    {
        assert!(!table.as_str().contains("<em>tag</em>"));
    }
    assert_eq!(page.matches("<table>").count(), 18);
    let anchors: Vec<_> = regex::Regex::new(r#"id="(mm-[^"]+)""#)
        .unwrap()
        .captures_iter(page)
        .map(|capture| capture[1].to_owned())
        .collect();
    let unique: std::collections::BTreeSet<_> = anchors.iter().collect();
    assert_eq!(anchors.len(), 23);
    assert_eq!(anchors.len(), unique.len());
    for link in regex::Regex::new(r##"href="#(mm-[^"]+)""##)
        .unwrap()
        .captures_iter(page)
    {
        assert!(
            unique.contains(&link[1].to_owned()),
            "broken link {}",
            &link[1]
        );
    }
}
