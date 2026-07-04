# Profile Detail 03: Scanner, Shape Identity, and Object Samples

Covers:

```text
Phase 4: source scanner
Phase 5: shape identity and object samples
```

---

## 1. Target Files

```text
src/scan/mod.rs
src/scan/json.rs
src/scan/jsonl.rs
src/scan/path.rs
src/scan/visitor.rs
src/shape/mod.rs
src/shape/id.rs
src/shape/token.rs
src/shape/accumulator.rs
src/shape/sample.rs
src/util/json_type.rs
src/util/hash.rs
src/util/truncate.rs
tests/json_scan.rs
tests/jsonl_scan.rs
tests/shape_identity.rs
tests/object_samples.rs
tests/heterogeneous_array.rs
```

---

## 2. Scanner Strategy

v0.1.0 must stream input.

For initial implementation, using `serde_json::Deserializer` is acceptable if the scanner processes one top-level JSON value or one JSONL line at a time and does not keep the full source around after traversal.

JSON:

```rust
pub fn scan_json_file<R: std::io::Read>(
    reader: R,
    visitor: &mut impl ScanVisitor,
) -> crate::error::Result<()> {
    let value: serde_json::Value = serde_json::from_reader(reader)?;
    let mut path = SourcePath::root();
    visitor.begin_document(0)?;
    walk_value(&value, &mut path, visitor)?;
    visitor.end_document(0)?;
    Ok(())
}
```

JSONL:

```rust
pub fn scan_jsonl_file<R: std::io::BufRead>(
    reader: R,
    visitor: &mut impl ScanVisitor,
) -> crate::error::Result<()> {
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)?;
        let mut path = SourcePath::root();
        visitor.begin_document(idx as u64)?;
        walk_value(&value, &mut path, visitor)?;
        visitor.end_document(idx as u64)?;
    }
    Ok(())
}
```

Later optimization may replace `serde_json::Value` traversal with event streaming. The interface should not require retaining `Value`.

---

## 3. SourcePath

`src/scan/path.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourcePath {
    parts: Vec<PathPart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathPart {
    Field(String),
    ArrayIndex(usize),
}

impl SourcePath {
    pub fn root() -> Self {
        Self { parts: Vec::new() }
    }

    pub fn push_field(&mut self, field: &str) {
        self.parts.push(PathPart::Field(field.to_string()));
    }

    pub fn push_index(&mut self, index: usize) {
        self.parts.push(PathPart::ArrayIndex(index));
    }

    pub fn pop(&mut self) {
        self.parts.pop();
    }

    pub fn as_str(&self) -> String {
        // Example: $.items[0].amount
        todo!()
    }

    pub fn to_canonical_guess(&self) -> String {
        // Example: $.items[].amount
        todo!()
    }
}
```

Canonical guessing is fallback only. Prefer refs resolver output.

---

## 4. ScanVisitor

`src/scan/visitor.rs`:

```rust
use serde_json::Value;
use crate::scan::path::SourcePath;

pub trait ScanVisitor {
    fn begin_document(&mut self, document_index: u64) -> crate::error::Result<()>;
    fn end_document(&mut self, document_index: u64) -> crate::error::Result<()>;

    fn visit_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        object: &serde_json::Map<String, Value>,
    ) -> crate::error::Result<()>;

    fn visit_array(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        array: &[Value],
    ) -> crate::error::Result<()>;

    fn visit_scalar(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        value: &Value,
    ) -> crate::error::Result<()>;
}
```

Traversal must call `visit_object` for object elements inside arrays. This is the v0.1.0 heterogeneous object array mechanism.

---

## 5. Walk Algorithm

```rust
fn walk_value(
    value: &serde_json::Value,
    path: &mut SourcePath,
    visitor: &mut impl ScanVisitor,
) -> crate::error::Result<()> {
    match value {
        serde_json::Value::Object(obj) => {
            visitor.visit_object(current_document_index(), path, obj)?;
            for (key, child) in obj {
                path.push_field(key);
                walk_value(child, path, visitor)?;
                path.pop();
            }
        }
        serde_json::Value::Array(arr) => {
            visitor.visit_array(current_document_index(), path, arr)?;
            for (idx, child) in arr.iter().enumerate() {
                path.push_index(idx);
                walk_value(child, path, visitor)?;
                path.pop();
            }
        }
        _ => visitor.visit_scalar(current_document_index(), path, value)?,
    }
    Ok(())
}
```

