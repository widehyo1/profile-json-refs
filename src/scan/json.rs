use std::io::Read;
use std::time::Instant;

use serde_json::Value;

use crate::error::Result;
use crate::scan::path::SourcePath;
use crate::scan::visitor::{ScanVisitor, walk_value};

pub fn scan_json_file<R: Read>(reader: R, visitor: &mut impl ScanVisitor) -> Result<()> {
    let parse_started = visitor.perf_enabled().then(Instant::now);
    let value: Value = serde_json::from_reader(reader)?;
    if let Some(started) = parse_started {
        visitor.record_scan_parse_elapsed(started.elapsed());
    }

    let mut path = SourcePath::root();
    visitor.begin_scan_walk_timing();
    let walk_result: Result<()> = (|| {
        visitor.begin_document(0)?;
        walk_value(0, &value, &mut path, visitor)?;
        visitor.end_document(0)?;
        Ok(())
    })();
    visitor.end_scan_walk_timing();
    walk_result?;
    Ok(())
}
