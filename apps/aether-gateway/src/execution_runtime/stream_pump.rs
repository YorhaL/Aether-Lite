use std::collections::BTreeMap;
use std::future::Future;
use std::io::Error as IoError;
use std::time::{Duration, Instant};

use aether_contracts::{
    ExecutionError, ExecutionErrorKind, ExecutionPhase, ExecutionResponseObservation,
    ExecutionStreamTerminalSummary, ExecutionTelemetry, StreamFrame, StreamFramePayload,
    StreamFrameType,
};
use async_stream::stream;
use axum::body::Bytes;
use base64::Engine as _;
use futures_util::{Stream, StreamExt};
use http_body_util::BodyExt;
use serde_json::Value;
use tracing::warn;

use crate::ai_serving::api::StreamingStandardTerminalObserver;
use crate::execution_runtime::transport::{
    format_hyper_error_chain, format_wreq_upstream_request_error,
    stream_first_byte_timeout_message, DirectUpstreamResponse,
};
use crate::execution_runtime::DirectUpstreamStreamExecution;
use crate::GatewayError;
use aether_gateway_execution::stream::encode_stream_frame_ndjson;

const STREAM_USAGE_OBSERVER_MAX_LINE_BYTES: usize = 1024 * 1024;

pub(crate) fn build_direct_execution_frame_stream(
    execution: DirectUpstreamStreamExecution,
) -> impl Stream<Item = Result<Bytes, IoError>> + Send + 'static {
    stream! {
        let DirectUpstreamStreamExecution {
            request_id: _,
            candidate_id: _,
            status_code,
            headers,
            provider_api_format,
            stream_summary_report_context,
            prefetched_body,
            stream_precommit_committed: _,
            response,
            started_at,
            response_observation,
            stream_first_byte_timeout,
            upstream_target_permit,
        } = execution;
        let _upstream_target_permit = upstream_target_permit;

        let mut observer_context = stream_summary_report_context;
        if observer_context
            .get("provider_api_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            if let Some(object) = observer_context.as_object_mut() {
                object.insert(
                    "provider_api_format".to_string(),
                    Value::String(provider_api_format.clone()),
                );
            }
        }
        let mut stream_terminal_observer = StreamingStandardTerminalObserver::default();
        let mut observer_buffered = Vec::new();

        match encode_headers_frame(
            status_code,
            headers,
            &response_observation,
        ) {
            Ok(frame) => yield Ok(frame),
            Err(err) => {
                yield Err(err);
                return;
            }
        }

        let mut upstream_bytes = 0u64;
        let mut ttfb_ms = None;
        let mut first_chunk_telemetry_emitted = false;
        let mut prefetched_body_failed = false;
        for item in prefetched_body {
            match item {
                Ok(chunk) => {
                    if ttfb_ms.is_none() {
                        ttfb_ms = Some(started_at.elapsed().as_millis() as u64);
                    }
                    if !first_chunk_telemetry_emitted {
                        match encode_telemetry_frame(ttfb_ms, ttfb_ms, upstream_bytes) {
                            Ok(frame) => yield Ok(frame),
                            Err(err) => {
                                yield Err(err);
                                return;
                            }
                        }
                        first_chunk_telemetry_emitted = true;
                    }
                    upstream_bytes += chunk.len() as u64;
                    observe_stream_chunk(
                        &mut stream_terminal_observer,
                        &observer_context,
                        &mut observer_buffered,
                        chunk.as_ref(),
                    );
                    match encode_data_frame(&chunk) {
                        Ok(frame) => yield Ok(frame),
                        Err(err) => {
                            yield Err(err);
                            return;
                        }
                    }
                }
                Err(message) => {
                    warn!(
                        event_name = "stream_pump_body_read_error",
                        log_type = "ops",
                        status_code,
                        upstream_bytes,
                        error = %message,
                        "upstream body stream read error"
                    );
                    match encode_error_frame(message) {
                        Ok(frame) => yield Ok(frame),
                        Err(encode_err) => {
                            yield Err(encode_err);
                            return;
                        }
                    }
                    prefetched_body_failed = true;
                    break;
                }
            }
        }
        if !prefetched_body_failed {
        match response {
            DirectUpstreamResponse::Reqwest(response) => {
                let mut bytes_stream = response.bytes_stream();
                loop {
                    let item = if ttfb_ms.is_none() {
                        match await_stream_first_byte(
                            bytes_stream.next(),
                            started_at,
                            stream_first_byte_timeout,
                        )
                        .await
                        {
                            Ok(item) => item,
                            Err(timeout) => {
                                match encode_first_byte_timeout_frame(timeout) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                break;
                            }
                        }
                    } else {
                        bytes_stream.next().await
                    };
                    let Some(item) = item else {
                        break;
                    };
                    match item {
                        Ok(chunk) => {
                            if ttfb_ms.is_none() {
                                ttfb_ms = Some(started_at.elapsed().as_millis() as u64);
                            }
                            if !first_chunk_telemetry_emitted {
                                match encode_telemetry_frame(ttfb_ms, ttfb_ms, upstream_bytes) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                first_chunk_telemetry_emitted = true;
                            }
                            upstream_bytes += chunk.len() as u64;
                            observe_stream_chunk(
                                &mut stream_terminal_observer,
                                &observer_context,
                                &mut observer_buffered,
                                chunk.as_ref(),
                            );
                            match encode_data_frame(&chunk) {
                                Ok(frame) => yield Ok(frame),
                                Err(err) => {
                                    yield Err(err);
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let message = format_error_chain(&err);
                            warn!(
                                event_name = "stream_pump_body_read_error",
                                log_type = "ops",
                                status_code,
                                upstream_bytes,
                                error = %message,
                                "upstream body stream read error"
                            );
                            match encode_error_frame(message) {
                                Ok(frame) => yield Ok(frame),
                                Err(encode_err) => {
                                    yield Err(encode_err);
                                    return;
                                }
                            }
                            break;
                        }
                    }
                }
            }
            DirectUpstreamResponse::HyperH2c(response) => {
                let mut bytes_stream = response.into_body().into_data_stream();
                loop {
                    let item = if ttfb_ms.is_none() {
                        match await_stream_first_byte(
                            bytes_stream.next(),
                            started_at,
                            stream_first_byte_timeout,
                        )
                        .await
                        {
                            Ok(item) => item,
                            Err(timeout) => {
                                match encode_first_byte_timeout_frame(timeout) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                break;
                            }
                        }
                    } else {
                        bytes_stream.next().await
                    };
                    let Some(item) = item else {
                        break;
                    };
                    match item {
                        Ok(chunk) => {
                            if ttfb_ms.is_none() {
                                ttfb_ms = Some(started_at.elapsed().as_millis() as u64);
                            }
                            if !first_chunk_telemetry_emitted {
                                match encode_telemetry_frame(ttfb_ms, ttfb_ms, upstream_bytes) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                first_chunk_telemetry_emitted = true;
                            }
                            upstream_bytes += chunk.len() as u64;
                            observe_stream_chunk(
                                &mut stream_terminal_observer,
                                &observer_context,
                                &mut observer_buffered,
                                chunk.as_ref(),
                            );
                            match encode_data_frame(&chunk) {
                                Ok(frame) => yield Ok(frame),
                                Err(err) => {
                                    yield Err(err);
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let message = format_hyper_error_chain(&err);
                            warn!(
                                event_name = "stream_pump_body_read_error",
                                log_type = "ops",
                                status_code,
                                upstream_bytes,
                                error = %message,
                                "upstream body stream read error"
                            );
                            match encode_error_frame(message) {
                                Ok(frame) => yield Ok(frame),
                                Err(encode_err) => {
                                    yield Err(encode_err);
                                    return;
                                }
                            }
                            break;
                        }
                    }
                }
            }
            DirectUpstreamResponse::BrowserWreq(response) => {
                let mut bytes_stream = response.bytes_stream();
                loop {
                    let item = if ttfb_ms.is_none() {
                        match await_stream_first_byte(
                            bytes_stream.next(),
                            started_at,
                            stream_first_byte_timeout,
                        )
                        .await
                        {
                            Ok(item) => item,
                            Err(timeout) => {
                                match encode_first_byte_timeout_frame(timeout) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                break;
                            }
                        }
                    } else {
                        bytes_stream.next().await
                    };
                    let Some(item) = item else {
                        break;
                    };
                    match item {
                        Ok(chunk) => {
                            if ttfb_ms.is_none() {
                                ttfb_ms = Some(started_at.elapsed().as_millis() as u64);
                            }
                            if !first_chunk_telemetry_emitted {
                                match encode_telemetry_frame(ttfb_ms, ttfb_ms, upstream_bytes) {
                                    Ok(frame) => yield Ok(frame),
                                    Err(err) => {
                                        yield Err(err);
                                        return;
                                    }
                                }
                                first_chunk_telemetry_emitted = true;
                            }
                            upstream_bytes += chunk.len() as u64;
                            observe_stream_chunk(
                                &mut stream_terminal_observer,
                                &observer_context,
                                &mut observer_buffered,
                                chunk.as_ref(),
                            );
                            match encode_data_frame(&chunk) {
                                Ok(frame) => yield Ok(frame),
                                Err(err) => {
                                    yield Err(err);
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let message = format_wreq_upstream_request_error(&err);
                            warn!(
                                event_name = "stream_pump_body_read_error",
                                log_type = "ops",
                                status_code,
                                upstream_bytes,
                                error = %message,
                                "upstream body stream read error"
                            );
                            match encode_error_frame(message) {
                                Ok(frame) => yield Ok(frame),
                                Err(encode_err) => {
                                    yield Err(encode_err);
                                    return;
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
        }
        let summary = finalize_stream_terminal_summary(
            &mut stream_terminal_observer,
            &observer_context,
            &mut observer_buffered,
        );

        match encode_telemetry_frame(
            ttfb_ms,
            Some(started_at.elapsed().as_millis() as u64),
            upstream_bytes,
        ) {
            Ok(frame) => yield Ok(frame),
            Err(err) => {
                yield Err(err);
                return;
            }
        }
        match encode_stream_frame_ndjson(&StreamFrame::eof_with_summary(summary)) {
            Ok(frame) => yield Ok(frame),
            Err(err) => yield Err(err),
        }
    }
}

fn encode_headers_frame(
    status_code: u16,
    headers: BTreeMap<String, String>,
    response_observation: &ExecutionResponseObservation,
) -> Result<Bytes, IoError> {
    encode_stream_frame_ndjson(&StreamFrame {
        frame_type: StreamFrameType::Headers,
        payload: StreamFramePayload::Headers {
            status_code,
            headers,
            response_observation: Some(response_observation.clone()),
        },
    })
}

fn encode_telemetry_frame(
    ttfb_ms: Option<u64>,
    elapsed_ms: Option<u64>,
    upstream_bytes: u64,
) -> Result<Bytes, IoError> {
    encode_stream_frame_ndjson(&StreamFrame {
        frame_type: StreamFrameType::Telemetry,
        payload: StreamFramePayload::Telemetry {
            telemetry: ExecutionTelemetry {
                ttfb_ms,
                elapsed_ms,
                upstream_bytes: Some(upstream_bytes),
            },
        },
    })
}

fn encode_data_frame(chunk: &Bytes) -> Result<Bytes, IoError> {
    encode_stream_frame_ndjson(&StreamFrame {
        frame_type: StreamFrameType::Data,
        payload: StreamFramePayload::Data {
            chunk_b64: Some(base64::engine::general_purpose::STANDARD.encode(chunk)),
            text: None,
        },
    })
}

fn encode_error_frame(message: String) -> Result<Bytes, IoError> {
    encode_stream_frame_ndjson(&StreamFrame {
        frame_type: StreamFrameType::Error,
        payload: StreamFramePayload::Error {
            error: ExecutionError {
                kind: ExecutionErrorKind::ProtocolError,
                phase: ExecutionPhase::StreamRead,
                message,
                upstream_status: None,
                retryable: true,
                failover_recommended: true,
            },
        },
    })
}

fn encode_first_byte_timeout_frame(timeout: Duration) -> Result<Bytes, IoError> {
    encode_stream_frame_ndjson(&StreamFrame {
        frame_type: StreamFrameType::Error,
        payload: StreamFramePayload::Error {
            error: ExecutionError {
                kind: ExecutionErrorKind::FirstByteTimeout,
                phase: ExecutionPhase::FirstByte,
                message: stream_first_byte_timeout_message(timeout),
                upstream_status: None,
                retryable: true,
                failover_recommended: true,
            },
        },
    })
}

async fn await_stream_first_byte<T, F>(
    future: F,
    started_at: Instant,
    timeout: Option<Duration>,
) -> Result<T, Duration>
where
    F: Future<Output = T>,
{
    let Some(timeout) = timeout else {
        return Ok(future.await);
    };
    let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
        return Err(timeout);
    };
    if remaining.is_zero() {
        return Err(timeout);
    }
    tokio::time::timeout(remaining, future)
        .await
        .map_err(|_| timeout)
}

fn format_error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn observe_stream_chunk(
    observer: &mut StreamingStandardTerminalObserver,
    report_context: &Value,
    observer_buffered: &mut Vec<u8>,
    chunk: &[u8],
) {
    observe_stream_bytes(observer, report_context, observer_buffered, chunk);
}

fn finalize_stream_terminal_summary(
    observer: &mut StreamingStandardTerminalObserver,
    report_context: &Value,
    observer_buffered: &mut Vec<u8>,
) -> Option<ExecutionStreamTerminalSummary> {
    if !observer_buffered.is_empty() {
        let line = std::mem::take(observer_buffered);
        observer.push_line(report_context, line);
    }
    observer.finish(report_context)
}

fn observe_stream_bytes(
    observer: &mut StreamingStandardTerminalObserver,
    report_context: &Value,
    observer_buffered: &mut Vec<u8>,
    normalized: &[u8],
) {
    if normalized.is_empty()
        || observer
            .latest_summary()
            .and_then(|summary| summary.parser_error.as_deref())
            .is_some()
    {
        return;
    }

    let mut remaining = normalized;
    while !remaining.is_empty() {
        let line_part_len = remaining
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(remaining.len(), |index| index + 1);
        if observer_buffered.len().saturating_add(line_part_len)
            > STREAM_USAGE_OBSERVER_MAX_LINE_BYTES
        {
            observer.disable_with_error(format!(
                "stream usage event exceeded {STREAM_USAGE_OBSERVER_MAX_LINE_BYTES} bytes"
            ));
            observer_buffered.clear();
            return;
        }
        observer_buffered.extend_from_slice(&remaining[..line_part_len]);
        remaining = &remaining[line_part_len..];
        if observer_buffered.last() == Some(&b'\n') {
            let line = std::mem::take(observer_buffered);
            observer.push_line(report_context, line);
        }
    }
}
