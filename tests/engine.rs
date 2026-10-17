use mdbook_memmap::{loader::load_definition, model::*, project::Project};
use num_bigint::BigUint;
use serde_yaml_ng::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const TIMER: &str = include_str!("fixtures/ips/timer.yaml");
const CLUSTER: &str = include_str!("fixtures/systems/timer_cluster.yaml");
const SYSTEM: &str = include_str!("fixtures/systems/mosaic_example.yaml");

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir(fixture.root().join("ips")).unwrap();
        fs::create_dir(fixture.root().join("systems")).unwrap();
        fs::write(fixture.ip(), TIMER).unwrap();
        fs::write(fixture.cluster(), CLUSTER).unwrap();
        fs::write(fixture.system(), SYSTEM).unwrap();
        fixture
    }
    fn root(&self) -> &Path {
        self.dir.path()
    }
    fn ip(&self) -> PathBuf {
        self.root().join("ips/timer.yaml")
    }
    fn cluster(&self) -> PathBuf {
        self.root().join("systems/timer_cluster.yaml")
    }
    fn system(&self) -> PathBuf {
        self.root().join("systems/mosaic_example.yaml")
    }
    fn project(&self) -> Project {
        Project::new(&[self.root().into()]).unwrap()
    }
    fn write(&self, path: &Path, text: &str, mutate: impl FnOnce(&mut Value)) {
        let mut value = serde_yaml_ng::from_str(text).unwrap();
        mutate(&mut value);
        fs::write(path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    }
}

fn error<T: std::fmt::Debug>(action: impl FnOnce() -> anyhow::Result<T>, rule: &str) {
    let error = format!("{:#}", action().unwrap_err());
    assert!(
        error.contains(&format!("[{rule}]")),
        "expected {rule}: {error}"
    );
}

#[test]
fn nested_example_matches_all_python_reference_addresses_and_reuses_ip() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let view = project.resolve(&fixture.system(), "cpu").unwrap();
    assert_eq!(project.definitions().count(), 3);
    assert_eq!(view.blocks.len(), 2);
    assert!(Arc::ptr_eq(
        &view.blocks[0].definition,
        &view.blocks[1].definition
    ));
    let addresses: Vec<_> = view
        .blocks
        .iter()
        .flat_map(|block| {
            block
                .memory_map
                .registers
                .iter()
                .map(|register| &block.base + &register.offset)
        })
        .collect();
    assert_eq!(
        addresses,
        [0x40005000u64, 0x40005004, 0x40006000, 0x40006004].map(BigUint::from)
    );
    let field = &view.blocks[0].memory_map.registers[1].fields[0];
    assert_eq!(field.modified_write_value, Some(WriteEffect::OneToClear));
    assert!(field.volatile);
    assert_eq!(field.hw_access, HardwareAccess::ReadWrite);
    assert_eq!(
        field
            .resets
            .iter()
            .map(|reset| reset.reset_type.as_str())
            .collect::<Vec<_>>(),
        ["HARD", "SOFT"]
    );
}

#[test]
fn scalar_contract_is_lexical_and_preserves_yaml12_prose() {
    let fixture = Fixture::new();
    for zero in ["-0", "-0x0", "+0x0"] {
        fs::write(
            fixture.ip(),
            TIMER.replace("offset: 0x000", &format!("offset: {zero}")),
        )
        .unwrap();
        load_definition(&fixture.ip()).unwrap();
    }
    for text in [
        "on",
        "OFF",
        "2026-10-16",
        "0b11",
        "0o10",
        "1_000",
        "inf",
        "NaN",
        "1e2e3",
    ] {
        let input = TIMER.replace("General-purpose timer.", text);
        fs::write(fixture.ip(), input).unwrap();
        assert!(load_definition(&fixture.ip())
            .unwrap()
            .description
            .unwrap()
            .contains(text));
        let input = TIMER.replace("vendor: example.org", &format!("vendor: {text}"));
        fs::write(fixture.ip(), input).unwrap();
        load_definition(&fixture.ip()).unwrap();
    }
    for scalar in [
        "true", "32.0", "'32'", "null", "0b100000", "0o40", "3_2", "-32", "+32.0", ".NAN",
    ] {
        fs::write(
            fixture.ip(),
            TIMER.replace("data_width_bits: 32", &format!("data_width_bits: {scalar}")),
        )
        .unwrap();
        error(|| load_definition(&fixture.ip()), "schema");
    }
    fs::write(
        fixture.ip(),
        TIMER.replace("offset: 0x000", "offset: 00012"),
    )
    .unwrap();
    let definition = load_definition(&fixture.ip()).unwrap();
    let Kind::Ip { maps, .. } = definition.kind else {
        panic!()
    };
    assert_eq!(maps[0].registers[0].offset, BigUint::from(12u8));
}

