//! Read absolute Steam totals without mutating Steam files or retaining their contents.
use super::keyvalues::parse_root;
use super::{
    ScanInput,
    path::{join, normalize},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterValue {
    pub seconds: i64,
    /// Steam writes 0 for a game it has never recorded a play time for; that is unknown, not
    /// 1970.
    pub last_played: Option<i64>,
}

pub fn read(input: &ScanInput<'_>) -> BTreeMap<String, CounterValue> {
    let mut totals = BTreeMap::<String, CounterValue>::new();
    let mut seen = BTreeSet::new();
    let mut roots: Vec<_> = input
        .roots
        .iter()
        .filter_map(|root| input.files.canonicalize(root).ok())
        .collect();
    roots.sort();
    for root in roots {
        if !seen.insert(normalize(&root).to_lowercase()) {
            continue;
        }
        let userdata = join(&root, "userdata");
        let Ok(mut accounts) = input.files.read_dir(&userdata) else {
            continue;
        };
        accounts.sort();
        for account in accounts {
            if account.is_empty() || !account.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let path = join(
                &join(&join(&userdata, &account), "config"),
                "localconfig.vdf",
            );
            let Ok(text) = input.files.read_to_string(&path) else {
                tracing::debug!("Steam counter file unavailable");
                continue;
            };
            let Ok(config) = parse_root(&text, "UserLocalConfigStore") else {
                tracing::warn!("ignored malformed Steam counter file");
                continue;
            };
            let Some(apps) = config
                .get("Software")
                .and_then(|s| s.get("Valve"))
                .and_then(|s| s.get("Steam"))
                .and_then(|s| s.get("apps"))
            else {
                continue;
            };
            for (id, app) in apps.entries() {
                if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    continue;
                }
                let value = app
                    .text("Playtime")
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .filter(|v| *v >= 0)
                    .and_then(|v| v.checked_mul(60))
                    .zip(
                        app.text("LastPlayed")
                            .and_then(|s| s.trim().parse::<i64>().ok())
                            .filter(|v| *v >= 0),
                    );
                let Some((seconds, last_played)) = value else {
                    tracing::debug!("ignored invalid Steam counter entry");
                    continue;
                };
                let candidate = CounterValue {
                    seconds,
                    last_played: (last_played != 0).then_some(last_played),
                };
                if totals
                    .get(id)
                    .is_none_or(|old| candidate.last_played > old.last_played)
                {
                    totals.insert(id.clone(), candidate);
                }
            }
        }
    }
    totals
}
#[cfg(test)]
mod tests {
    use super::super::memory::MemoryFiles;
    use super::*;
    fn config(minutes: &str, last: &str) -> String {
        format!(
            r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "620" {{ "Playtime" "{minutes}" "LastPlayed" "{last}" }} }} }} }} }} }}"#
        )
    }
    #[test]
    fn converts_minutes_and_chooses_latest_account_with_corrupt_neighbor() {
        let mut files = MemoryFiles::new();
        files = files.with_file(
            "/steam/userdata/1/config/localconfig.vdf",
            &config("754", "100"),
        );
        files = files.with_file(
            "/steam/userdata/2/config/localconfig.vdf",
            &config("10", "90"),
        );
        files = files.with_file("/steam/userdata/3/config/localconfig.vdf", "malformed {");
        let roots = vec!["/steam".into()];
        assert_eq!(
            read(&ScanInput {
                files: &files,
                roots: &roots
            })["620"],
            CounterValue {
                seconds: 45240,
                last_played: Some(100)
            }
        );
        files = files.with_file(
            "/steam/userdata/2/config/localconfig.vdf",
            &config("12", "110"),
        );
        assert_eq!(
            read(&ScanInput {
                files: &files,
                roots: &roots
            })["620"]
                .seconds,
            720
        );
    }
    #[test]
    fn a_zero_last_played_is_unknown_and_a_date_is_present() {
        let mut files = MemoryFiles::new();
        files = files.with_file(
            "/steam/userdata/1/config/localconfig.vdf",
            &config("1", "0"),
        );
        let totals = read(&ScanInput {
            files: &files,
            roots: &["/steam".into()],
        });
        assert_eq!(
            totals["620"],
            CounterValue {
                seconds: 60,
                last_played: None
            }
        );
    }

    #[test]
    fn invalid_or_missing_values_produce_no_total() {
        for text in [
            config("-1", "100"),
            config("9223372036854775807", "100"),
            config("1", "-1"),
            config("no", "100"),
            r#""UserLocalConfigStore" {}"#.into(),
        ] {
            let mut files = MemoryFiles::new();
            files = files.with_file("/steam/userdata/1/config/localconfig.vdf", &text);
            assert!(
                read(&ScanInput {
                    files: &files,
                    roots: &["/steam".into()]
                })
                .is_empty()
            );
        }
    }
}
