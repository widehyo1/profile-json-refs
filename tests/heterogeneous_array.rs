use profile_json_refs::error::Result;
use profile_json_refs::refs::resolver::RefsResolver;
use profile_json_refs::refs::site::{RefsIndex, SiteContext};
use profile_json_refs::scan::json::scan_json_file;
use profile_json_refs::scan::path::SourcePath;
use profile_json_refs::scan::visitor::ScanVisitor;
use profile_json_refs::shape::accumulator::ShapeAccumulator;
use profile_json_refs::sqlite::schema::create_schema;
use rusqlite::Connection;
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Default)]
struct ObjectPathVisitor {
    object_paths: Vec<String>,
}

impl ScanVisitor for ObjectPathVisitor {
    fn begin_document(&mut self, _document_index: u64) -> Result<()> {
        Ok(())
    }

    fn end_document(&mut self, _document_index: u64) -> Result<()> {
        Ok(())
    }

    fn visit_object(
        &mut self,
        _document_index: u64,
        path: &SourcePath,
        _object: &Map<String, Value>,
    ) -> Result<()> {
        self.object_paths.push(path.as_str());
        Ok(())
    }

    fn visit_array(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _array: &[Value],
    ) -> Result<()> {
        Ok(())
    }

    fn visit_scalar(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _value: &Value,
    ) -> Result<()> {
        Ok(())
    }
}

#[test]
fn object_elements_inside_arrays_emit_object_events() {
    let input = br#"{"items":[{"id":1},{"error":"invalid"}]}"#;
    let mut visitor = ObjectPathVisitor::default();

    scan_json_file(&input[..], &mut visitor).unwrap();

    assert!(visitor.object_paths.contains(&"$".to_string()));
    assert!(visitor.object_paths.contains(&"$.items[0]".to_string()));
    assert!(visitor.object_paths.contains(&"$.items[1]".to_string()));
}

struct ShapeCollectingVisitor {
    resolver: RefsResolver,
    accumulator: ShapeAccumulator,
}

impl ScanVisitor for ShapeCollectingVisitor {
    fn begin_document(&mut self, _document_index: u64) -> Result<()> {
        Ok(())
    }

    fn end_document(&mut self, _document_index: u64) -> Result<()> {
        Ok(())
    }

    fn visit_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        object: &Map<String, Value>,
    ) -> Result<()> {
        let context = self.resolver.resolve_object(path);
        self.accumulator
            .observe_object(document_index, path, &context, object, &Default::default())
    }

    fn visit_array(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _array: &[Value],
    ) -> Result<()> {
        Ok(())
    }

    fn visit_scalar(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _value: &Value,
    ) -> Result<()> {
        Ok(())
    }
}

fn array_item_refs_index() -> RefsIndex {
    let mut site_by_source_path = HashMap::new();
    for index in 0..3 {
        site_by_source_path.insert(
            format!("$.items[{index}]"),
            SiteContext {
                canonical_path: "$.items[]".to_string(),
                site_path: Some("$.items[]".to_string()),
                schema_path: "#/items".to_string(),
            },
        );
    }

    RefsIndex {
        site_by_source_path,
        ..RefsIndex::default()
    }
}

#[test]
fn heterogeneous_object_array_produces_multiple_shapes_for_one_array_site() {
    let input = br#"{
        "items": [
            {"id": 1, "type": "A", "amount": 100},
            {"id": 2, "type": "B", "error": "invalid"},
            {"id": 3, "type": "A", "amount": "200"}
        ]
    }"#;
    let mut visitor = ShapeCollectingVisitor {
        resolver: RefsResolver::new(array_item_refs_index()),
        accumulator: ShapeAccumulator::default(),
    };

    scan_json_file(&input[..], &mut visitor).unwrap();

    let item_shapes: Vec<_> = visitor
        .accumulator
        .shape_rows()
        .into_iter()
        .filter(|row| {
            row.canonical_path == "$.items[]" && row.site_path.as_deref() == Some("$.items[]")
        })
        .collect();

    assert_eq!(item_shapes.len(), 3);
    assert!(item_shapes.iter().all(|row| row.schema_path == "#/items"));
}

#[test]
fn profile_schema_still_does_not_create_array_specific_tables() {
    let conn = Connection::open_in_memory().unwrap();

    create_schema(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table' AND name LIKE 'prof_array_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 0);
}