#[test]
fn forbidden_yaml_features_fail_with_source_locations() {
    let fixture = Fixture::new();
    for (text, rule) in [
        ("a: 1\na: 2", "yaml-duplicate"),
        ("a: {x: 1, x: 2}", "yaml-duplicate"),
        ("1: x", "yaml-key"),
        ("? [a, b]\n: value", "yaml-key"),
        ("a: {<<: {x: 1}}", "yaml-merge"),
        ("a: &shared 1", "yaml-feature"),
        ("a: *shared", "yaml-feature"),
        ("a: !!str 1", "yaml-feature"),
        ("a: !custom 1", "yaml-feature"),
        ("%YAML 1.1\n---\na: 1", "yaml-version"),
        ("%TAG !x! tag:example.org,2026:\n---\na: 1", "yaml-version"),
        ("a: 1\n---\nb: 2", "yaml-syntax"),
        ("a: [1", "yaml-syntax"),
        ("", "yaml-empty"),
    ] {
        fs::write(fixture.ip(), text).unwrap();
        error(|| load_definition(&fixture.ip()), rule);
    }
    fs::write(fixture.ip(), "root:\n  offset: 0\n  offset: 4\n").unwrap();
    let message = format!("{:#}", load_definition(&fixture.ip()).unwrap_err());
    assert!(message.contains(":3:3"));
    assert!(message.contains("$.root.offset"));
}

#[test]
fn yaml12_directive_is_accepted_and_nesting_is_bounded() {
    let fixture = Fixture::new();
    fs::write(fixture.ip(), format!("%YAML 1.2\n---\n{TIMER}")).unwrap();
    load_definition(&fixture.ip()).unwrap();
    fs::write(
        fixture.ip(),
        format!("{}0{}", "[".repeat(200), "]".repeat(200)),
    )
    .unwrap();
    error(|| load_definition(&fixture.ip()), "yaml-depth");
}

#[test]
fn naming_checks_reject_bad_forms_in_each_scope() {
    let fixture = Fixture::new();
    for name in ["ctrl", "_CTRL", "CTRL_", "C__TRL", "1CTRL", "CTR-L", "ÉTAT"] {
        fs::write(
            fixture.ip(),
            TIMER.replace("name: CTRL", &format!("name: {name}")),
        )
        .unwrap();
        error(|| load_definition(&fixture.ip()), "schema");
    }
    for name in ["CSR", "_csr", "csr_", "c__sr", "1csr", "cs-r"] {
        fs::write(
            fixture.ip(),
            TIMER.replace("name: csr", &format!("name: {name}")),
        )
        .unwrap();
        error(|| load_definition(&fixture.ip()), "schema");
    }
    fs::write(fixture.ip(), TIMER.replace("name: ENABLE", "name: enable")).unwrap();
    let message = format!("{:#}", load_definition(&fixture.ip()).unwrap_err());
    assert!(message.contains("$.maps[0].registers[0].fields[0].name"));
}

#[test]
fn unknown_properties_and_explicit_nulls_fail() {
    let fixture = Fixture::new();
    for (path, value) in [
        (vec!["extra"], Value::Bool(true)),
        (vec!["description"], Value::Null),
        (vec!["reset_types"], Value::Sequence(vec![])),
        (vec!["maps"], Value::Sequence(vec![])),
    ] {
        fixture.write(&fixture.ip(), TIMER, |root| root[path[0]] = value);
        error(|| load_definition(&fixture.ip()), "schema");
    }
    fixture.write(&fixture.ip(), TIMER, |root| {
        root["vlnv"]["version"] = Value::Number(1.into())
    });
    error(|| load_definition(&fixture.ip()), "schema");
}

