use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use aether_cache::ExpiringMap;
use aether_data::DataLayerError;
use aether_data_contracts::repository::provider_catalog::{
    ProviderCatalogKeyListQuery, ProviderCatalogReadRepository, StoredProviderCatalogEndpoint,
    StoredProviderCatalogKey, StoredProviderCatalogKeyMaintenanceSummary,
    StoredProviderCatalogKeyPage, StoredProviderCatalogKeyStats, StoredProviderCatalogProvider,
};
use async_trait::async_trait;
use tokio::sync::Notify;

const PROVIDER_CATALOG_CACHE_TTL: Duration = Duration::from_secs(5);
const PROVIDER_CATALOG_CACHE_MAX_ENTRIES: usize = 1024;
const PROVIDER_CATALOG_CACHE_MAX_INFLIGHT: usize = 1024;
const PROVIDER_CATALOG_CACHE_LOAD_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct CachedProviderCatalogReadRepository {
    inner: Arc<dyn ProviderCatalogReadRepository>,
    entries: ExpiringMap<ProviderCatalogCacheKey, ProviderCatalogCacheValue>,
    inflight: Mutex<HashMap<ProviderCatalogCacheKey, Arc<ProviderCatalogInflightState>>>,
    admission: Arc<tokio::sync::Semaphore>,
    epoch: AtomicU64,
    mutation: Mutex<()>,
}

impl CachedProviderCatalogReadRepository {
    pub(super) fn new(inner: Arc<dyn ProviderCatalogReadRepository>) -> Self {
        Self {
            inner,
            entries: ExpiringMap::new(),
            inflight: Mutex::new(HashMap::new()),
            admission: Arc::new(tokio::sync::Semaphore::new(
                PROVIDER_CATALOG_CACHE_MAX_INFLIGHT,
            )),
            epoch: AtomicU64::new(0),
            mutation: Mutex::new(()),
        }
    }

