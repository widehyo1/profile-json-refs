use rusqlite::Connection;

use crate::error::{ProfileError, Result};

pub const REQUIRED_TABLES: &[&str] = &[
    "schema_paths",
    "array_index_refs",
    "schema_definitions",
    "schema_object_counts",
    "schema_field_counts",
    "schema_site_counts",
    "schema_site_field_counts",
    "schema_site_presence_shapes",
    "schema_site_presence_shape_limits",
];

pub fn validate_required_tables(conn: &Connection) -> Result<()> {
    let mut stmt =
        conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1")?;

    for table in REQUIRED_TABLES {
        let found: rusqlite::Result<String> = stmt.query_row([table], |row| row.get(0));
        if found.is_err() {
            return Err(ProfileError::InvalidConfig(format!(
                "refs database is missing required table: {table}"
            )));
        }
    }

    Ok(())
}
