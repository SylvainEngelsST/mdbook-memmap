use mdbook_memmap::{
    project::Project,
    release::{sha256, ReleaseLock},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        for folder in ["fixtures/ips", "fixtures/systems", "reference"] {
            fs::create_dir_all(fixture.root().join(folder)).unwrap();
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
            (
                "ips/rendering.yaml",
                include_str!("fixtures/ips/rendering.yaml"),
            ),
            (
                "systems/rendering.yaml",
                include_str!("fixtures/systems/rendering.yaml"),
            ),
        ] {
            fs::write(fixture.inputs().join(name), text).unwrap();
        }
        fs::write(fixture.lock(), include_str!("reference/release.lock.json")).unwrap();
        fixture
    }
    fn root(&self) -> &Path {
        self.dir.path()
    }
    fn inputs(&self) -> PathBuf {
        self.root().join("fixtures")
    }
    fn lock(&self) -> PathBuf {
        self.root().join("reference/release.lock.json")
    }
    fn value(&self) -> Value {
        serde_json::from_slice(&fs::read(self.lock()).unwrap()).unwrap()
    }
    fn write(&self, value: &Value) {
        fs::write(self.lock(), serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
    fn inventory(&self) -> anyhow::Result<Project> {
        let lock = Arc::new(ReleaseLock::load(&self.lock())?);
        let mut project = Project::with_release_lock(Arc::clone(&lock));
        lock.verify_inventory(&mut project)?;
        Ok(project)
    }
}

#[test]
fn hashing_matches_standard_sha256_vectors() {
    assert_eq!(
        sha256(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn independent_python_locks_are_portable_and_cover_complete_inventory() {
    for _ in 0..2 {
        let fixture = Fixture::new();
        let project = fixture.inventory().unwrap();
        assert_eq!(project.definitions().count(), 3);
        let lock = ReleaseLock::load(&fixture.lock()).unwrap();
        assert_eq!(lock.release(), "native-compat-1");
        assert_eq!(lock.digest(), sha256(&fs::read(fixture.lock()).unwrap()));
        assert!(lock
            .pins()
            .values()
            .all(|pin| !Path::new(&pin.path).is_absolute()));
    }
}

#[test]
fn modified_unpinned_and_identity_mismatched_sources_fail() {
    let fixture = Fixture::new();
    let ip = fixture.inputs().join("ips/timer.yaml");
    fs::write(&ip, b"not even valid authoring YAML").unwrap();
    let error = fixture.inventory().err().unwrap().to_string();
    assert!(error.contains("[release-pin]"), "{error}");
    let fixture = Fixture::new();
    let mut value = fixture.value();
    value["sources"]
        .as_array_mut()
        .unwrap()
        .retain(|pin| pin["vlnv"]["name"] != "timer");
    fixture.write(&value);
    assert!(format!("{:#}", fixture.inventory().err().unwrap()).contains("[release-pin]"));
    let fixture = Fixture::new();
    let mut value = fixture.value();
    value["sources"][0]["vlnv"]["version"] = json!("wrong");
    fixture.write(&value);
    assert!(format!("{:#}", fixture.inventory().err().unwrap()).contains("VLNV differs"));
}

#[test]
fn malformed_lock_shapes_and_duplicate_canonical_sources_fail() {
    let fixture = Fixture::new();
    let original = fixture.value();
    let mut mutations = vec![];
    for (key, replacement) in [
        ("format_version", json!(true)),
        ("format_version", json!(2)),
        ("format_version", json!("1")),
        ("release", json!(" ")),
        ("sources", json!([])),
        ("extra", json!(true)),
        ("entry", json!({"source":"/absolute","map":"csr"})),
        (
            "entry",
            json!({"source":"../fixtures/ips/timer.yaml","map":"csr","address_space":"cpu"}),
        ),
        (
            "entry",
            json!({"source":"../fixtures/ips/timer.yaml","map":null}),
        ),
        ("entry", json!({"source":"../fixtures/ips/timer.yaml"})),
        (
            "entry",
            json!({"source":"../fixtures/ips/timer.yaml","map":" "}),
        ),
    ] {
        let mut value = original.clone();
        value[key] = replacement;
        mutations.push(value);
    }
    for (key, replacement) in [
        ("path", json!("/absolute")),
        ("path", json!("")),
        ("sha256", json!("A".repeat(64))),
        ("sha256", json!("bad")),
        (
            "vlnv",
            json!({"vendor":"example.org","library":"ip","name":"timer","version":"1","extra":true}),
        ),
        (
            "vlnv",
            json!({"vendor":"","library":"ip","name":"timer","version":"1"}),
        ),
    ] {
        let mut value = original.clone();
        value["sources"][0][key] = replacement;
        mutations.push(value);
    }
    let mut duplicate = original.clone();
    let mut pin = duplicate["sources"][0].clone();
    pin["path"] = json!("../fixtures/ips/./timer.yaml");
    duplicate["sources"].as_array_mut().unwrap().push(pin);
    mutations.push(duplicate);
    for value in mutations {
        fixture.write(&value);
        assert!(ReleaseLock::load(&fixture.lock()).is_err(), "{value}");
    }
    for text in [
        format!(
            "{{\"release\":\"duplicate\",{}",
            &serde_json::to_string(&original).unwrap()[1..]
        ),
        serde_json::to_string(&original)
            .unwrap()
            .replace("\"sha256\":", "\"sha256\":\"duplicate\",\"sha256\":"),
        serde_json::to_string(&original).unwrap().replace(
            "\"address_space\":\"cpu\"",
            "\"address_space\":\"cpu\",\"address_space\":\"cpu\"",
        ),
    ] {
        fs::write(fixture.lock(), text).unwrap();
        assert!(ReleaseLock::load(&fixture.lock()).is_err());
    }
}

#[test]
fn entry_selectors_and_every_unused_pin_are_validated() {
    let fixture = Fixture::new();
    let original = fixture.value();
    for entry in [
        json!({"source":"../fixtures/ips/timer.yaml","map":"csr"}),
        json!({"source":"../fixtures/systems/mosaic_example.yaml","address_space":"cpu"}),
    ] {
        let mut value = original.clone();
        value["entry"] = entry;
        fixture.write(&value);
        fixture.inventory().unwrap();
    }
    for entry in [
        json!({"source":"../fixtures/ips/timer.yaml","map":"missing"}),
        json!({"source":"../fixtures/systems/mosaic_example.yaml","map":"csr"}),
        json!({"source":"../fixtures/ips/timer.yaml","address_space":"cpu"}),
        json!({"source":"../fixtures/systems/mosaic_example.yaml","address_space":"missing"}),
        json!({"source":"../fixtures/ips/rendering.yaml","map":"csr"}),
    ] {
        let mut value = original.clone();
        value["entry"] = entry;
        fixture.write(&value);
        assert!(fixture.inventory().is_err());
    }
    fs::write(fixture.lock(), include_str!("reference/combined.lock.json")).unwrap();
    assert_eq!(fixture.inventory().unwrap().definitions().count(), 5);
    fs::write(
        fixture.inputs().join("ips/rendering.yaml"),
        b"# changed unused pin\n",
    )
    .unwrap();
    assert!(format!("{:#}", fixture.inventory().err().unwrap()).contains("[release-pin]"));
}

#[test]
fn lock_and_source_changes_after_loading_are_detected() {
    let fixture = Fixture::new();
    let lock = ReleaseLock::load(&fixture.lock()).unwrap();
    fixture.inventory().unwrap();
    fs::write(fixture.lock(), b"changed").unwrap();
    assert!(lock
        .verify_unchanged()
        .unwrap_err()
        .to_string()
        .contains("lock changed"));
    let fixture = Fixture::new();
    let lock = ReleaseLock::load(&fixture.lock()).unwrap();
    let ip = fixture.inputs().join("ips/timer.yaml");
    fs::write(
        &ip,
        fs::read(&ip)
            .unwrap()
            .into_iter()
            .chain(*b"\n")
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(lock
        .verify_unchanged()
        .unwrap_err()
        .to_string()
        .contains("[release-pin]"));
}

#[test]
fn verification_binds_the_parsed_snapshot_not_just_current_disk_bytes() {
    let fixture = Fixture::new();
    let mut project = Project::new();
    let ip = fixture.inputs().join("ips/timer.yaml");
    project.load(&ip).unwrap();
    let mut changed = fs::read(&ip).unwrap();
    changed.extend_from_slice(b"\n# concurrent edit\n");
    fs::write(&ip, &changed).unwrap();
    let mut value = fixture.value();
    value["sources"][0]["sha256"] = json!(sha256(&changed));
    fixture.write(&value);
    let lock = ReleaseLock::load(&fixture.lock()).unwrap();
    lock.verify_unchanged().unwrap();
    assert!(lock
        .verify_snapshots(&project)
        .unwrap_err()
        .to_string()
        .contains("[release-pin]"));
}

#[test]
fn locked_projects_allow_only_pinned_files_without_directory_restrictions() {
    let fixture = Fixture::new();
    let lock = Arc::new(ReleaseLock::load(&fixture.lock()).unwrap());
    let mut project = Project::with_release_lock(Arc::clone(&lock));
    lock.verify_inventory(&mut project).unwrap();
    assert_eq!(project.definitions().count(), 3);
    assert!(format!(
        "{:#}",
        project
            .load(&fixture.inputs().join("ips/rendering.yaml"))
            .unwrap_err()
    )
    .contains("[release-pin]"));
}

#[test]
fn implementation_identity_covers_all_native_sources_and_dependency_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pending = vec![root.join("src")];
    let mut paths = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("build.rs"),
    ];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(path);
            }
        }
    }
    let hashes: std::collections::BTreeMap<_, _> = paths
        .iter()
        .map(|path| {
            (
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/"),
                sha256(&fs::read(path).unwrap()),
            )
        })
        .collect();
    assert_eq!(
        mdbook_memmap::provenance::generator_identity().unwrap()["implementation_sha256"],
        sha256(&serde_json::to_vec(&hashes).unwrap())
    );
}

#[cfg(unix)]
#[test]
fn canonical_pin_path_retargeting_is_detected_even_when_bytes_match() {
    let fixture = Fixture::new();
    let ip = fixture.inputs().join("ips/timer.yaml");
    let original = fixture.inputs().join("ips/original.yaml");
    let replacement = fixture.inputs().join("ips/replacement.yaml");
    fs::rename(&ip, &original).unwrap();
    fs::copy(&original, &replacement).unwrap();
    std::os::unix::fs::symlink(&original, &ip).unwrap();
    let lock = ReleaseLock::load(&fixture.lock()).unwrap();
    lock.verify_unchanged().unwrap();
    fs::remove_file(&ip).unwrap();
    std::os::unix::fs::symlink(&replacement, &ip).unwrap();
    assert!(lock
        .verify_unchanged()
        .unwrap_err()
        .to_string()
        .contains("source path changed"));
}