In real code, pass `document_index` explicitly instead of `current_document_index()`.

---

## 6. JSON Type

`src/util/json_type.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JsonType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Object,
    Array,
    Unknown,
}

impl JsonType {
    pub fn from_value(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsonType::Null,
            serde_json::Value::Bool(_) => JsonType::Boolean,
            serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => JsonType::Integer,
            serde_json::Value::Number(_) => JsonType::Number,
            serde_json::Value::String(_) => JsonType::String,
            serde_json::Value::Array(_) => JsonType::Array,
            serde_json::Value::Object(_) => JsonType::Object,
        }
    }

    pub fn as_sql_str(self) -> &'static str {
        match self {
            JsonType::Null => "null",
            JsonType::Boolean => "boolean",
            JsonType::Integer => "integer",
            JsonType::Number => "number",
            JsonType::String => "string",
            JsonType::Object => "object",
            JsonType::Array => "array",
            JsonType::Unknown => "unknown",
        }
    }
}
```

---

## 7. Shape Identity

Shape grain:

```text
canonical_path + site_path + schema_path + field_set_hash + type_set_hash
```

`field_set_hash` is based on sorted field names.

`type_set_hash` is based on sorted `(field_name, observed_type)` pairs for the current object occurrence.

`src/shape/id.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeKey {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_set_hash: String,
    pub type_set_hash: String,
}

#[derive(Debug, Clone)]
pub struct ShapeFacts {
    pub shape_id: String,
    pub field_set_json: String,
    pub type_set_json: String,
}

pub fn compute_shape_facts(
    canonical_path: &str,
    site_path: Option<&str>,
    schema_path: &str,
    object: &serde_json::Map<String, serde_json::Value>,
) -> ShapeFacts {
    let mut field_names: Vec<&str> = object.keys().map(String::as_str).collect();
    field_names.sort_unstable();

    let field_set_json = serde_json::to_string(&field_names).expect("field names are serializable");
    let field_set_hash = crate::util::hash::stable_hex(field_set_json.as_bytes());

    let mut type_pairs: Vec<(String, String)> = object.iter()
        .map(|(k, v)| (k.clone(), crate::util::json_type::JsonType::from_value(v).as_sql_str().to_string()))
        .collect();
    type_pairs.sort_unstable();

    let type_set_json = serde_json::to_string(&type_pairs).expect("type pairs are serializable");
    let type_set_hash = crate::util::hash::stable_hex(type_set_json.as_bytes());

    let shape_input = format!(
        "{canonical_path}\x1f{}\x1f{schema_path}\x1f{field_set_hash}\x1f{type_set_hash}",
        site_path.unwrap_or("")
    );

    ShapeFacts {
        shape_id: crate::util::hash::stable_hex(shape_input.as_bytes()),
        field_set_json,
        type_set_json,
    }
}
```

Performance note: this simple implementation is acceptable for early phases. Later optimize with interned field/type tokens and materialize JSON only at flush.

---

## 8. Shape Accumulator

`src/shape/accumulator.rs`:

```rust
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct ShapeAccumulator {
    shapes: HashMap<String, ShapeRow>,
    object_samples: ObjectSampleAccumulator,
}

#[derive(Debug, Clone)]
pub struct ShapeRow {
    pub shape_id: String,
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_set_hash: String,
    pub type_set_hash: String,
    pub field_set_json: String,
    pub type_set_json: String,
    pub object_count: u64,
    pub first_seen_document_index: Option<u64>,
    pub first_seen_path: Option<String>,
}

impl ShapeAccumulator {
    pub fn observe_object(
        &mut self,
        document_index: u64,
        source_path: &str,
        context: &crate::refs::resolver::ResolvedObjectContext,
        object: &serde_json::Map<String, serde_json::Value>,
        config: &crate::config::SamplingConfig,
    ) {
        let facts = crate::shape::id::compute_shape_facts(
            &context.canonical_path,
            context.site_path.as_deref(),
            &context.schema_path,
            object,
        );

        let row = self.shapes.entry(facts.shape_id.clone()).or_insert_with(|| ShapeRow {
            shape_id: facts.shape_id.clone(),
            canonical_path: context.canonical_path.clone(),
            site_path: context.site_path.clone(),
            schema_path: context.schema_path.clone(),
            field_set_hash: crate::util::hash::stable_hex(facts.field_set_json.as_bytes()),
            type_set_hash: crate::util::hash::stable_hex(facts.type_set_json.as_bytes()),
            field_set_json: facts.field_set_json.clone(),
            type_set_json: facts.type_set_json.clone(),
            object_count: 0,
            first_seen_document_index: Some(document_index),
            first_seen_path: Some(source_path.to_string()),
        });

        row.object_count += 1;

        self.object_samples.observe(document_index, source_path, context, &facts, object, config);
    }
}
```

