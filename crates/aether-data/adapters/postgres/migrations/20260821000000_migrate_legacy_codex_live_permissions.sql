-- #741 exposed Codex Frameless Live through the openai:responses permission
-- bucket.  Codex Live is now a first-class format, so preserve the access that
-- administrators had already granted while keeping the runtime permission
-- relationship strict after this one-time migration.

UPDATE public.users
SET allowed_api_formats = (allowed_api_formats::jsonb || '["codex:live"]'::jsonb)::json
WHERE LOWER(BTRIM(COALESCE(allowed_api_formats_mode, ''))) = 'specific'
  AND json_typeof(allowed_api_formats) = 'array'
  AND allowed_api_formats::jsonb ? 'openai:responses'
  AND NOT (allowed_api_formats::jsonb ? 'codex:live');

UPDATE public.user_groups
SET allowed_api_formats = (allowed_api_formats::jsonb || '["codex:live"]'::jsonb)::json
WHERE LOWER(BTRIM(COALESCE(allowed_api_formats_mode, ''))) = 'specific'
  AND json_typeof(allowed_api_formats) = 'array'
  AND allowed_api_formats::jsonb ? 'openai:responses'
  AND NOT (allowed_api_formats::jsonb ? 'codex:live');

UPDATE public.api_keys
SET allowed_api_formats = (allowed_api_formats::jsonb || '["codex:live"]'::jsonb)::json
WHERE json_typeof(allowed_api_formats) = 'array'
  AND allowed_api_formats::jsonb ? 'openai:responses'
  AND NOT (allowed_api_formats::jsonb ? 'codex:live');

UPDATE public.provider_api_keys AS provider_key
SET
  api_formats = (provider_key.api_formats::jsonb || '["codex:live"]'::jsonb)::json,
  updated_at = NOW()
FROM public.providers AS provider
WHERE provider.id = provider_key.provider_id
  AND LOWER(BTRIM(COALESCE(provider.provider_type, ''))) = 'codex'
  AND json_typeof(provider_key.api_formats) = 'array'
  AND provider_key.api_formats::jsonb ? 'openai:responses'
  AND NOT (provider_key.api_formats::jsonb ? 'codex:live');

UPDATE public.provider_api_keys AS provider_key
SET
  auth_type_by_format = (
    provider_key.auth_type_by_format::jsonb
    || jsonb_build_object(
      'codex:live',
      provider_key.auth_type_by_format::jsonb -> 'openai:responses'
    )
  )::json,
  updated_at = NOW()
FROM public.providers AS provider
WHERE provider.id = provider_key.provider_id
  AND LOWER(BTRIM(COALESCE(provider.provider_type, ''))) = 'codex'
  AND json_typeof(provider_key.auth_type_by_format) = 'object'
  AND provider_key.auth_type_by_format::jsonb ? 'openai:responses'
  AND NOT (provider_key.auth_type_by_format::jsonb ? 'codex:live');

UPDATE public.provider_api_keys AS provider_key
SET
  allow_auth_channel_mismatch_formats = (
    provider_key.allow_auth_channel_mismatch_formats::jsonb || '["codex:live"]'::jsonb
  )::json,
  updated_at = NOW()
FROM public.providers AS provider
WHERE provider.id = provider_key.provider_id
  AND LOWER(BTRIM(COALESCE(provider.provider_type, ''))) = 'codex'
  AND json_typeof(provider_key.allow_auth_channel_mismatch_formats) = 'array'
  AND provider_key.allow_auth_channel_mismatch_formats::jsonb ? 'openai:responses'
  AND NOT (provider_key.allow_auth_channel_mismatch_formats::jsonb ? 'codex:live');

UPDATE public.provider_api_keys AS provider_key
SET
  rate_multipliers = (
    provider_key.rate_multipliers::jsonb
    || jsonb_build_object(
      'codex:live',
      provider_key.rate_multipliers::jsonb -> 'openai:responses'
    )
  )::json,
  updated_at = NOW()
FROM public.providers AS provider
WHERE provider.id = provider_key.provider_id
  AND LOWER(BTRIM(COALESCE(provider.provider_type, ''))) = 'codex'
  AND json_typeof(provider_key.rate_multipliers) = 'object'
  AND provider_key.rate_multipliers::jsonb ? 'openai:responses'
  AND NOT (provider_key.rate_multipliers::jsonb ? 'codex:live');

UPDATE public.provider_api_keys AS provider_key
SET
  global_priority_by_format = (
    provider_key.global_priority_by_format::jsonb
    || jsonb_build_object(
      'codex:live',
      provider_key.global_priority_by_format::jsonb -> 'openai:responses'
    )
  )::json,
  updated_at = NOW()
FROM public.providers AS provider
WHERE provider.id = provider_key.provider_id
  AND LOWER(BTRIM(COALESCE(provider.provider_type, ''))) = 'codex'
  AND json_typeof(provider_key.global_priority_by_format) = 'object'
  AND provider_key.global_priority_by_format::jsonb ? 'openai:responses'
  AND NOT (provider_key.global_priority_by_format::jsonb ? 'codex:live');
