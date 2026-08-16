use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use aether_data_contracts::repository::candidates::{
    PublicHealthStatusCount, PublicHealthTimelineBucket, RequestCandidateReadRepository,
    StoredRequestCandidate,
};
use async_trait::async_trait;

const RECENT_REQUEST_CANDIDATES_TTL: Duration = Duration::from_millis(100);
const RECENT_REQUEST_CANDIDATES_MAX_CACHE_KEYS: usize = 8;

#[derive(Clone)]
pub(super) struct CachedRequestCandidateReadRepository {
    inner: Arc<dyn RequestCandidateReadRepository>,
    recent: Arc<RwLock<BTreeMap<usize, (Instant, Vec<StoredRequestCandidate>)>>>,
    recent_load_guard: Arc<tokio::sync::Mutex<()>>,
}

impl CachedRequestCandidateReadRepository {
    pub(super) fn new(inner: Arc<dyn RequestCandidateReadRepository>) -> Self {
        Self {
            inner,
            recent: Default::default(),
            recent_load_guard: Default::default(),
        }
    }

    fn cached_recent(&self, limit: usize, now: Instant) -> Option<Vec<StoredRequestCandidate>> {
        self.recent
            .read()
            .expect("recent request candidates cache lock")
            .get(&limit)
            .filter(|(loaded_at, _)| {
                now.duration_since(*loaded_at) <= RECENT_REQUEST_CANDIDATES_TTL
            })
            .map(|(_, rows)| rows.clone())
    }

    fn store_recent(&self, limit: usize, rows: &[StoredRequestCandidate], now: Instant) {
        let mut cache = self
            .recent
            .write()
            .expect("recent request candidates cache lock");
        if cache.len() >= RECENT_REQUEST_CANDIDATES_MAX_CACHE_KEYS && !cache.contains_key(&limit) {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, (loaded_at, _))| *loaded_at)
                .map(|(key, _)| *key)
            {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(limit, (now, rows.to_vec()));
    }
}

#[async_trait]
impl RequestCandidateReadRepository for CachedRequestCandidateReadRepository {
    async fn list_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, aether_data::DataLayerError> {
        self.inner.list_by_request_id(request_id).await
    }

    async fn list_attempted_by_request_id(
        &self,
        request_id: &str,
    ) -> Result<Vec<StoredRequestCandidate>, aether_data::DataLayerError> {
        self.inner.list_attempted_by_request_id(request_id).await
    }

    async fn list_recent(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, aether_data::DataLayerError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let now = Instant::now();
        if let Some(rows) = self.cached_recent(limit, now) {
            return Ok(rows);
        }

        let _guard = self.recent_load_guard.lock().await;
        let now = Instant::now();
        if let Some(rows) = self.cached_recent(limit, now) {
            return Ok(rows);
        }

        let rows = self.inner.list_recent(limit).await?;
        self.store_recent(limit, &rows, now);
        Ok(rows)
    }

    async fn count_active_for_user_since(
        &self,
        user_id: &str,
        active_since_unix_secs: u64,
    ) -> Result<u64, aether_data::DataLayerError> {
        self.inner
            .count_active_for_user_since(user_id, active_since_unix_secs)
            .await
    }

    async fn list_by_provider_id(
        &self,
        provider_id: &str,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, aether_data::DataLayerError> {
        self.inner.list_by_provider_id(provider_id, limit).await
    }

    async fn list_finalized_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<StoredRequestCandidate>, aether_data::DataLayerError> {
        self.inner
            .list_finalized_by_endpoint_ids_since(endpoint_ids, since_unix_secs, limit)
            .await
    }

    async fn count_finalized_statuses_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
    ) -> Result<Vec<PublicHealthStatusCount>, aether_data::DataLayerError> {
        self.inner
            .count_finalized_statuses_by_endpoint_ids_since(endpoint_ids, since_unix_secs)
            .await
    }

    async fn aggregate_finalized_timeline_by_endpoint_ids_since(
        &self,
        endpoint_ids: &[String],
        since_unix_secs: u64,
        until_unix_secs: u64,
        segments: u32,
    ) -> Result<Vec<PublicHealthTimelineBucket>, aether_data::DataLayerError> {
        self.inner
            .aggregate_finalized_timeline_by_endpoint_ids_since(
                endpoint_ids,
                since_unix_secs,
                until_unix_secs,
                segments,
            )
            .await
    }
}
