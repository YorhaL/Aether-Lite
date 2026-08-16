use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::{mapref::entry::Entry as DashMapEntry, DashMap};

use super::{
    AppState, CachedProviderTransportSnapshot, GatewayError, ProviderTransportSnapshotCacheKey,
    ProviderTransportSnapshotFlight, ProviderTransportSnapshotFlightResult,
    PROVIDER_TRANSPORT_SNAPSHOT_CACHE_MAX_ENTRIES, PROVIDER_TRANSPORT_SNAPSHOT_CACHE_STALE_TTL,
    PROVIDER_TRANSPORT_SNAPSHOT_CACHE_TTL,
};

enum ProviderTransportSnapshotCacheLookup {
    Fresh(Arc<crate::provider_transport::GatewayProviderTransportSnapshot>),
    Stale(Arc<crate::provider_transport::GatewayProviderTransportSnapshot>),
    Miss,
}

enum ProviderTransportSnapshotReloadResult {
    Published(Arc<crate::provider_transport::GatewayProviderTransportSnapshot>),
    Missing,
    Invalidated,
}

enum ProviderTransportSnapshotInflightRegistration {
    Leader(ProviderTransportSnapshotInflightGuard),
    Follower(Arc<ProviderTransportSnapshotFlight>),
    Retry,
}

struct ProviderTransportSnapshotInflightGuard {
    inflight: Arc<DashMap<ProviderTransportSnapshotCacheKey, Arc<ProviderTransportSnapshotFlight>>>,
    cache_key: Option<ProviderTransportSnapshotCacheKey>,
    flight: Arc<ProviderTransportSnapshotFlight>,
}

impl ProviderTransportSnapshotInflightGuard {
    fn generation(&self) -> u64 {
        self.flight.generation()
    }

    fn generation_is_current(&self, state: &AppState) -> bool {
        state
            .provider_transport_snapshot_cache_generation
            .load(Ordering::Acquire)
            == self.generation()
    }

    fn finish(&mut self, result: ProviderTransportSnapshotFlightResult) {
        let Some(cache_key) = self.cache_key.take() else {
            return;
        };
        self.flight.complete(result);
        self.inflight
            .remove_if(&cache_key, |_, current| Arc::ptr_eq(current, &self.flight));
    }
}

impl Drop for ProviderTransportSnapshotInflightGuard {
    fn drop(&mut self) {
        self.finish(ProviderTransportSnapshotFlightResult::Retry);
    }
}

fn flight_result(
    result: &Result<ProviderTransportSnapshotReloadResult, GatewayError>,
) -> ProviderTransportSnapshotFlightResult {
    match result {
        Ok(ProviderTransportSnapshotReloadResult::Published(snapshot)) => {
            ProviderTransportSnapshotFlightResult::Published(Arc::clone(snapshot))
        }
        Ok(ProviderTransportSnapshotReloadResult::Missing) => {
            ProviderTransportSnapshotFlightResult::Missing
        }
        Ok(ProviderTransportSnapshotReloadResult::Invalidated) => {
            ProviderTransportSnapshotFlightResult::Invalidated
        }
        Err(err) => ProviderTransportSnapshotFlightResult::Error(err.clone()),
    }
}

impl AppState {
    pub(crate) fn clear_provider_transport_snapshot_cache(&self) {
        self.provider_transport_snapshot_cache_generation
            .fetch_add(1, Ordering::AcqRel);
        self.provider_transport_snapshot_cache.clear();

        let current_generation = self
            .provider_transport_snapshot_cache_generation
            .load(Ordering::Acquire);
        let mut invalidated = Vec::new();
        self.provider_transport_snapshot_inflight
            .retain(|_, flight| {
                if flight.generation() < current_generation {
                    invalidated.push(Arc::clone(flight));
                    false
                } else {
                    true
                }
            });
        for flight in invalidated {
            flight.complete(ProviderTransportSnapshotFlightResult::Invalidated);
        }
    }

    fn register_provider_transport_snapshot_inflight(
        &self,
        cache_key: &ProviderTransportSnapshotCacheKey,
        generation: u64,
    ) -> ProviderTransportSnapshotInflightRegistration {
        let flight = Arc::new(ProviderTransportSnapshotFlight::new(generation));
        match self
            .provider_transport_snapshot_inflight
            .entry(cache_key.clone())
        {
            DashMapEntry::Occupied(entry) => {
                let current = Arc::clone(entry.get());
                if current.generation() == generation {
                    return ProviderTransportSnapshotInflightRegistration::Follower(current);
                }
                if self
                    .provider_transport_snapshot_cache_generation
                    .load(Ordering::Acquire)
                    != generation
                {
                    return ProviderTransportSnapshotInflightRegistration::Retry;
                }
                let invalidated = entry.remove();
                invalidated.complete(ProviderTransportSnapshotFlightResult::Invalidated);
                ProviderTransportSnapshotInflightRegistration::Retry
            }
            DashMapEntry::Vacant(entry) => {
                if self
                    .provider_transport_snapshot_cache_generation
                    .load(Ordering::Acquire)
                    != generation
                {
                    return ProviderTransportSnapshotInflightRegistration::Retry;
                }
                entry.insert(Arc::clone(&flight));
                ProviderTransportSnapshotInflightRegistration::Leader(
                    ProviderTransportSnapshotInflightGuard {
                        inflight: Arc::clone(&self.provider_transport_snapshot_inflight),
                        cache_key: Some(cache_key.clone()),
                        flight,
                    },
                )
            }
        }
    }

