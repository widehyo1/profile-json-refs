use profile_json_refs::error::Result;
use profile_json_refs::scan::json::scan_json_file;
use profile_json_refs::scan::path::SourcePath;
use profile_json_refs::scan::visitor::ScanVisitor;
use serde_json::{Map, Value};

#[derive(Default)]
struct RecordingVisitor {
    begin_documents: Vec<u64>,
    end_documents: Vec<u64>,
    object_paths: Vec<String>,
    array_paths: Vec<String>,
    scalar_paths: Vec<String>,
}

impl ScanVisitor for RecordingVisitor {
    fn begin_document(&mut self, document_index: u64) -> Result<()> {
        self.begin_documents.push(document_index);
        Ok(())
    }

    fn end_document(&mut self, document_index: u64) -> Result<()> {
        self.end_documents.push(document_index);
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
        path: &SourcePath,
        _array: &[Value],
    ) -> Result<()> {
        self.array_paths.push(path.as_str());
        Ok(())
    }

    fn visit_scalar(
        &mut self,
        _document_index: u64,
        path: &SourcePath,
        _value: &Value,
    ) -> Result<()> {
        self.scalar_paths.push(path.as_str());
        Ok(())
    }
}

#[test]
fn json_scanner_counts_documents_and_value_kinds() {
    let input = br#"{
        "items": [
            {"id": 1, "tags": ["new", null]},
            {"id": 2}
        ],
        "active": true,
        "empty": {}
    }"#;
    let mut visitor = RecordingVisitor::default();

    scan_json_file(&input[..], &mut visitor).unwrap();

    assert_eq!(visitor.begin_documents, vec![0]);
    assert_eq!(visitor.end_documents, vec![0]);
    assert_eq!(visitor.object_paths.len(), 4);
    assert_eq!(visitor.array_paths.len(), 2);
    assert_eq!(visitor.scalar_paths.len(), 5);
}

#[test]
fn json_scanner_tracks_object_array_and_scalar_paths() {
    let input = br#"{
        "items": [
            {"id": 1, "tags": ["new", null]},
            {"id": 2}
        ],
        "active": true,
        "empty": {}
    }"#;
    let mut visitor = RecordingVisitor::default();

    scan_json_file(&input[..], &mut visitor).unwrap();

    assert!(visitor.object_paths.contains(&"$".to_string()));
    assert!(visitor.object_paths.contains(&"$.items[0]".to_string()));
    assert!(visitor.object_paths.contains(&"$.items[1]".to_string()));
    assert!(visitor.object_paths.contains(&"$.empty".to_string()));
    assert!(visitor.array_paths.contains(&"$.items".to_string()));
    assert!(visitor.array_paths.contains(&"$.items[0].tags".to_string()));
    assert!(visitor.scalar_paths.contains(&"$.active".to_string()));
    assert!(
        visitor
            .scalar_paths
            .contains(&"$.items[0].tags[1]".to_string())
    );
}
