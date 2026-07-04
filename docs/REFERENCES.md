# References

This document records the upstream references that `profile-json-refs` depends on.

`profile-json-refs` is downstream of `dump-json-refs`. It consumes the original JSON/JSONL source and `refs/schemas.sqlite`, then writes `profile.sqlite`.

---

## Upstream Repository

```text
https://github.com/widehyo1/dump-json-refs
```

`dump-json-refs` owns structural refs extraction.

`profile-json-refs` must not depend on undocumented implementation details of `dump-json-refs`. It should consume only the documented SQLite output contract.

---

## Required Upstream Artifact

```text
refs/schemas.sqlite
```

The default CLI path is:

```text
refs/schemas.sqlite
```

The path may be overridden with:

```bash
profile-json-refs <INPUT_FILE> --refs <FILE>
```

---

## Required Refs Tables

`profile-json-refs` v0.1.0 expects `refs/schemas.sqlite` to provide structural facts equivalent to the following table groups:

```text
schema paths
schema definitions
schema object counts
schema field counts
schema site counts
schema site field counts
schema site presence shapes
schema site presence shape limits
array / relation refs when needed for navigation
```

Implementation names should follow the upstream SQLite contract. At the time of this spec, the important site-level tables are:

```text
schema_site_counts
schema_site_field_counts
schema_site_presence_shapes
schema_site_presence_shape_limits
```

These site-level tables are structural seeds for profile shape grouping.

---

## Refs-to-Profile Mapping

`profile-json-refs` uses refs data to anchor source-scan observations to:

```text
canonical_path
site_path
schema_path
field_name
observed_type
field combination / presence shape
array object element context when available
```

`profile-json-refs` adds value-level facts on top of these structural refs.

---

## Truncated Presence Shapes

If the upstream refs database indicates that presence shapes were truncated, profile generation should continue.

The profile output must not pretend that missing shape identities are available.

Recommended behavior:

```text
- emit a warning to stderr
- continue scanning the source
- write usable profile facts where possible
```

Warnings are not stored in `profile.sqlite`.

---

## Stdin Difference

`dump-json-refs` may support stdin.

`profile-json-refs` v0.1.0 does not support stdin. It requires a filesystem input path so the source file and refs database relationship remains explicit.
