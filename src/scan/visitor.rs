use serde_json::{Map, Value};

use crate::error::Result;
use crate::scan::path::SourcePath;

pub trait ScanVisitor {
    fn begin_document(&mut self, document_index: u64) -> Result<()>;
    fn end_document(&mut self, document_index: u64) -> Result<()>;

    fn visit_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        object: &Map<String, Value>,
    ) -> Result<()>;

    fn visit_array(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        array: &[Value],
    ) -> Result<()>;

    fn visit_scalar(&mut self, document_index: u64, path: &SourcePath, value: &Value)
    -> Result<()>;
}

pub(crate) fn walk_value(
    document_index: u64,
    value: &Value,
    path: &mut SourcePath,
    visitor: &mut impl ScanVisitor,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            visitor.visit_object(document_index, path, object)?;
            for (field, child) in object {
                path.push_field(field);
                walk_value(document_index, child, path, visitor)?;
                path.pop();
            }
        }
        Value::Array(array) => {
            visitor.visit_array(document_index, path, array)?;
            for (index, child) in array.iter().enumerate() {
                path.push_index(index);
                walk_value(document_index, child, path, visitor)?;
                path.pop();
            }
        }
        _ => visitor.visit_scalar(document_index, path, value)?,
    }
    Ok(())
}
