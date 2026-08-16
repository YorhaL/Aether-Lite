CREATE TABLE IF NOT EXISTS lite_admission_policies (
    scope_kind TEXT NOT NULL
        CHECK (scope_kind IN ('system', 'user_group', 'user', 'api_key')),
    subject_id TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    document TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (scope_kind, subject_id)
);
