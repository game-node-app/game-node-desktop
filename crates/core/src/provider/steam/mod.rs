//! Native Steam provider adapter for the reusable scanner implementation.

pub mod counter;
pub mod keyvalues;

use crate::provider::{RawRecord, ScanError, ScanInput};

struct FilesAdapter<'a>(&'a dyn crate::ports::HostFiles);

impl gamenode_scanner::Files for FilesAdapter<'_> {
    fn canonicalize(&self, path: &str) -> Result<String, String> {
        self.0.canonicalize(path).map_err(|error| error.to_string())
    }

    fn read_dir(&self, path: &str) -> Result<Vec<String>, String> {
        self.0.read_dir(path).map_err(|error| error.to_string())
    }

    fn read_to_string(&self, path: &str) -> Result<String, String> {
        self.0
            .read_to_string(path)
            .map_err(|error| error.to_string())
    }

    fn is_dir(&self, path: &str) -> bool {
        self.0.is_dir(path)
    }
}

pub fn scan(input: &ScanInput<'_>) -> Result<Vec<RawRecord>, ScanError> {
    let files = FilesAdapter(input.files);
    gamenode_scanner::steam::scan(&gamenode_scanner::ScanInput {
        files: &files,
        roots: input.roots,
    })
    .map(|records| {
        records
            .into_iter()
            .map(|record| RawRecord {
                id: record.id,
                name: record.name,
                install_dir: record.install_dir,
                fields: record.fields,
            })
            .collect()
    })
    .map_err(ScanError::Failed)
}

#[cfg(test)]
mod tests;
