-- Preserve the explicit access granted by the original #741 implementation,
-- which represented Codex Live as openai:responses.  Runtime permission
-- matching remains strict after this one-time data upgrade.

UPDATE users
SET allowed_api_formats = json_insert(allowed_api_formats, '$[#]', 'codex:live')
WHERE LOWER(TRIM(COALESCE(allowed_api_formats_mode, ''))) = 'specific'
  AND json_valid(allowed_api_formats)
  AND json_type(CASE WHEN json_valid(allowed_api_formats) THEN allowed_api_formats END) = 'array'
  AND EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(users.allowed_api_formats) THEN users.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'openai:responses'
  )
  AND NOT EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(users.allowed_api_formats) THEN users.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'codex:live'
  );

UPDATE user_groups
SET allowed_api_formats = json_insert(allowed_api_formats, '$[#]', 'codex:live')
WHERE LOWER(TRIM(COALESCE(allowed_api_formats_mode, ''))) = 'specific'
  AND json_valid(allowed_api_formats)
  AND json_type(CASE WHEN json_valid(allowed_api_formats) THEN allowed_api_formats END) = 'array'
  AND EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(user_groups.allowed_api_formats) THEN user_groups.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'openai:responses'
  )
  AND NOT EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(user_groups.allowed_api_formats) THEN user_groups.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'codex:live'
  );

UPDATE api_keys
SET allowed_api_formats = json_insert(allowed_api_formats, '$[#]', 'codex:live')
WHERE json_valid(allowed_api_formats)
  AND json_type(CASE WHEN json_valid(allowed_api_formats) THEN allowed_api_formats END) = 'array'
  AND EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(api_keys.allowed_api_formats) THEN api_keys.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'openai:responses'
  )
  AND NOT EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(api_keys.allowed_api_formats) THEN api_keys.allowed_api_formats ELSE '[]' END
    )
    WHERE value = 'codex:live'
  );

UPDATE provider_api_keys
SET
  api_formats = json_insert(api_formats, '$[#]', 'codex:live'),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE provider_id IN (
    SELECT id FROM providers
    WHERE LOWER(TRIM(COALESCE(provider_type, ''))) = 'codex'
  )
  AND json_valid(api_formats)
  AND json_type(CASE WHEN json_valid(api_formats) THEN api_formats END) = 'array'
  AND EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(provider_api_keys.api_formats) THEN provider_api_keys.api_formats ELSE '[]' END
    )
    WHERE value = 'openai:responses'
  )
  AND NOT EXISTS (
    SELECT 1 FROM json_each(
      CASE WHEN json_valid(provider_api_keys.api_formats) THEN provider_api_keys.api_formats ELSE '[]' END
    )
    WHERE value = 'codex:live'
  );

UPDATE provider_api_keys
SET
  auth_type_by_format = json_set(
    auth_type_by_format,
    '$."codex:live"',
    json_extract(auth_type_by_format, '$."openai:responses"')
  ),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE provider_id IN (
    SELECT id FROM providers
    WHERE LOWER(TRIM(COALESCE(provider_type, ''))) = 'codex'
  )
  AND json_valid(auth_type_by_format)
  AND json_type(CASE WHEN json_valid(auth_type_by_format) THEN auth_type_by_format END) = 'object'
  AND json_type(
    CASE WHEN json_valid(auth_type_by_format) THEN auth_type_by_format END,
    '$."openai:responses"'
  ) IS NOT NULL
  AND json_type(
    CASE WHEN json_valid(auth_type_by_format) THEN auth_type_by_format END,
    '$."codex:live"'
  ) IS NULL;

UPDATE provider_api_keys
SET
  allow_auth_channel_mismatch_formats = json_insert(
    allow_auth_channel_mismatch_formats,
    '$[#]',
    'codex:live'
  ),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE provider_id IN (
    SELECT id FROM providers
    WHERE LOWER(TRIM(COALESCE(provider_type, ''))) = 'codex'
  )
  AND json_valid(allow_auth_channel_mismatch_formats)
  AND json_type(
    CASE
      WHEN json_valid(allow_auth_channel_mismatch_formats)
      THEN allow_auth_channel_mismatch_formats
    END
  ) = 'array'
  AND EXISTS (
    SELECT 1 FROM json_each(
      CASE
        WHEN json_valid(provider_api_keys.allow_auth_channel_mismatch_formats)
        THEN provider_api_keys.allow_auth_channel_mismatch_formats
        ELSE '[]'
      END
    )
    WHERE value = 'openai:responses'
  )
  AND NOT EXISTS (
    SELECT 1 FROM json_each(
      CASE
        WHEN json_valid(provider_api_keys.allow_auth_channel_mismatch_formats)
        THEN provider_api_keys.allow_auth_channel_mismatch_formats
        ELSE '[]'
      END
    )
    WHERE value = 'codex:live'
  );

UPDATE provider_api_keys
SET
  rate_multipliers = json_set(
    rate_multipliers,
    '$."codex:live"',
    json_extract(rate_multipliers, '$."openai:responses"')
  ),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE provider_id IN (
    SELECT id FROM providers
    WHERE LOWER(TRIM(COALESCE(provider_type, ''))) = 'codex'
  )
  AND json_valid(rate_multipliers)
  AND json_type(CASE WHEN json_valid(rate_multipliers) THEN rate_multipliers END) = 'object'
  AND json_type(
    CASE WHEN json_valid(rate_multipliers) THEN rate_multipliers END,
    '$."openai:responses"'
  ) IS NOT NULL
  AND json_type(
    CASE WHEN json_valid(rate_multipliers) THEN rate_multipliers END,
    '$."codex:live"'
  ) IS NULL;

UPDATE provider_api_keys
SET
  global_priority_by_format = json_set(
    global_priority_by_format,
    '$."codex:live"',
    json_extract(global_priority_by_format, '$."openai:responses"')
  ),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE provider_id IN (
    SELECT id FROM providers
    WHERE LOWER(TRIM(COALESCE(provider_type, ''))) = 'codex'
  )
  AND json_valid(global_priority_by_format)
  AND json_type(
    CASE WHEN json_valid(global_priority_by_format) THEN global_priority_by_format END
  ) = 'object'
  AND json_type(
    CASE WHEN json_valid(global_priority_by_format) THEN global_priority_by_format END,
    '$."openai:responses"'
  ) IS NOT NULL
  AND json_type(
    CASE WHEN json_valid(global_priority_by_format) THEN global_priority_by_format END,
    '$."codex:live"'
  ) IS NULL;
