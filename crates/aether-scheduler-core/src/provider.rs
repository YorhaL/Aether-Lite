use std::collections::BTreeMap;

use aether_data_contracts::repository::provider_catalog::StoredProviderCatalogProvider;
pub fn build_provider_concurrent_limit_map(
    providers: Vec<StoredProviderCatalogProvider>,
) -> BTreeMap<String, usize> {
    providers
        .into_iter()
        .filter_map(|provider| {
            provider
                .concurrent_limit
                .and_then(|limit| usize::try_from(limit).ok())
                .filter(|limit| *limit > 0)
                .map(|limit| (provider.id, limit))
        })
        .collect()
}
