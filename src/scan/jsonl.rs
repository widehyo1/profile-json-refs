use std::io::BufRead;
use std::time::Instant;

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

        let perf_enabled = visitor.perf_enabled();
        let parse_started = perf_enabled.then(Instant::now);
        let value: Value = serde_json::from_str(&line)?;
        if let Some(started) = parse_started {
            visitor.record_scan_parse_elapsed(started.elapsed());
            visitor.record_scan_bytes((line.len() + 1) as u64);
        }

        let document_index = line_index as u64;
        let mut path = SourcePath::root();
        visitor.begin_scan_walk_timing();
        let walk_result: Result<()> = (|| {
            visitor.begin_document(document_index)?;
            walk_value(document_index, &value, &mut path, visitor)?;
            visitor.end_document(document_index)?;
            Ok(())
        })();
        visitor.end_scan_walk_timing();
        walk_result?;
    }
    Ok(())
}
