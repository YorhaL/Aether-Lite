use std::collections::BTreeMap;
use std::sync::RwLock;

use async_trait::async_trait;

use super::{BillingReadRepository, StoredBillingModelContext};
use crate::DataLayerError;

type BillingContextKey = (String, String, Option<String>);
type BillingContextMap = BTreeMap<BillingContextKey, StoredBillingModelContext>;

#[derive(Debug, Default)]
pub struct InMemoryBillingReadRepository {
    by_key: RwLock<BillingContextMap>,
}

impl InMemoryBillingReadRepository {
    pub fn seed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = StoredBillingModelContext>,
    {
        let mut by_key = BTreeMap::new();
        for item in items {
            by_key.insert(
                (
                    item.provider_id.clone(),
                    item.global_model_name.clone(),
                    item.provider_api_key_id.clone(),
                ),
                item,
            );
        }
        Self {
            by_key: RwLock::new(by_key),
        }
    }
}

#[async_trait]
impl BillingReadRepository for InMemoryBillingReadRepository {
    async fn find_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let by_key = self.by_key.read().expect("billing repository lock");
        if let Some(value) = find_context_by_provider_model_name(
            &by_key,
            provider_id,
            provider_api_key_id,
            global_model_name,
        ) {
            return Ok(Some(value));
        }

        let key = (
            provider_id.to_string(),
            global_model_name.to_string(),
            provider_api_key_id.map(ToOwned::to_owned),
        );
        if let Some(value) = by_key.get(&key) {
            return Ok(Some(value.clone()));
        }

        if let Some(value) = by_key
            .get(&(provider_id.to_string(), global_model_name.to_string(), None))
            .cloned()
        {
            return Ok(Some(value));
        }

        Ok(by_key
            .iter()
            .find(|((stored_provider_id, stored_model_name, _), _)| {
                stored_provider_id == provider_id && stored_model_name == global_model_name
            })
            .map(|(_, value)| value.clone()))
    }

    async fn find_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, DataLayerError> {
        let by_key = self.by_key.read().expect("billing repository lock");
        if let Some(value) =
            find_context_by_model_id_and_key(&by_key, provider_id, provider_api_key_id, model_id)
        {
            return Ok(Some(value));
        }
        if provider_api_key_id.is_some() {
            if let Some(value) =
                find_context_by_model_id_and_key(&by_key, provider_id, None, model_id)
            {
                return Ok(Some(value));
            }
        }
        Ok(by_key
            .iter()
            .find(|((stored_provider_id, _, _), value)| {
                stored_provider_id == provider_id && value.model_id.as_deref() == Some(model_id)
            })
            .map(|(_, value)| value.clone()))
    }
}

fn find_context_by_provider_model_name(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    requested_model: &str,
) -> Option<StoredBillingModelContext> {
    find_context_by_provider_model_name_and_key(
        by_key,
        provider_id,
        provider_api_key_id,
        requested_model,
    )
    .or_else(|| {
        provider_api_key_id.and_then(|_| {
            find_context_by_provider_model_name_and_key(by_key, provider_id, None, requested_model)
        })
    })
    .or_else(|| {
        by_key
            .iter()
            .find(|((stored_provider_id, _, _), value)| {
                stored_provider_id == provider_id
                    && value.model_provider_model_name.as_deref() == Some(requested_model)
            })
            .map(|(_, value)| value.clone())
    })
}

fn find_context_by_provider_model_name_and_key(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    requested_model: &str,
) -> Option<StoredBillingModelContext> {
    by_key
        .iter()
        .filter(|((stored_provider_id, _, stored_key_id), value)| {
            stored_provider_id == provider_id
                && stored_key_id.as_deref() == provider_api_key_id
                && value.model_provider_model_name.as_deref() == Some(requested_model)
        })
        .min_by_key(|(_, value)| {
            let has_model_price =
                value.model_tiered_pricing.is_some() || value.model_price_per_request.is_some();
            let has_default_price =
                value.default_tiered_pricing.is_some() || value.default_price_per_request.is_some();
            (!has_model_price, !has_default_price)
        })
        .map(|(_, value)| value.clone())
}

fn find_context_by_model_id_and_key(
    by_key: &BillingContextMap,
    provider_id: &str,
    provider_api_key_id: Option<&str>,
    model_id: &str,
) -> Option<StoredBillingModelContext> {
    by_key
        .iter()
        .find(|((stored_provider_id, _, stored_key_id), value)| {
            stored_provider_id == provider_id
                && stored_key_id.as_deref() == provider_api_key_id
                && value.model_id.as_deref() == Some(model_id)
        })
        .map(|(_, value)| value.clone())
}
