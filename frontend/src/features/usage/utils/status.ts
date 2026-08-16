import type { RequestStatus, UsageRecord } from '../types'

export type TimelineFinalStatus = 'success' | 'failed' | 'streaming' | 'pending' | 'cancelled'

type RequestStatusLike = RequestStatus | string | null | undefined

type UsageFailureSignal = {
  status_code?: number | null
  error_message?: string | null
  image_progress?: {
    phase?: string | null
  } | null
}

type UsageDisplayStatusRecord = UsageFailureSignal & {
  status?: RequestStatusLike
  first_byte_time_ms?: number | null
}

function hasLegacyFailureSignal(
  record: UsageFailureSignal
): boolean {
  return (typeof record.status_code === 'number' && record.status_code >= 400) ||
    (typeof record.error_message === 'string' && record.error_message.trim().length > 0)
}

function hasImageProgressFailureSignal(
  record: UsageFailureSignal
): boolean {
  return typeof record.image_progress?.phase === 'string' &&
    record.image_progress.phase.trim().toLowerCase() === 'failed'
}

function hasAnyFailureSignal(
  record: UsageFailureSignal
): boolean {
  return hasLegacyFailureSignal(record) || hasImageProgressFailureSignal(record)
}

export function hasUsageFallback(
  record: Pick<UsageRecord, 'has_fallback'>
): boolean {
  return record.has_fallback === true
}

export function hasUsageRetry(
  record: Pick<UsageRecord, 'has_retry'>
): boolean {
  return record.has_retry === true
}

export function isUsageUpstreamStream(
  record: Pick<
    UsageRecord,
    | 'is_stream'
    | 'upstream_is_stream'
  >
): boolean {
  return typeof record.upstream_is_stream === 'boolean'
    ? record.upstream_is_stream
    : record.is_stream
}

export function formatUsageStreamLabel(
  record: Pick<
    UsageRecord,
    | 'is_stream'
    | 'upstream_is_stream'
  >
): string {
  return isUsageUpstreamStream(record) ? '流式' : '标准'
}

function hasTerminalSuccessStatusCode(
  record: UsageFailureSignal
): boolean {
  return typeof record.status_code === 'number' &&
    record.status_code >= 200 &&
    record.status_code < 300
}

export function isUsageRecordFailed(record: UsageFailureSignal & Pick<UsageRecord, 'status'>): boolean {
  const status = typeof record.status === 'string' ? record.status.trim().toLowerCase() : ''
  if (status) {
    if (status === 'pending' || status === 'streaming') {
      return !hasTerminalSuccessStatusCode(record) && hasAnyFailureSignal(record)
    }
    if (status === 'cancelled') {
      return false
    }
    if (status === 'completed') {
      return false
    }
    if (status === 'failed') {
      return true
    }
  }
  if (hasTerminalSuccessStatusCode(record)) {
    return false
  }
  if (status) {
    return status === 'failed'
  }
  return hasAnyFailureSignal(record)
}

export function isUsageRecordSuccessful(record: UsageFailureSignal & Pick<UsageRecord, 'status'>): boolean {
  const status = typeof record.status === 'string' ? record.status.trim().toLowerCase() : ''
  if (status) {
    if (status === 'completed') {
      return true
    }
    if (status === 'failed') {
      return false
    }
    return false
  }
  if (hasTerminalSuccessStatusCode(record)) {
    return true
  }
  return !hasAnyFailureSignal(record)
}

export function normalizeRequestStatus(status: RequestStatusLike): RequestStatus | undefined {
  const normalized = typeof status === 'string' ? status.trim().toLowerCase() : ''
  switch (normalized) {
    case 'pending':
    case 'streaming':
    case 'completed':
    case 'failed':
    case 'cancelled':
      return normalized
    default:
      return undefined
  }
}

export function resolveDisplayRequestStatus(record: UsageDisplayStatusRecord): RequestStatus | undefined {
  const status = normalizeRequestStatus(record.status)
  if ((status === 'pending' || status === 'streaming') &&
    !hasTerminalSuccessStatusCode(record) &&
    hasAnyFailureSignal(record)) {
    return 'failed'
  }
  if (status === 'streaming' && record.first_byte_time_ms == null) {
    return 'pending'
  }
  return status ?? (hasAnyFailureSignal(record) ? 'failed' : undefined)
}

export function mapRequestStatusToTimelineStatus(
  status: RequestStatusLike
): TimelineFinalStatus | undefined {
  switch (normalizeRequestStatus(status)) {
    case 'completed':
      return 'success'
    case 'failed':
      return 'failed'
    case 'streaming':
      return 'streaming'
    case 'pending':
      return 'pending'
    case 'cancelled':
      return 'cancelled'
    default:
      return undefined
  }
}

function normalizeTimelineFinalStatus(status: string | null | undefined): TimelineFinalStatus | undefined {
  const normalized = typeof status === 'string' ? status.trim().toLowerCase() : ''
  switch (normalized) {
    case 'success':
    case 'failed':
    case 'streaming':
    case 'pending':
    case 'cancelled':
      return normalized
    default:
      return undefined
  }
}

export function resolveTimelineFinalStatus(params: {
  hasPendingCandidates?: boolean
  traceFinalStatus?: string | null
  requestStatus?: RequestStatusLike
  statusCode?: number
}): TimelineFinalStatus {
  const hasTerminalSuccessStatusCode = typeof params.statusCode === 'number'
    ? params.statusCode >= 200 && params.statusCode < 300
    : undefined

  const requestStatus = mapRequestStatusToTimelineStatus(params.requestStatus)
  if (requestStatus === 'success' || requestStatus === 'failed' || requestStatus === 'cancelled') {
    if (requestStatus === 'success' && hasTerminalSuccessStatusCode === false) {
      return 'failed'
    }
    return requestStatus
  }
  if (requestStatus === 'pending' || requestStatus === 'streaming') {
    return requestStatus
  }

  const traceStatus = normalizeTimelineFinalStatus(params.traceFinalStatus)
  if (traceStatus === 'success' || traceStatus === 'failed' || traceStatus === 'cancelled') {
    if (traceStatus === 'success' && hasTerminalSuccessStatusCode === false) {
      return 'failed'
    }
    return traceStatus
  }

  if (params.hasPendingCandidates) {
    return 'pending'
  }

  if (hasTerminalSuccessStatusCode !== undefined) {
    return hasTerminalSuccessStatusCode ? 'success' : 'failed'
  }

  if (traceStatus) {
    return traceStatus
  }

  if (requestStatus) {
    return requestStatus
  }

  return 'pending'
}
