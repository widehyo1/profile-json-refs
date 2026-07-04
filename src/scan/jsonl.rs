use std::io::BufRead;

use serde_json::Value;

use crate::error::Result;
use crate::scan::path::SourcePath;
use crate::scan::visitor::{ScanVisitor, walk_value};

pub fn scan_jsonl_file<R: BufRead>(reader: R, visitor: &mut impl ScanVisitor) -> Result<()> {
    for (line_index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(&line)?;
        let document_index = line_index as u64;
        let mut path = SourcePath::root();
        visitor.begin_document(document_index)?;
        walk_value(document_index, &value, &mut path, visitor)?;
        visitor.end_document(document_index)?;
    }
    Ok(())
}
