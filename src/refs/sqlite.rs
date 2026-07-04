use rusqlite::Connection;
use std::path::Path;

use crate::ProfileWarning;
use crate::error::Result;
use crate::refs::contract::validate_required_tables;
use crate::refs::site::{RefsIndex, SiteContext, SitePresenceShapeSeed};

pub const W_REFS_PRESENCE_SHAPES_TRUNCATED: &str = "W_REFS_PRESENCE_SHAPES_TRUNCATED";

#[derive(Debug)]
pub struct LoadedRefs {
    pub index: RefsIndex,
    pub warnings: Vec<ProfileWarning>,
}

pub fn load_refs_index_from_path(path: impl AsRef<Path>) -> Result<LoadedRefs> {
    let conn = Connection::open(path)?;
    load_refs_index(&conn)
}

pub fn load_refs_index(conn: &Connection) -> Result<LoadedRefs> {
    validate_required_tables(conn)?;

    let mut index = RefsIndex {
        schema_by_canonical: load_schema_by_canonical(conn)?,
        ..RefsIndex::default()
    };
    index.site_by_source_path = load_site_contexts(conn, &index)?;
    let truncated_sites = load_truncated_sites(conn)?;
    index.presence_shape_truncated = !truncated_sites.is_empty();
    index.presence_shape_seeds = load_presence_shape_seeds(conn, &index, &truncated_sites)?;

    let warnings = if index.presence_shape_truncated {
        vec![ProfileWarning {
            code: W_REFS_PRESENCE_SHAPES_TRUNCATED.to_string(),
            message: "refs database indicates truncated site presence shapes".to_string(),
        }]
    } else {
        Vec::new()
    };

    Ok(LoadedRefs { index, warnings })
}

fn load_schema_by_canonical(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT object_path, schema_path
         FROM schema_paths
         ORDER BY object_path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut mapping = std::collections::HashMap::new();
    for row in rows {
        let (object_path, schema_path) = row?;
        mapping.insert(object_path, schema_path);
    }
    Ok(mapping)
}

fn load_site_contexts(
    conn: &Connection,
    index: &RefsIndex,
) -> Result<std::collections::HashMap<String, SiteContext>> {
    let mut stmt = conn.prepare(
        "SELECT c.site_path, c.schema_path, p.object_path
         FROM schema_site_counts AS c
         LEFT JOIN schema_paths AS p ON p.schema_path = c.schema_path
         ORDER BY c.site_path, c.schema_path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut contexts = std::collections::HashMap::new();
    for row in rows {
        let (site_path, schema_path, object_path) = row?;
        let canonical_path = object_path
            .or_else(|| {
                index
                    .schema_by_canonical
                    .iter()
                    .find_map(|(canonical, mapped_schema)| {
                        (mapped_schema == &schema_path).then(|| canonical.clone())
                    })
            })
            .unwrap_or_else(|| schema_path.clone());
        contexts.insert(
            site_path.clone(),
            SiteContext {
                canonical_path,
                site_path: Some(site_path),
                schema_path,
            },
        );
    }
    Ok(contexts)
}

fn load_truncated_sites(
    conn: &Connection,
) -> Result<std::collections::HashSet<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT schema_path, site_path, site_kind
         FROM schema_site_presence_shape_limits
         WHERE truncated != 0
         ORDER BY schema_path, site_path, site_kind",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut sites = std::collections::HashSet::new();
    for row in rows {
        sites.insert(row?);
    }
    Ok(sites)
}

fn load_presence_shape_seeds(
    conn: &Connection,
    index: &RefsIndex,
    truncated_sites: &std::collections::HashSet<(String, String, String)>,
) -> Result<Vec<SitePresenceShapeSeed>> {
    let mut stmt = conn.prepare(
        "SELECT s.schema_path, s.site_path, s.site_kind, s.present_fields_json, p.object_path
         FROM schema_site_presence_shapes AS s
         LEFT JOIN schema_paths AS p ON p.schema_path = s.schema_path
         ORDER BY s.schema_path, s.site_path, s.site_kind, s.present_fields_json",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut seeds = Vec::new();
    for row in rows {
        let (schema_path, site_path, site_kind, present_fields_json, object_path) = row?;
        let field_names =
            serde_json::from_str::<Vec<String>>(&present_fields_json).map_err(|err| {
                crate::error::ProfileError::InvalidConfig(format!(
                    "invalid refs presence shape field list for {site_path}: {err}"
                ))
            })?;
        let canonical_path = object_path
            .or_else(|| {
                index
                    .schema_by_canonical
                    .iter()
                    .find_map(|(canonical, mapped_schema)| {
                        (mapped_schema == &schema_path).then(|| canonical.clone())
                    })
            })
            .unwrap_or_else(|| schema_path.clone());
        let truncated =
            truncated_sites.contains(&(schema_path.clone(), site_path.clone(), site_kind.clone()));
        seeds.push(SitePresenceShapeSeed {
            canonical_path,
            site_path: Some(site_path),
            schema_path,
            field_names,
            truncated,
        });
    }
    Ok(seeds)
}