#[test]
fn usage_defaults_and_memory_reserved_rules() {
    let fixture = Fixture::new();
    fixture.write(&fixture.ip(), TIMER, |root| {
        root["maps"][0]
            .as_mapping_mut()
            .unwrap()
            .remove(Value::String("usage".into()));
    });
    let Kind::Ip { maps, .. } = load_definition(&fixture.ip()).unwrap().kind else {
        panic!()
    };
    assert_eq!(maps[0].usage, Usage::Register);
    assert!(maps[0].access.is_none());
    for usage in ["memory", "reserved"] {
        fixture.write(&fixture.ip(), TIMER, |root| {
            let map = root["maps"][0].as_mapping_mut().unwrap();
            map.remove(Value::String("registers".into()));
            map.insert("usage".into(), usage.into());
            if usage == "memory" {
                map.insert("access".into(), "read-write".into());
            }
        });
        let Kind::Ip { maps, .. } = load_definition(&fixture.ip()).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            maps[0].access,
            Some(if usage == "memory" {
                Access::ReadWrite
            } else {
                Access::NoAccess
            })
        );
    }
}

#[test]
fn duplicate_names_bounds_and_register_alignment() {
    let fixture = Fixture::new();
    for (from, to, rule) in [
        ("name: STATUS", "name: CTRL", "duplicate-name"),
        ("offset: 0x004", "offset: 0x002", "register-alignment"),
        ("offset: 0x004", "offset: 0x000", "overlap"),
        ("offset: 0x004", "offset: 4096", "register-bounds"),
        ("lsb: 0", "lsb: 32", "field-bounds"),
        ("width_bits: 1", "width_bits: 33", "field-bounds"),
        ("width_bits: 32", "width_bits: 31", "schema"),
    ] {
        fs::write(fixture.ip(), TIMER.replace(from, to)).unwrap();
        error(|| load_definition(&fixture.ip()), rule);
    }
    fixture.write(&fixture.ip(), TIMER, |root| {
        let field = root["maps"][0]["registers"][0]["fields"][0].clone();
        root["maps"][0]["registers"][0]["fields"]
            .as_sequence_mut()
            .unwrap()
            .push(field);
    });
    error(|| load_definition(&fixture.ip()), "duplicate-name");
}

#[test]
fn reset_references_duplicates_and_masked_bits() {
    let fixture = Fixture::new();
    for (from, to, rule) in [
        ("resetType: HARD", "resetType: MISSING", "reset-reference"),
        ("resetType: SOFT", "resetType: HARD", "reset-duplicate"),
        ("mask: 1", "mask: 2", "reset-width"),
        ("value: 0", "value: 2", "reset-width"),
    ] {
        fs::write(fixture.ip(), TIMER.replace(from, to)).unwrap();
        error(|| load_definition(&fixture.ip()), rule);
    }
    fs::write(
        fixture.ip(),
        TIMER
            .replace("mask: 1", "mask: 0")
            .replace("value: 0", "value: 1"),
    )
    .unwrap();
    let Kind::Ip { maps, .. } = load_definition(&fixture.ip()).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        maps[0].registers[0].fields[0].resets[0].value,
        BigUint::from(1u8)
    );
    assert_eq!(
        maps[0].registers[0].fields[0].resets[0].mask,
        BigUint::from(0u8)
    );
}

#[test]
fn hardware_access_and_software_side_effect_rules() {
    let fixture = Fixture::new();
    for (from, to) in [
        ("volatile: true", "volatile: false"),
        ("hwAccess: read-only", "hwAccess: writeOnce"),
        ("access: read-write", "access: read-only"),
        (
            "modified_write_value: oneToClear",
            "modified_write_value: invalid",
        ),
    ] {
        fs::write(fixture.ip(), TIMER.replace(from, to)).unwrap();
        error(|| load_definition(&fixture.ip()), "schema");
    }
    fixture.write(&fixture.ip(), TIMER, |root| {
        root["maps"][0]["registers"][0]["fields"][0]["access"] = "write-only".into();
        root["maps"][0]["registers"][0]["fields"][0]["read_action"] = "clear".into();
    });
    error(|| load_definition(&fixture.ip()), "schema");
}

