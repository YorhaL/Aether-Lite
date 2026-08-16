use std::sync::Arc;

use aether_data::repository::candidates::InMemoryRequestCandidateRepository;
use aether_data_contracts::repository::candidates::{
    RequestCandidateReadRepository, RequestCandidateStatus, RequestCandidateWriteRepository,
    UpsertRequestCandidateRecord,
};
use serde_json::json;

use crate::AppState;

#[tokio::test]
async fn gateway_background_request_candidate_cleanup_deletes_expired_entries_in_batches() {
    fn now_unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    async fn seed_candidate(
        repository: &InMemoryRequestCandidateRepository,
        id: &str,
        created_at_unix_ms: u64,
    ) {
        repository
            .upsert(UpsertRequestCandidateRecord {
                id: id.to_string(),
                request_id: format!("req-{id}"),
                user_id: Some("user-1".to_string()),
                api_key_id: Some("api-key-1".to_string()),
                username: Some("alice".to_string()),
                api_key_name: Some("default".to_string()),
                candidate_index: 0,
                retry_index: 0,
                provider_id: Some("provider-1".to_string()),
                endpoint_id: Some("endpoint-1".to_string()),
                key_id: Some("key-1".to_string()),
                status: RequestCandidateStatus::Success,
                skip_reason: None,
                is_cached: Some(false),
                status_code: Some(200),
                error_type: None,
                error_message: None,
                latency_ms: Some(10),
                concurrent_requests: Some(1),
                extra_data: None,
                required_capabilities: None,
                created_at_unix_ms: Some(created_at_unix_ms),
                started_at_unix_ms: Some(created_at_unix_ms),
                finished_at_unix_ms: Some(created_at_unix_ms.saturating_add(1)),
            })
            .await
            .expect("candidate should seed");
    }

    let repository = Arc::new(InMemoryRequestCandidateRepository::default());
    seed_candidate(&repository, "cand-expired-1", 1_000).await;
    seed_candidate(&repository, "cand-expired-2", 2_000).await;
    seed_candidate(&repository, "cand-active", now_unix_ms()).await;

    let data_state = crate::data::GatewayDataState::with_request_candidate_repository_for_tests(
        Arc::clone(&repository),
    )
    .with_system_config_values_for_tests([
        ("enable_auto_cleanup".to_string(), json!(true)),
        ("cleanup_batch_size".to_string(), json!(1)),
        (
            "request_candidates_cleanup_batch_size".to_string(),
            json!(1),
        ),
        ("request_candidates_retention_days".to_string(), json!(30)),
    ]);

    let gateway_state = AppState::new()
        .expect("gateway state should build")
        .with_data_state_for_tests(data_state);
    let background_tasks = gateway_state.spawn_background_tasks();
    assert!(!background_tasks.is_empty(), "cleanup worker should spawn");

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let rows = repository
            .list_recent(10)
            .await
            .expect("list recent should succeed");
        if rows.len() == 1 {
            assert_eq!(rows[0].id, "cand-active");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "cleanup worker did not delete expired request candidates within 500ms"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    background_tasks.shutdown().await;
}
