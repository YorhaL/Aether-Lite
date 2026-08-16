use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredBillingModelContext {
    pub provider_id: String,
    pub provider_api_key_id: Option<String>,
    pub provider_api_key_rate_multipliers: Option<Value>,
    pub provider_api_key_cache_ttl_minutes: Option<i64>,
    pub global_model_id: String,
    pub global_model_name: String,
    pub global_model_config: Option<Value>,
    pub default_price_per_request: Option<f64>,
    pub default_tiered_pricing: Option<Value>,
    pub model_id: Option<String>,
    pub model_provider_model_name: Option<String>,
    pub model_config: Option<Value>,
    pub model_price_per_request: Option<f64>,
    pub model_tiered_pricing: Option<Value>,
}

impl StoredBillingModelContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: String,
        provider_api_key_id: Option<String>,
        provider_api_key_rate_multipliers: Option<Value>,
        provider_api_key_cache_ttl_minutes: Option<i64>,
        global_model_id: String,
        global_model_name: String,
        global_model_config: Option<Value>,
        default_price_per_request: Option<f64>,
        default_tiered_pricing: Option<Value>,
        model_id: Option<String>,
        model_provider_model_name: Option<String>,
        model_config: Option<Value>,
        model_price_per_request: Option<f64>,
        model_tiered_pricing: Option<Value>,
    ) -> Result<Self, crate::DataLayerError> {
        if provider_id.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.provider_id is empty".to_string(),
            ));
        }
        if global_model_id.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.global_model_id is empty".to_string(),
            ));
        }
        if global_model_name.trim().is_empty() {
            return Err(crate::DataLayerError::UnexpectedValue(
                "billing.global_model_name is empty".to_string(),
            ));
        }
        Ok(Self {
            provider_id,
            provider_api_key_id,
            provider_api_key_rate_multipliers,
            provider_api_key_cache_ttl_minutes,
            global_model_id,
            global_model_name,
            global_model_config,
            default_price_per_request,
            default_tiered_pricing,
            model_id,
            model_provider_model_name,
            model_config,
            model_price_per_request,
            model_tiered_pricing,
        })
    }
}

#[async_trait]
pub trait BillingReadRepository: Send + Sync {
    async fn find_model_context(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        global_model_name: &str,
    ) -> Result<Option<StoredBillingModelContext>, crate::DataLayerError>;

    async fn find_model_context_by_model_id(
        &self,
        provider_id: &str,
        provider_api_key_id: Option<&str>,
        model_id: &str,
    ) -> Result<Option<StoredBillingModelContext>, crate::DataLayerError>;
}