#[test]
fn arbitrary_precision_resets_and_addresses_are_not_truncated() {
    let fixture = Fixture::new();
    let huge = "0x1000000000000000000000000";
    let text = TIMER
        .replace("size_bytes: 4096", &format!("size_bytes: {huge}"))
        .replace("width_bits: 32", "width_bits: 128")
        .replace("offset: 0x004", "offset: 0x010")
        .replace("width_bits: 1\n", "width_bits: 128\n")
        .replace("mask: 1", "mask: 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF");
    fs::write(fixture.ip(), text).unwrap();
    let Kind::Ip { maps, .. } = load_definition(&fixture.ip()).unwrap().kind else {
        panic!()
    };
    assert!(maps[0].size_bytes.bits() > 64);
    assert_eq!(maps[0].registers[0].fields[0].resets[0].mask.bits(), 128);
}

#[test]
fn system_allocation_checks_and_duplicate_targets() {
    let fixture = Fixture::new();
    for (from, to, rule) in [
        ("name: TIMER1", "name: TIMER0", "duplicate-name"),
        ("base: 0x6000", "base: 0x5000", "overlap"),
        ("base: 0x6000", "base: 0x10000", "mapping-bounds"),
    ] {
        fs::write(fixture.cluster(), CLUSTER.replace(from, to)).unwrap();
        error(|| fixture.project().resolve(&fixture.system(), "cpu"), rule);
    }
    fixture.write(&fixture.cluster(), CLUSTER, |root| {
        let mapping = root["address_spaces"][0]["instances"][0]["mappings"][0].clone();
        root["address_spaces"][0]["instances"][0]["mappings"]
            .as_sequence_mut()
            .unwrap()
            .push(mapping);
    });
    error(
        || fixture.project().resolve(&fixture.system(), "cpu"),
        "mapping-duplicate",
    );
}

#[test]
fn validates_targets_ranges_and_unselected_spaces() {
    let fixture = Fixture::new();
    for (from, to, rule) in [
        ("target: csr", "target: absent", "mapping-target"),
        ("size_bytes: 4096", "size_bytes: 2048", "target-bounds"),
    ] {
        fs::write(fixture.cluster(), CLUSTER.replace(from, to)).unwrap();
        error(|| fixture.project().resolve(&fixture.system(), "cpu"), rule);
    }
    fixture.write(&fixture.system(), SYSTEM, |root| {
        let mut other = root["address_spaces"][0].clone();
        other["name"] = "other".into();
        other["instances"][0]["mappings"][0]["target"] = "absent".into();
        root["address_spaces"]
            .as_sequence_mut()
            .unwrap()
            .push(other);
    });
    fs::write(fixture.cluster(), CLUSTER).unwrap();
    error(
        || fixture.project().resolve(&fixture.system(), "cpu"),
        "mapping-target",
    );
}

#[test]
fn cycles_missing_files_and_vlnv_conflicts_are_rejected() {
    let fixture = Fixture::new();
    fs::write(
        fixture.cluster(),
        CLUSTER.replace("../ips/timer.yaml", "../systems/mosaic_example.yaml"),
    )
    .unwrap();
    error(
        || fixture.project().load(&fixture.system()),
        "hierarchy-cycle",
    );
    fs::write(
        fixture.cluster(),
        CLUSTER.replace("../ips/timer.yaml", "../ips/missing.yaml"),
    )
    .unwrap();
    error(|| fixture.project().load(&fixture.system()), "source-read");
    fs::write(
        fixture.cluster(),
        CLUSTER.replacen("../ips/timer.yaml", "../ips/copy.yaml", 1),
    )
    .unwrap();
    fs::write(fixture.root().join("ips/copy.yaml"), TIMER).unwrap();
    error(
        || fixture.project().load(&fixture.system()),
        "vlnv-conflict",
    );
}

#[test]
fn snapshots_reuse_definitions_until_new_project_and_clear_after_errors() {
    let fixture = Fixture::new();
    let mut project = fixture.project();
    let first = project.load(&fixture.ip()).unwrap();
    fs::write(fixture.ip(), TIMER.replace("name: CTRL", "name: CONTROL")).unwrap();
    assert!(Arc::ptr_eq(&first, &project.load(&fixture.ip()).unwrap()));
    let fresh = fixture.project().load(&fixture.ip()).unwrap();
    assert!(!Arc::ptr_eq(&first, &fresh));
    assert!(project.load(&fixture.root().join("missing.yaml")).is_err());
    assert_eq!(project.definitions().count(), 0);
    assert!(project.snapshots().is_empty());
}

