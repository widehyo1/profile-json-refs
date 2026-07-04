use profile_json_refs::error::Result;
use profile_json_refs::scan::json::scan_json_file;
use profile_json_refs::scan::path::SourcePath;
use profile_json_refs::scan::visitor::ScanVisitor;
use serde_json::{Map, Value};

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
