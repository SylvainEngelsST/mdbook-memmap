//! Typed authoring definitions and resolved views. Addresses and reset values
//! retain arbitrary precision; widths use checked 64-bit arithmetic.

use num_bigint::BigUint;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Origin {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub path: String,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{} {}",
            self.file.display(),
            self.line,
            self.column,
            self.path
        )
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum Access {
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "write-only")]
    WriteOnly,
    #[serde(rename = "read-write")]
    ReadWrite,
    #[serde(rename = "writeOnce")]
    WriteOnce,
    #[serde(rename = "read-writeOnce")]
    ReadWriteOnce,
    #[serde(rename = "no-access")]
    NoAccess,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HardwareAccess {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    NoAccess,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WriteEffect {
    OneToClear,
    OneToSet,
    OneToToggle,
    ZeroToClear,
    ZeroToSet,
    ZeroToToggle,
    Clear,
    Set,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReadEffect {
    Clear,
    Set,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Usage {
    Register,
    Memory,
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Vlnv {
    pub vendor: String,
    pub library: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct ResetType {
    pub name: String,
    pub description: Option<String>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct Reset {
    pub reset_type: String,
    pub value: BigUint,
    pub mask: BigUint,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub description: Option<String>,
    pub lsb: u64,
    pub width_bits: u64,
    pub access: Access,
    pub hw_access: HardwareAccess,
    pub volatile: bool,
    pub modified_write_value: Option<WriteEffect>,
    pub read_action: Option<ReadEffect>,
    pub resets: Vec<Reset>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct Register {
    pub name: String,
    pub description: Option<String>,
    pub offset: BigUint,
    pub width_bits: u64,
    pub fields: Vec<Field>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct MemoryMap {
    pub name: String,
    pub description: Option<String>,
    pub usage: Usage,
    pub size_bytes: BigUint,
    pub data_width_bits: u64,
    pub access: Option<Access>,
    pub registers: Vec<Register>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct Mapping {
    pub target: String,
    pub base: BigUint,
    pub size_bytes: BigUint,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub name: String,
    pub description: Option<String>,
    pub source: String,
    pub mappings: Vec<Mapping>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub struct AddressSpace {
    pub name: String,
    pub description: Option<String>,
    pub address_width_bits: u64,
    pub data_width_bits: u64,
    pub size_bytes: BigUint,
    pub instances: Vec<Instance>,
    pub origin: Origin,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Ip {
        reset_types: Vec<ResetType>,
        maps: Vec<MemoryMap>,
    },
    System {
        address_spaces: Vec<AddressSpace>,
    },
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub vlnv: Vlnv,
    pub description: Option<String>,
    pub origin: Origin,
    pub kind: Kind,
}

#[derive(Debug, Clone)]
pub struct ResolvedBlock {
    pub definition: Arc<Definition>,
    pub memory_map: MemoryMap,
    pub instances: Vec<Instance>,
    pub base: BigUint,
    pub mapping: Mapping,
}

impl ResolvedBlock {
    pub fn instance_path(&self) -> Vec<String> {
        self.instances
            .iter()
            .map(|instance| instance.name.clone())
            .collect()
    }
}

#[derive(Debug)]
pub struct ResolvedView {
    pub definition: Arc<Definition>,
    pub address_space: AddressSpace,
    pub blocks: Vec<ResolvedBlock>,
}