Avoid recalculating hashes from JSON strings in final code; return hashes from `compute_shape_facts`.

---

## 9. Object Sample Scopes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleScope {
    CanonicalPath,
    SitePath,
    FieldSet,
    TypeSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectSampleKind {
    FirstSeen,
    FirstNonEmpty,
    PrioritySample,
}
```

Sample keys:

```rust
pub fn object_sample_key(
    scope: SampleScope,
    canonical_path: &str,
    site_path: Option<&str>,
    field_set_hash: Option<&str>,
    type_set_hash: Option<&str>,
) -> String {
    match scope {
        SampleScope::CanonicalPath => canonical_path.to_string(),
        SampleScope::SitePath => format!("{canonical_path}\x1f{}", site_path.unwrap_or("")),
        SampleScope::FieldSet => format!(
            "{canonical_path}\x1f{}\x1f{}",
            site_path.unwrap_or(""),
            field_set_hash.unwrap_or("")
        ),
        SampleScope::TypeSet => format!(
            "{canonical_path}\x1f{}\x1f{}\x1f{}",
            site_path.unwrap_or(""),
            field_set_hash.unwrap_or(""),
            type_set_hash.unwrap_or("")
        ),
    }
}
```

---

## 10. Empty / Non-Empty Rules

For object samples:

```text
{}      empty object
[]      empty array
null    empty value
""      non-empty string
0       non-empty number
false   non-empty boolean
{"a": null} non-empty object
[null] non-empty array
```

RDB reverse engineering rationale:

```text
{}, [], and null are weak for logical type/structure inference.

"" is not weak in the same way. It proves observed_type = string and may indicate
a sentinel/default/null-substitute pattern. It is eligible for first_non_empty.
```

For object-level candidates, the primary check is:

```rust
pub fn object_is_structurally_non_empty(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    !obj.is_empty()
}
```

For value-level candidates, use:

```rust
pub fn value_is_non_empty(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Object(obj) => !obj.is_empty(),
        serde_json::Value::Array(arr) => !arr.is_empty(),
        serde_json::Value::String(_) => true,
        serde_json::Value::Bool(_) => true,
        serde_json::Value::Number(_) => true,
    }
}
```

---

## 11. ObjectSampleAccumulator

Use chunk-local state.

```rust
use std::collections::{HashMap, BinaryHeap};

pub struct ObjectSampleAccumulator {
    once_seen: HashMap<(SampleScope, String, ObjectSampleKind), ()>,
    priority: HashMap<(SampleScope, String), TopK<ObjectSampleCandidate>>,
    pending_rows: Vec<ObjectSampleRow>,
    flush_row_limit: usize,
}

pub struct ObjectSampleCandidate {
    pub priority: u64,
    pub row: ObjectSampleRow,
}