    fn get_cached_provider_transport_snapshot_arc(
        &self,
        cache_key: &ProviderTransportSnapshotCacheKey,
    ) -> ProviderTransportSnapshotCacheLookup {
        let Some(cached) = self
            .provider_transport_snapshot_cache
            .get(cache_key)
            .map(|entry| entry.clone())
        else {
            return ProviderTransportSnapshotCacheLookup::Miss;
        };
        if cached.generation
            != self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire)
        {
            self.provider_transport_snapshot_cache
                .remove_if(cache_key, |_, current| {
                    current.generation == cached.generation
                });
            return ProviderTransportSnapshotCacheLookup::Miss;
        }
        let age = cached.loaded_at.elapsed();
        if age <= PROVIDER_TRANSPORT_SNAPSHOT_CACHE_TTL {
            return ProviderTransportSnapshotCacheLookup::Fresh(cached.snapshot);
        }
        if age <= PROVIDER_TRANSPORT_SNAPSHOT_CACHE_STALE_TTL {
            return ProviderTransportSnapshotCacheLookup::Stale(cached.snapshot);
        }
        self.provider_transport_snapshot_cache
            .remove_if(cache_key, |_, current| {
                current.generation == cached.generation
            });
        ProviderTransportSnapshotCacheLookup::Miss
    }

    fn put_cached_provider_transport_snapshot(
        &self,
        cache_key: ProviderTransportSnapshotCacheKey,
        snapshot: Arc<crate::provider_transport::GatewayProviderTransportSnapshot>,
        generation: u64,
    ) -> bool {
        if generation
            != self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire)
        {
            return false;
        }
        if self.provider_transport_snapshot_cache.len()
            >= PROVIDER_TRANSPORT_SNAPSHOT_CACHE_MAX_ENTRIES
        {
            self.provider_transport_snapshot_cache.retain(|_, entry| {
                entry.loaded_at.elapsed() <= PROVIDER_TRANSPORT_SNAPSHOT_CACHE_STALE_TTL
            });
            if self.provider_transport_snapshot_cache.len()
                >= PROVIDER_TRANSPORT_SNAPSHOT_CACHE_MAX_ENTRIES
            {
                if let Some(oldest_key) = self
                    .provider_transport_snapshot_cache
                    .iter()
                    .min_by_key(|entry| entry.value().loaded_at)
                    .map(|entry| entry.key().clone())
                {
                    self.provider_transport_snapshot_cache.remove(&oldest_key);
                }
            }
        }
        self.provider_transport_snapshot_cache.insert(
            cache_key.clone(),
            CachedProviderTransportSnapshot {
                loaded_at: std::time::Instant::now(),
                generation,
                snapshot,
            },
        );
        if generation
            != self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire)
        {
            self.provider_transport_snapshot_cache
                .remove_if(&cache_key, |_, current| current.generation == generation);
            return false;
        }
        true
    }

    pub(crate) async fn read_provider_transport_snapshot_uncached(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<crate::provider_transport::GatewayProviderTransportSnapshot>, GatewayError>
    {
        self.data
            .read_provider_transport_snapshot(provider_id, endpoint_id, key_id)
            .await
            .map_err(|err| GatewayError::Internal(err.to_string()))
    }

    async fn reload_provider_transport_snapshot(
        &self,
        cache_key: &ProviderTransportSnapshotCacheKey,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
        generation: u64,
    ) -> Result<ProviderTransportSnapshotReloadResult, GatewayError> {
        if generation
            != self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire)
        {
            return Ok(ProviderTransportSnapshotReloadResult::Invalidated);
        }
        let loaded = self
            .read_provider_transport_snapshot_uncached(provider_id, endpoint_id, key_id)
            .await?;
        if generation
            != self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire)
        {
            return Ok(ProviderTransportSnapshotReloadResult::Invalidated);
        }
        let Some(snapshot) = loaded else {
            return Ok(ProviderTransportSnapshotReloadResult::Missing);
        };
        let snapshot = Arc::new(snapshot);
        if self.put_cached_provider_transport_snapshot(
            cache_key.clone(),
            Arc::clone(&snapshot),
            generation,
        ) {
            Ok(ProviderTransportSnapshotReloadResult::Published(snapshot))
        } else {
            Ok(ProviderTransportSnapshotReloadResult::Invalidated)
        }
    }

    fn start_provider_transport_snapshot_background_refresh(
        &self,
        cache_key: ProviderTransportSnapshotCacheKey,
        provider_id: String,
        endpoint_id: String,
        key_id: String,
    ) {
        let mut inflight_guard = loop {
            let generation = self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire);
            match self.register_provider_transport_snapshot_inflight(&cache_key, generation) {
                ProviderTransportSnapshotInflightRegistration::Leader(guard) => break guard,
                ProviderTransportSnapshotInflightRegistration::Follower(_) => return,
                ProviderTransportSnapshotInflightRegistration::Retry => continue,
            }
        };
        let generation = inflight_guard.generation();
        let state = self.clone();
        tokio::spawn(async move {
            let result = state
                .reload_provider_transport_snapshot(
                    &cache_key,
                    &provider_id,
                    &endpoint_id,
                    &key_id,
                    generation,
                )
                .await;
            if matches!(&result, Ok(ProviderTransportSnapshotReloadResult::Missing)) {
                state
                    .provider_transport_snapshot_cache
                    .remove_if(&cache_key, |_, current| current.generation == generation);
            }
            let result = if inflight_guard.generation_is_current(&state) {
                flight_result(&result)
            } else {
                ProviderTransportSnapshotFlightResult::Invalidated
            };
            inflight_guard.finish(result);
        });
    }

    pub(crate) async fn read_provider_transport_snapshot_arc(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<
        Option<Arc<crate::provider_transport::GatewayProviderTransportSnapshot>>,
        GatewayError,
    > {
        let Some(cache_key) =
            ProviderTransportSnapshotCacheKey::new(provider_id, endpoint_id, key_id)
        else {
            return Ok(self
                .read_provider_transport_snapshot_uncached(provider_id, endpoint_id, key_id)
                .await?
                .map(Arc::new));
        };
        loop {
            match self.get_cached_provider_transport_snapshot_arc(&cache_key) {
                ProviderTransportSnapshotCacheLookup::Fresh(snapshot) => {
                    return Ok(Some(snapshot));
                }
                ProviderTransportSnapshotCacheLookup::Stale(snapshot) => {
                    self.start_provider_transport_snapshot_background_refresh(
                        cache_key.clone(),
                        provider_id.to_string(),
                        endpoint_id.to_string(),
                        key_id.to_string(),
                    );
                    return Ok(Some(snapshot));
                }
                ProviderTransportSnapshotCacheLookup::Miss => {}
            }

            let generation = self
                .provider_transport_snapshot_cache_generation
                .load(Ordering::Acquire);
            match self.register_provider_transport_snapshot_inflight(&cache_key, generation) {
                ProviderTransportSnapshotInflightRegistration::Retry => continue,
                ProviderTransportSnapshotInflightRegistration::Follower(flight) => {
                    let flight_generation = flight.generation();
                    let result = flight.wait().await;
                    if self
                        .provider_transport_snapshot_cache_generation
                        .load(Ordering::Acquire)
                        != flight_generation
                    {
                        continue;
                    }
                    match result {
                        ProviderTransportSnapshotFlightResult::Published(snapshot) => {
                            return Ok(Some(snapshot));
                        }
                        ProviderTransportSnapshotFlightResult::Missing => return Ok(None),
                        ProviderTransportSnapshotFlightResult::Error(err) => return Err(err),
                        ProviderTransportSnapshotFlightResult::Invalidated
                        | ProviderTransportSnapshotFlightResult::Retry => continue,
                    }
                }
                ProviderTransportSnapshotInflightRegistration::Leader(mut guard) => {
                    let result = self
                        .reload_provider_transport_snapshot(
                            &cache_key,
                            provider_id,
                            endpoint_id,
                            key_id,
                            generation,
                        )
                        .await;
                    let published = if guard.generation_is_current(self) {
                        flight_result(&result)
                    } else {
                        ProviderTransportSnapshotFlightResult::Invalidated
                    };
                    guard.finish(published);
                    if !guard.generation_is_current(self) {
                        continue;
                    }
                    match result {
                        Ok(ProviderTransportSnapshotReloadResult::Published(snapshot)) => {
                            return Ok(Some(snapshot));
                        }
                        Ok(ProviderTransportSnapshotReloadResult::Missing) => return Ok(None),
                        Ok(ProviderTransportSnapshotReloadResult::Invalidated) => continue,
                        Err(err) => return Err(err),
                    }
                }
            }
        }
    }

    pub(crate) async fn read_provider_transport_snapshot(
        &self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<crate::provider_transport::GatewayProviderTransportSnapshot>, GatewayError>
    {
        Ok(self
            .read_provider_transport_snapshot_arc(provider_id, endpoint_id, key_id)
            .await?
            .map(|snapshot| (*snapshot).clone()))
    }
}
