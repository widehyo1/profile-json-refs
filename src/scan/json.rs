use std::io::Read;

use serde_json::Value;

use crate::error::Result;
use crate::scan::path::SourcePath;
use crate::scan::visitor::{ScanVisitor, walk_value};

pub fn scan_json_file<R: Read>(reader: R, visitor: &mut impl ScanVisitor) -> Result<()> {
    let value: Value = serde_json::from_reader(reader)?;
    let mut path = SourcePath::root();
    visitor.begin_document(0)?;
    walk_value(0, &value, &mut path, visitor)?;
    visitor.end_document(0)?;
    Ok(())
}
