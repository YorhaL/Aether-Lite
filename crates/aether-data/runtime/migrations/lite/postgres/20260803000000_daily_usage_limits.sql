CREATE SCHEMA IF NOT EXISTS aether_lite;

CREATE TABLE IF NOT EXISTS aether_lite.api_key_daily_usage_limits (
    api_key_id text PRIMARY KEY,
    daily_usage_limit_usd double precision NOT NULL
);

CREATE TABLE IF NOT EXISTS aether_lite.user_group_daily_usage_limits (
    user_group_id text PRIMARY KEY,
    daily_usage_limit_usd double precision,
    daily_usage_limit_mode text NOT NULL DEFAULT 'inherit',
    CONSTRAINT user_group_daily_usage_limit_mode_check
        CHECK (daily_usage_limit_mode IN ('inherit', 'system', 'custom'))
);

INSERT INTO aether_lite.user_group_daily_usage_limits (
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
    FROM public.user_groups
    WHERE id = '00000000-0000-0000-0000-000000000001'
)
ON CONFLICT (user_group_id) DO NOTHING;
