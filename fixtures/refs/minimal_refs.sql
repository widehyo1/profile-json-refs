CREATE TABLE schema_paths (
    schema_path TEXT NOT NULL,
    object_path TEXT PRIMARY KEY
);

CREATE TABLE array_index_refs (
    array_path TEXT NOT NULL,
    array_index_path TEXT NOT NULL,
    schema_path TEXT NOT NULL,
    PRIMARY KEY (array_path, array_index_path)
);

CREATE TABLE schema_definitions (
    schema_path TEXT PRIMARY KEY,
    schema_kind TEXT NOT NULL,
    schema_json TEXT NOT NULL
);

CREATE TABLE schema_object_counts (
    schema_path TEXT PRIMARY KEY,
    object_count INTEGER NOT NULL CHECK (object_count > 0)
);

CREATE TABLE schema_field_counts (
    schema_path TEXT NOT NULL,
    field_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    field_count INTEGER NOT NULL CHECK (field_count > 0),
    PRIMARY KEY (schema_path, field_name, field_type)
);

CREATE TABLE schema_site_counts (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    object_count INTEGER NOT NULL CHECK (object_count > 0),
    PRIMARY KEY (schema_path, site_path, site_kind)
);

CREATE TABLE schema_site_field_counts (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    field_name TEXT NOT NULL,
    schema_field_type TEXT NOT NULL,
    present_count INTEGER NOT NULL CHECK (present_count >= 0),
    missing_count INTEGER NOT NULL CHECK (missing_count >= 0),
    PRIMARY KEY (schema_path, site_path, site_kind, field_name)
);

CREATE TABLE schema_site_presence_shapes (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    present_fields_hash TEXT NOT NULL,
    present_fields_json TEXT NOT NULL,
    missing_fields_json TEXT NOT NULL,
    object_count INTEGER NOT NULL CHECK (object_count > 0),
    first_array_index_path TEXT,
    PRIMARY KEY (schema_path, site_path, site_kind, present_fields_hash)
);

CREATE TABLE schema_site_presence_shape_limits (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    observed_shape_count INTEGER NOT NULL CHECK (observed_shape_count >= 0),
    stored_shape_count INTEGER NOT NULL CHECK (stored_shape_count >= 0),
    truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
    PRIMARY KEY (schema_path, site_path, site_kind)
);

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/root.json', '$');

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/root_array_item.json', '$[]');

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/documents_item.json', '$.documents[]');

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/items_item.json', '$.items[]');

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/payload.json', '$.payload');

INSERT INTO schema_paths(schema_path, object_path)
VALUES ('refs/payload_nested.json', '$.payload.nested');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/root.json', 'object', '{}');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/root_array_item.json', 'object', '{}');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/documents_item.json', 'object', '{}');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/items_item.json', 'object', '{}');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/payload.json', 'object', '{}');

INSERT INTO schema_definitions(schema_path, schema_kind, schema_json)
VALUES ('refs/payload_nested.json', 'object', '{}');

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/root.json', 1);

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/root_array_item.json', 1);

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/documents_item.json', 1);

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/items_item.json', 1);

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/payload.json', 1);

INSERT INTO schema_object_counts(schema_path, object_count)
VALUES ('refs/payload_nested.json', 1);

INSERT INTO schema_field_counts(schema_path, field_name, field_type, field_count)
VALUES ('refs/root.json', 'id', 'number', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/root.json', '$', 'object', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/root_array_item.json', '$[0]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/root_array_item.json', '$[1]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/root_array_item.json', '$[2]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/documents_item.json', '$.documents[0]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/documents_item.json', '$.documents[1]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/items_item.json', '$.items[0]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/items_item.json', '$.items[1]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/items_item.json', '$.items[2]', 'array_item', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/payload.json', '$.payload', 'object', 1);

INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count)
VALUES ('refs/payload_nested.json', '$.payload.nested', 'object', 1);

INSERT INTO schema_site_field_counts(
    schema_path,
    site_path,
    site_kind,
    field_name,
    schema_field_type,
    present_count,
    missing_count
)
VALUES ('refs/root.json', '$', 'object', 'id', 'number', 1, 0);

INSERT INTO schema_site_presence_shapes(
    schema_path,
    site_path,
    site_kind,
    present_fields_hash,
    present_fields_json,
    missing_fields_json,
    object_count,
    first_array_index_path
)
VALUES ('refs/root.json', '$', 'object', 'hash-id', '["id"]', '[]', 1, NULL);

INSERT INTO schema_site_presence_shape_limits(
    schema_path,
    site_path,
    site_kind,
    observed_shape_count,
    stored_shape_count,
    truncated
)
VALUES ('refs/root.json', '$', 'object', 1, 1, 0);
