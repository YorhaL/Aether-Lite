CREATE SCHEMA IF NOT EXISTS aether_lite;

CREATE TABLE IF NOT EXISTS aether_lite.admission_policies (
    scope_kind text NOT NULL,
    subject_id text NOT NULL,
    schema_version integer NOT NULL,
    document jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (scope_kind, subject_id),
    CONSTRAINT admission_policy_scope_kind_check
        CHECK (scope_kind IN ('system', 'user_group', 'user', 'api_key')),
    CONSTRAINT admission_policy_schema_version_positive
        CHECK (schema_version > 0)
);