#[test]
fn source_roots_and_view_kind_are_enforced() {
    let fixture = Fixture::new();
    assert!(Project::new(&[]).is_err());
    assert!(Project::new(&[fixture.ip()]).is_err());
    let mut project = Project::new(&[fixture.root().join("systems")]).unwrap();
    error(|| project.load(&fixture.system()), "source-root");
    error(
        || fixture.project().resolve(&fixture.ip(), "cpu"),
        "view-kind",
    );
    error(
        || fixture.project().resolve(&fixture.system(), "absent"),
        "view-name",
    );
    fs::write(fixture.ip(), [0xff]).unwrap();
    error(|| fixture.project().load(&fixture.ip()), "source-read");
}

#[cfg(unix)]
#[test]
fn canonical_roots_reject_symlink_escape_and_accept_reuse() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("external.yaml");
    fs::write(&external, TIMER).unwrap();
    let link = fixture.root().join("link.yaml");
    symlink(&external, &link).unwrap();
    error(|| fixture.project().load(&link), "source-root");
    let mut project = Project::new(&[fixture.root().into(), outside.path().into()]).unwrap();
    let first = project.load(&link).unwrap();
    assert!(Arc::ptr_eq(&first, &project.load(&external).unwrap()));
}

#[test]
fn resolved_alignment_is_checked_after_parent_placement() {
    let fixture = Fixture::new();
    fixture.write(&fixture.system(), SYSTEM, |root| {
        root["address_spaces"][0]["instances"][0]["mappings"][0]["base"] = 1.into();
    });
    error(
        || fixture.project().resolve(&fixture.system(), "cpu"),
        "resolved-alignment",
    );
}

#[test]
fn full_allocation_tails_are_reserved_not_extra_storage() {
    let fixture = Fixture::new();
    fixture.write(&fixture.cluster(), CLUSTER, |root| {
        root["address_spaces"][0]["instances"][0]["mappings"][0]["size_bytes"] = 8192.into();
    });
    error(
        || fixture.project().resolve(&fixture.system(), "cpu"),
        "overlap",
    );
    fixture.write(&fixture.cluster(), CLUSTER, |root| {
        root["address_spaces"][0]["instances"][0]["mappings"][0]["size_bytes"] = 6144.into();
        root["address_spaces"][0]["instances"][1]["mappings"][0]["base"] = 28672.into();
    });
    let view = fixture.project().resolve(&fixture.system(), "cpu").unwrap();
    assert_eq!(view.blocks[0].mapping.size_bytes, BigUint::from(6144u16));
    assert_eq!(view.blocks[0].memory_map.size_bytes, BigUint::from(4096u16));
}

fn subsystem(name: &str, instance: &str, source: &str) -> String {
    format!(
        "schema_version: 1
naming_profile: rds-v1
kind: system
vlnv: {{vendor: example.org, library: tests, name: {name}, version: '1.0'}}
address_spaces:
  - name: apb
    address_width_bits: 32
    data_width_bits: 32
    size_bytes: 4096
    instances:
      - name: {instance}
        source: {source}
        mappings:
          - {{target: csr, base: 0, size_bytes: 4096}}
"
    )
}

#[test]
fn flattened_instance_path_collisions_fail() {
    let fixture = Fixture::new();
    let mut left: Value =
        serde_yaml_ng::from_str(&subsystem("left", "B", "../ips/timer.yaml")).unwrap();
    left["address_spaces"][0]["instances"][0]["name"] = "B_C".into();
    fs::write(
        fixture.root().join("systems/left.yaml"),
        serde_yaml_ng::to_string(&left).unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.root().join("systems/right.yaml"),
        subsystem("right", "C", "../ips/timer.yaml"),
    )
    .unwrap();
    fixture.write(&fixture.cluster(), CLUSTER, |root| {
        let instances = root["address_spaces"][0]["instances"]
            .as_sequence_mut()
            .unwrap();
        instances[0]["name"] = "A".into();
        instances[0]["source"] = "left.yaml".into();
        instances[0]["mappings"][0]["target"] = "apb".into();
        instances[1]["name"] = "A_B".into();
        instances[1]["source"] = "right.yaml".into();
        instances[1]["mappings"][0]["target"] = "apb".into();
    });
    error(
        || fixture.project().resolve(&fixture.system(), "cpu"),
        "flattened-name",
    );
}