    async fn get_or_load<F, Fut>(
        &self,
        key: ProviderCatalogCacheKey,
        load: F,
    ) -> Result<ProviderCatalogCacheValue, DataLayerError>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<ProviderCatalogCacheValue, DataLayerError>>,
    {
        loop {
            if let Some(value) = self.entries.get_fresh(&key, PROVIDER_CATALOG_CACHE_TTL) {
                return Ok(value);
            }

            match self.register_inflight(&key) {
                InflightRegistration::Saturated => {
                    return Err(DataLayerError::TimedOut(format!(
                        "provider catalog cache admission saturated for {key:?}"
                    )));
                }
                InflightRegistration::Follower(state) => {
                    state.wait().await;
                    match self.follower_completion(&state) {
                        Some(ProviderCatalogInflightCompletion::Loaded(value)) => return Ok(value),
                        Some(ProviderCatalogInflightCompletion::Failed(error)) => {
                            return Err(error);
                        }
                        Some(
                            ProviderCatalogInflightCompletion::Cancelled
                            | ProviderCatalogInflightCompletion::Invalidated,
                        )
                        | None => continue,
                    }
                }
                InflightRegistration::Leader(mut guard) => {
                    if let Some(value) = self.entries.get_fresh(&key, PROVIDER_CATALOG_CACHE_TTL) {
                        guard.finish(ProviderCatalogInflightCompletion::Loaded(value.clone()));
                        return Ok(value);
                    }
                    let result =
                        match tokio::time::timeout(PROVIDER_CATALOG_CACHE_LOAD_TIMEOUT, load())
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(DataLayerError::TimedOut(format!(
                                "provider catalog cache load exceeded {}ms for {key:?}",
                                PROVIDER_CATALOG_CACHE_LOAD_TIMEOUT.as_millis()
                            ))),
                        };
                    match result {
                        Ok(value) => {
                            guard.finish_loaded(value.clone());
                            return Ok(value);
                        }
                        Err(error) => {
                            guard.finish(ProviderCatalogInflightCompletion::Failed(error.clone()));
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    fn register_inflight(&self, key: &ProviderCatalogCacheKey) -> InflightRegistration<'_> {
        {
            let inflight = self
                .inflight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(state) = inflight.get(key) {
                return InflightRegistration::Follower(Arc::clone(state));
            }
        }

        let _mutation = self
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut inflight = self
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = inflight.get(key) {
            return InflightRegistration::Follower(Arc::clone(state));
        }
        if inflight.len() >= PROVIDER_CATALOG_CACHE_MAX_INFLIGHT {
            return InflightRegistration::Saturated;
        }
        let Ok(admission) = Arc::clone(&self.admission).try_acquire_owned() else {
            return InflightRegistration::Saturated;
        };

        let state = Arc::new(ProviderCatalogInflightState {
            notify: Arc::new(Notify::new()),
            completion: OnceLock::new(),
            epoch: self.epoch.load(Ordering::Acquire),
        });
        inflight.insert(key.clone(), Arc::clone(&state));
        InflightRegistration::Leader(InflightGuard {
            cache: self,
            key: Some(key.clone()),
            state,
            admission: Some(admission),
        })
    }

    fn finish_inflight(
        &self,
        key: &ProviderCatalogCacheKey,
        state: &Arc<ProviderCatalogInflightState>,
        admission: tokio::sync::OwnedSemaphorePermit,
        completion: ProviderCatalogInflightCompletion,
        cache_value: Option<ProviderCatalogCacheValue>,
    ) {
        let _mutation = self
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = {
            let mut inflight = self
                .inflight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            drop(admission);
            debug_assert!(self.admission.available_permits() > 0);
            if inflight
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, state))
                && self.epoch.load(Ordering::Acquire) == state.epoch
            {
                if let Some(value) = cache_value {
                    self.entries.insert(
                        key.clone(),
                        value,
                        PROVIDER_CATALOG_CACHE_TTL,
                        PROVIDER_CATALOG_CACHE_MAX_ENTRIES,
                    );
                }
                state.complete(completion);
                inflight.remove(key);
                true
            } else {
                false
            }
        };
        drop(_mutation);
        if removed {
            state.notify.notify_waiters();
        }
    }

    fn follower_completion(
        &self,
        state: &ProviderCatalogInflightState,
    ) -> Option<ProviderCatalogInflightCompletion> {
        let before = self.epoch.load(Ordering::Acquire);
        let completion = state.completion.get().cloned();
        let after = self.epoch.load(Ordering::Acquire);
        (before == state.epoch && before == after)
            .then_some(completion)
            .flatten()
    }

    fn clear(&self) {
        let _mutation = self
            .mutation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.entries.clear();
        let states = self
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain()
            .map(|(_, state)| state)
            .collect::<Vec<_>>();
        for state in &states {
            state.complete(ProviderCatalogInflightCompletion::Invalidated);
        }
        drop(_mutation);
        for state in states {
            state.notify.notify_waiters();
        }
    }
}

#[async_trait]
impl ProviderCatalogReadRepository for CachedProviderCatalogReadRepository {
    fn clear_local_cache(&self) {
        self.clear();
        self.inner.clear_local_cache();
    }

    async fn list_providers(
        &self,
        active_only: bool,
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError> {
        match self
            .get_or_load(
                ProviderCatalogCacheKey::Providers { active_only },
                || async move {
                    self.inner
                        .list_providers(active_only)
                        .await
                        .map(ProviderCatalogCacheValue::Providers)
                },
            )
            .await?
        {
            ProviderCatalogCacheValue::Providers(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_providers_by_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogProvider>, DataLayerError> {
        let key = ProviderCatalogCacheKey::ProvidersByIds(normalize_ids(provider_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_providers_by_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Providers)
            })
            .await?
        {
            ProviderCatalogCacheValue::Providers(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_endpoints_by_ids(
        &self,
        endpoint_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError> {
        let key = ProviderCatalogCacheKey::EndpointsByIds(normalize_ids(endpoint_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_endpoints_by_ids(endpoint_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Endpoints)
            })
            .await?
        {
            ProviderCatalogCacheValue::Endpoints(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_endpoints_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogEndpoint>, DataLayerError> {
        let key = ProviderCatalogCacheKey::EndpointsByProviderIds(normalize_ids(provider_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_endpoints_by_provider_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Endpoints)
            })
            .await?
        {
            ProviderCatalogCacheValue::Endpoints(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_keys_by_ids(
        &self,
        key_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        let key = ProviderCatalogCacheKey::KeysByIds(normalize_ids(key_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_keys_by_ids(key_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Keys)
            })
            .await?
        {
            ProviderCatalogCacheValue::Keys(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_keys_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        let key = ProviderCatalogCacheKey::KeysByProviderIds(normalize_ids(provider_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_keys_by_provider_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Keys)
            })
            .await?
        {
            ProviderCatalogCacheValue::Keys(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_key_summaries_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKey>, DataLayerError> {
        let key = ProviderCatalogCacheKey::KeySummariesByProviderIds(normalize_ids(provider_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_key_summaries_by_provider_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::Keys)
            })
            .await?
        {
            ProviderCatalogCacheValue::Keys(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_key_maintenance_summaries_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKeyMaintenanceSummary>, DataLayerError> {
        let key = ProviderCatalogCacheKey::KeyMaintenanceSummariesByProviderIds(normalize_ids(
            provider_ids,
        ));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_key_maintenance_summaries_by_provider_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::KeyMaintenanceSummaries)
            })
            .await?
        {
            ProviderCatalogCacheValue::KeyMaintenanceSummaries(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }

    async fn list_keys_page(
        &self,
        query: &ProviderCatalogKeyListQuery,
    ) -> Result<StoredProviderCatalogKeyPage, DataLayerError> {
        self.inner.list_keys_page(query).await
    }

    async fn list_key_stats_by_provider_ids(
        &self,
        provider_ids: &[String],
    ) -> Result<Vec<StoredProviderCatalogKeyStats>, DataLayerError> {
        let key = ProviderCatalogCacheKey::KeyStatsByProviderIds(normalize_ids(provider_ids));
        match self
            .get_or_load(key, || async move {
                self.inner
                    .list_key_stats_by_provider_ids(provider_ids)
                    .await
                    .map(ProviderCatalogCacheValue::KeyStats)
            })
            .await?
        {
            ProviderCatalogCacheValue::KeyStats(items) => Ok(items),
            _ => Ok(Vec::new()),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ProviderCatalogCacheKey {
    Providers { active_only: bool },
    ProvidersByIds(Vec<String>),
    EndpointsByIds(Vec<String>),
    EndpointsByProviderIds(Vec<String>),
    KeysByIds(Vec<String>),
    KeysByProviderIds(Vec<String>),
    KeySummariesByProviderIds(Vec<String>),
    KeyMaintenanceSummariesByProviderIds(Vec<String>),
    KeyStatsByProviderIds(Vec<String>),
}

#[derive(Clone)]
enum ProviderCatalogCacheValue {
    Providers(Vec<StoredProviderCatalogProvider>),
    Endpoints(Vec<StoredProviderCatalogEndpoint>),
    Keys(Vec<StoredProviderCatalogKey>),
    KeyMaintenanceSummaries(Vec<StoredProviderCatalogKeyMaintenanceSummary>),
    KeyStats(Vec<StoredProviderCatalogKeyStats>),
}

struct ProviderCatalogInflightState {
    notify: Arc<Notify>,
    completion: OnceLock<ProviderCatalogInflightCompletion>,
    epoch: u64,
}

impl ProviderCatalogInflightState {
    fn complete(&self, completion: ProviderCatalogInflightCompletion) {
        let _ = self.completion.set(completion);
    }

    async fn wait(&self) {
        loop {
            if self.completion.get().is_some() {
                return;
            }

            let mut notified = Box::pin(self.notify.notified());
            notified.as_mut().enable();
            if self.completion.get().is_some() {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Clone)]
enum ProviderCatalogInflightCompletion {
    Loaded(ProviderCatalogCacheValue),
    Failed(DataLayerError),
    Cancelled,
    Invalidated,
}

enum InflightRegistration<'a> {
    Leader(InflightGuard<'a>),
    Follower(Arc<ProviderCatalogInflightState>),
    Saturated,
}

struct InflightGuard<'a> {
    cache: &'a CachedProviderCatalogReadRepository,
    key: Option<ProviderCatalogCacheKey>,
    state: Arc<ProviderCatalogInflightState>,
    admission: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl InflightGuard<'_> {
    fn finish_loaded(&mut self, value: ProviderCatalogCacheValue) {
        let completion = ProviderCatalogInflightCompletion::Loaded(value.clone());
        self.finish_with_cache(completion, Some(value));
    }

    fn finish(&mut self, completion: ProviderCatalogInflightCompletion) {
        self.finish_with_cache(completion, None);
    }

    fn finish_with_cache(
        &mut self,
        completion: ProviderCatalogInflightCompletion,
        cache_value: Option<ProviderCatalogCacheValue>,
    ) {
        if let Some(key) = self.key.take() {
            let admission = self
                .admission
                .take()
                .expect("active provider catalog leader must own admission");
            self.cache
                .finish_inflight(&key, &self.state, admission, completion, cache_value);
        }
    }
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.finish(ProviderCatalogInflightCompletion::Cancelled);
    }
}

fn normalize_ids(ids: &[String]) -> Vec<String> {
    let mut normalized = ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}
