//! Native upgrade diagnostic for the settings database.
//!
//! Usage: `cargo run --example upgrade_check -- <database path>`
//!
//! Runs this build's migration runner over a database copy and prints only structural evidence:
//! schema version, recorded migration history, `PRAGMA integrity_check`, per-table row counts and
//! SHA-256 file hashes. It never prints row values, so it is safe to run on a copied profile.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn file_sha256(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn print_history(conn: &rusqlite::Connection) {
    let mut statement = conn
        .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
        .expect("schema_migrations is readable");
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })
        .expect("history is readable");
    for row in rows {
        let (version, name) = row.expect("history row decodes");
        println!("  {version}\t{name}");
    }
}

fn print_tables(conn: &rusqlite::Connection) {
    let mut statement = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("sqlite_master is readable");
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("table names decode")
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    for table in tables {
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\"")),
                [],
                |row| row.get(0),
            )
            .expect("table is readable");
        println!("  {table}\t{count}");
    }
}

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: upgrade_check <database path>");
        return std::process::ExitCode::from(2);
    };

    println!("database: {}", path.display());
    if let Some(hash) = file_sha256(&path) {
        println!("sha256 before: {hash}");
    }

    let conn = match gamenode_platform::db::open(&path) {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!("refused: {error}");
            if let Some(hash) = file_sha256(&path) {
                println!("sha256 after: {hash}");
            }
            return std::process::ExitCode::FAILURE;
        }
    };

    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("integrity_check runs");
    let version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .expect("history is readable");
    println!("schema version: {version}");
    println!("integrity_check: {integrity}");
    println!("history:");
    print_history(&conn);
    println!("tables:");
    print_tables(&conn);
    drop(conn);
    if let Some(hash) = file_sha256(&path) {
        println!("sha256 after: {hash}");
    }
    if let Ok(entries) = std::fs::read_dir(path.parent().unwrap_or(Path::new("."))) {
        let prefix = format!(
            "{}.pre-migration-",
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(prefix.as_str())
            {
                println!("snapshot: {}", entry.path().display());
            }
        }
    }
    std::process::ExitCode::SUCCESS
}
