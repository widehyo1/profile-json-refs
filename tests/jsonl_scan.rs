use std::io::BufReader;

use profile_json_refs::error::Result;
use profile_json_refs::scan::jsonl::scan_jsonl_file;
use profile_json_refs::scan::path::SourcePath;
use profile_json_refs::scan::visitor::ScanVisitor;
use serde_json::{Map, Value};

#[derive(Default)]
struct CountingVisitor {
    begin_documents: Vec<u64>,
    end_documents: Vec<u64>,
    object_paths: Vec<String>,
    array_paths: Vec<String>,
    scalar_paths: Vec<String>,
}

impl ScanVisitor for CountingVisitor {
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
fn jsonl_scanner_treats_each_non_empty_line_as_a_document() {
    let input = b"{\"id\":1}\n\n[1,{\"nested\":true}]\n";
    let mut visitor = CountingVisitor::default();

    scan_jsonl_file(BufReader::new(&input[..]), &mut visitor).unwrap();

    assert_eq!(visitor.begin_documents, vec![0, 2]);
    assert_eq!(visitor.end_documents, vec![0, 2]);
    assert_eq!(visitor.object_paths.len(), 2);
    assert_eq!(visitor.array_paths.len(), 1);
    assert_eq!(visitor.scalar_paths.len(), 3);
}

#[test]
fn jsonl_scanner_ignores_blank_lines() {
    let input = b"\n  \n{\"id\":1}\n";
    let mut visitor = CountingVisitor::default();

    scan_jsonl_file(BufReader::new(&input[..]), &mut visitor).unwrap();

    assert_eq!(visitor.begin_documents, vec![2]);
    assert_eq!(visitor.object_paths, vec!["$".to_string()]);
}
