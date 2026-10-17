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
