//! Reusable scanner contracts and filesystem-independent scanner implementations.

use std::collections::BTreeMap;

mod keyvalues;
pub mod steam;

pub trait Files {
    fn canonicalize(&self, path: &str) -> Result<String, String>;
    fn read_dir(&self, path: &str) -> Result<Vec<String>, String>;
    fn read_to_string(&self, path: &str) -> Result<String, String>;
    fn is_dir(&self, path: &str) -> bool;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawRecord {
    pub id: Option<String>,
    pub name: Option<String>,
    pub install_dir: Option<String>,
    pub fields: BTreeMap<String, String>,
}

pub struct ScanInput<'a> {
    pub files: &'a dyn Files,
    pub roots: &'a [String],
}