#[test]
fn hierarchy_depth_is_bounded_without_stack_overflow() {
    let fixture = Fixture::new();
    for index in 0..130 {
        let text = subsystem(
            &format!("level{index}"),
            "CHILD",
            &format!("level{}.yaml", index + 1),
        )
        .replace("target: csr", "target: apb");
        fs::write(
            fixture.root().join(format!("systems/level{index}.yaml")),
            text,
        )
        .unwrap();
    }

    error(
        || {
            fixture
                .project()
                .load(&fixture.root().join("systems/level0.yaml"))
        },
        "hierarchy-depth",
    );
}

fn remove_property(root: &mut Value, path: &str) {
    let mut parts: Vec<_> = path.split('.').collect();
    let property = parts.pop().unwrap();
    let mut node = root;
    for part in parts {
        node = if let Ok(index) = part.parse::<usize>() {
            &mut node[index]
        } else {
            &mut node[part]
        };
    }
    node.as_mapping_mut()
        .unwrap()
        .remove(Value::String(property.into()));
}

#[test]
fn every_required_ip_and_system_property_is_enforced() {
    let fixture = Fixture::new();
    for path in [
        "schema_version",
        "naming_profile",
        "kind",
        "vlnv",
        "maps",
        "vlnv.vendor",
        "vlnv.library",
        "vlnv.name",
        "vlnv.version",
        "maps.0.name",
        "maps.0.size_bytes",
        "maps.0.data_width_bits",
        "maps.0.registers",
        "maps.0.registers.0.name",
        "maps.0.registers.0.offset",
        "maps.0.registers.0.width_bits",
        "maps.0.registers.0.fields",
        "maps.0.registers.0.fields.0.name",
        "maps.0.registers.0.fields.0.lsb",
        "maps.0.registers.0.fields.0.width_bits",
        "maps.0.registers.0.fields.0.access",
        "maps.0.registers.0.fields.0.hwAccess",
        "maps.0.registers.0.fields.0.volatile",
        "maps.0.registers.0.fields.0.resets.0.resetType",
        "maps.0.registers.0.fields.0.resets.0.value",
        "maps.0.registers.0.fields.0.resets.0.mask",
        "reset_types.0.name",
    ] {
        fixture.write(&fixture.ip(), TIMER, |root| remove_property(root, path));
        let result = load_definition(&fixture.ip());
        assert!(result.is_err(), "missing {path} was accepted");
    }
    for path in [
        "address_spaces",
        "address_spaces.0.name",
        "address_spaces.0.address_width_bits",
        "address_spaces.0.data_width_bits",
        "address_spaces.0.size_bytes",
        "address_spaces.0.instances",
        "address_spaces.0.instances.0.name",
        "address_spaces.0.instances.0.source",
        "address_spaces.0.instances.0.mappings",
        "address_spaces.0.instances.0.mappings.0.target",
        "address_spaces.0.instances.0.mappings.0.base",
        "address_spaces.0.instances.0.mappings.0.size_bytes",
    ] {
        fixture.write(&fixture.system(), SYSTEM, |root| {
            remove_property(root, path)
        });
        assert!(
            load_definition(&fixture.system()).is_err(),
            "missing {path} was accepted"
        );
    }
}

#[test]
fn field_overlap_and_numeric_overflow_are_explicit_errors() {
    let fixture = Fixture::new();
    fixture.write(&fixture.ip(), TIMER, |root| {
        let mut other = root["maps"][0]["registers"][0]["fields"][0].clone();
        other["name"] = "OTHER".into();
        root["maps"][0]["registers"][0]["fields"]
            .as_sequence_mut()
            .unwrap()
            .push(other);
    });
    error(|| load_definition(&fixture.ip()), "overlap");
    fs::write(
        fixture.ip(),
        TIMER.replace("lsb: 0", "lsb: 18446744073709551615"),
    )
    .unwrap();
    error(|| load_definition(&fixture.ip()), "field-bounds");
    fs::write(
        fixture.ip(),
        TIMER.replace("width_bits: 32", "width_bits: 18446744073709551616"),
    )
    .unwrap();
    error(|| load_definition(&fixture.ip()), "schema");
    fs::write(
        fixture.cluster(),
        CLUSTER.replace("address_width_bits: 16", "address_width_bits: 8"),
    )
    .unwrap();
    error(|| load_definition(&fixture.cluster()), "address-width");
}