pub struct ObjectSampleRow {
    pub object_sample_id: String,
    pub sample_scope: SampleScope,
    pub sample_key: String,
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: Option<String>,
    pub field_set_hash: Option<String>,
    pub type_set_hash: Option<String>,
    pub shape_id: Option<String>,
    pub sample_kind: ObjectSampleKind,
    pub document_index: u64,
    pub source_path: String,
    pub sample_json: String,
    pub sample_json_truncated: bool,
    pub sample_is_empty_object: bool,
    pub sample_is_empty_array: bool,
    pub sample_priority: Option<u64>,
    pub sample_rank: Option<u32>,
}
```

`once_seen` is chunk-local only. SQLite uniqueness enforces global first_seen / first_non_empty.

Algorithm:

```rust
impl ObjectSampleAccumulator {
    pub fn observe(
        &mut self,
        document_index: u64,
        source_path: &str,
        context: &crate::refs::resolver::ResolvedObjectContext,
        shape: &crate::shape::id::ShapeFacts,
        object: &serde_json::Map<String, serde_json::Value>,
        config: &crate::config::SamplingConfig,
    ) {
        for scope in [
            SampleScope::CanonicalPath,
            SampleScope::SitePath,
            SampleScope::FieldSet,
            SampleScope::TypeSet,
        ] {
            let key = object_sample_key(
                scope,
                &context.canonical_path,
                context.site_path.as_deref(),
                Some(&shape.field_set_hash),
                Some(&shape.type_set_hash),
            );

            self.enqueue_once(scope, &key, ObjectSampleKind::FirstSeen, ...);

            if object_is_structurally_non_empty(object) {
                self.enqueue_once(scope, &key, ObjectSampleKind::FirstNonEmpty, ...);
            }

            self.enqueue_priority(scope, &key, object, config);
        }
    }
}
```

`enqueue_once` uses `INSERT OR IGNORE` later, so duplicate first_seen candidates across chunks are safe.

---

## 12. Deterministic Priority Sampling

Use deterministic priority, not classic reservoir state.

```rust
pub fn sample_priority(sample_scope: SampleScope, sample_key: &str, document_index: u64, source_path: &str) -> u64 {
    let input = format!("{:?}\x1f{sample_key}\x1f{document_index}\x1f{source_path}", sample_scope);
    crate::util::hash::stable_u64(input.as_bytes())
}
```

Lower priority wins. Keep top K lowest priorities per `(sample_scope, sample_key)`.

```rust
pub struct TopK<T> {
    limit: usize,
    // store max-heap by priority so the worst retained item is popped first
    heap: BinaryHeap<T>,
}
```

At chunk flush:

```text
1. write first_seen / first_non_empty with INSERT OR IGNORE
2. write priority_sample rows
3. run prune SQL to keep configured limit per sample key
4. clear chunk-local sample state
```

---

## 13. Heterogeneous Object Arrays

v0.1.0 handles heterogeneous object arrays through existing shape rows.

Example:

```json
{
  "items": [
    {"id": 1, "type": "A", "amount": 100},
    {"id": 2, "type": "B", "error": "invalid"},
    {"id": 3, "type": "A", "amount": "200"}
  ]
}
```

Expected:

```text
same array site:
  shape 1: field_set [amount, id, type], amount integer
  shape 2: field_set [error, id, type], error string
  shape 3: field_set [amount, id, type], amount string
```

Implementation rule:

```text
- When traversal sees an object inside an array, emit visit_object normally.
- Refs resolver should resolve the object element path to canonical/site/schema context.
- Shape identity distinguishes field_set_hash and type_set_hash.
- Do not collapse heterogeneous elements into one array-level row.
- Do not create prof_array_* tables.
```

Scalar-only and array-specific profiling is deferred.

---

## 14. Phase 4 Tests

```text
tests/json_scan.rs
  - counts documents, objects, arrays, scalars
  - object path tracking works

tests/jsonl_scan.rs
  - each non-empty line becomes a document
  - blank lines are ignored or rejected according to CLI contract; pick one and document it
  - source summary document_count is correct

tests/heterogeneous_array.rs
  - object elements inside arrays emit object events
```

Prefer ignoring blank JSONL lines if consistent with the project’s JSONL policy; if not, fail clearly.

---

## 15. Phase 5 Tests

```text
tests/shape_identity.rs
  - same field_set, different type_set -> different shape_id
  - different field_set -> different shape_id
  - same shape input -> stable shape_id

tests/object_samples.rs
  - every canonical/site/field_set/type_set key has first_seen
  - empty first_seen does not block first_non_empty
  - "{}" first_seen followed by non-empty object creates first_non_empty
  - priority_sample rows are bounded

tests/heterogeneous_array.rs
  - one array site can produce multiple prof_shape rows
  - no prof_array_* table exists
```

---

## 16. Commits

Phase 4:

```bash
git add src/scan src/util/json_type.rs tests/json_scan.rs tests/jsonl_scan.rs
git commit -m "feat(scan): stream JSON and JSONL source files"
```

Phase 5:

```bash
git add src/shape src/util/hash.rs src/util/truncate.rs tests/shape_identity.rs tests/object_samples.rs tests/heterogeneous_array.rs
git commit -m "feat(shape): collect shape facts and object samples"
```
