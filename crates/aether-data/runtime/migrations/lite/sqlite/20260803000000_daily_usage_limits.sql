CREATE TABLE IF NOT EXISTS lite_api_key_daily_usage_limits (
    api_key_id TEXT PRIMARY KEY NOT NULL,
    daily_usage_limit_usd REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS lite_user_group_daily_usage_limits (
    user_group_id TEXT PRIMARY KEY NOT NULL,
    daily_usage_limit_usd REAL,
    daily_usage_limit_mode TEXT NOT NULL DEFAULT 'inherit'
        CHECK (daily_usage_limit_mode IN ('inherit', 'system', 'custom'))
);

INSERT OR IGNORE INTO lite_user_group_daily_usage_limits (
    user_group_id,
    daily_usage_limit_usd,
    daily_usage_limit_mode
)
SELECT
    '00000000-0000-0000-0000-000000000001',
    NULL,
    'system'
WHERE EXISTS (
    SELECT 1
    FROM user_groups
    WHERE id = '00000000-0000-0000-0000-000000000001'
);
