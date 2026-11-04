//! The plain record every scanner produces, native or generic.

use std::collections::BTreeMap;

/// One detected game, as the runtime sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Provider-local id (for Steam, the app id).
    pub id: String,
    pub name: Option<String>,
    pub install_dir: String,
    /// Further named text fields a manifest template may refer to.
    pub fields: BTreeMap<String, String>,
}

impl Record {
    /// A template field: `id`, `name`, `install_dir`, or an entry of `fields`.
    pub fn field(&self, name: &str) -> Option<&str> {
        match name {
            "id" => Some(self.id.as_str()),
            "name" => self.name.as_deref(),
            "install_dir" => Some(self.install_dir.as_str()),
            other => self.fields.get(other).map(String::as_str),
        }
    }
}

/// What a scanner returns before the runtime checks it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawRecord {
    pub id: Option<String>,
    pub name: Option<String>,
    pub install_dir: Option<String>,
    pub fields: BTreeMap<String, String>,
}

/// Keeps records with a usable `id` and `install_dir`, drops the rest with a log, and removes
/// excluded ids and repeated ids (the first one wins). Never fails: one bad record does not
/// affect the others.
pub fn finish_records(provider: &str, raw: Vec<RawRecord>, exclude_ids: &[String]) -> Vec<Record> {
    let mut seen = std::collections::BTreeSet::new();
    let mut records = Vec::with_capacity(raw.len());
    for entry in raw {
        let id = entry.id.as_deref().map(str::trim).unwrap_or_default();
        let install_dir = entry
            .install_dir
            .as_deref()
            .map(str::trim)
            .unwrap_or_default();
        if id.is_empty() || id.chars().any(char::is_control) || install_dir.is_empty() {
            tracing::warn!(
                provider,
                "dropped a scan record without a usable id or install folder"
            );
            continue;
        }
        if exclude_ids.iter().any(|excluded| excluded == id) {
            continue;
        }
        if !seen.insert(id.to_owned()) {
            continue;
        }
        records.push(Record {
            id: id.to_owned(),
            name: entry
                .name
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty()),
            install_dir: install_dir.to_owned(),
            fields: entry.fields,
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: Option<&str>, dir: Option<&str>) -> RawRecord {
        RawRecord {
            id: id.map(str::to_owned),
            name: Some("Game".to_owned()),
            install_dir: dir.map(str::to_owned),
            fields: BTreeMap::new(),
        }
    }

    #[test]
    fn incomplete_records_are_dropped_and_the_rest_survive() {
        let records = finish_records(
            "steam",
            vec![
                raw(Some("1"), Some("/g/one")),
                raw(None, Some("/g/two")),
                raw(Some("3"), None),
                raw(Some("  "), Some("/g/four")),
                raw(Some("5"), Some("")),
                raw(Some("6"), Some("/g/six")),
            ],
            &[],
        );
        let ids: Vec<_> = records.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["1", "6"]);
    }

    #[test]
    fn excluded_and_repeated_ids_are_removed() {
        let records = finish_records(
            "steam",
            vec![
                raw(Some("228980"), Some("/g/redist")),
                raw(Some("10"), Some("/first")),
                raw(Some("10"), Some("/second")),
            ],
            &["228980".to_owned()],
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].install_dir, "/first");
    }

    #[test]
    fn fields_resolve_by_name() {
        let mut fields = BTreeMap::new();
        fields.insert("state_flags".to_owned(), "4".to_owned());
        let record = Record {
            id: "1".to_owned(),
            name: None,
            install_dir: "/g".to_owned(),
            fields,
        };
        assert_eq!(record.field("id"), Some("1"));
        assert_eq!(record.field("name"), None);
        assert_eq!(record.field("install_dir"), Some("/g"));
        assert_eq!(record.field("state_flags"), Some("4"));
        assert_eq!(record.field("other"), None);
    }
}
