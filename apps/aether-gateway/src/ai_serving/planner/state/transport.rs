use super::PlannerAppState;
pub(crate) use crate::ai_serving::transport::GatewayProviderTransportSnapshot;
use crate::GatewayError;
use std::sync::Arc;

impl<'a> PlannerAppState<'a> {
    pub(crate) async fn read_provider_transport_snapshot_arc(
        self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<Arc<GatewayProviderTransportSnapshot>>, GatewayError> {
        self.app()
            .read_provider_transport_snapshot_arc(provider_id, endpoint_id, key_id)
            .await
    }

    pub(crate) async fn read_provider_transport_snapshot(
        self,
        provider_id: &str,
        endpoint_id: &str,
        key_id: &str,
    ) -> Result<Option<GatewayProviderTransportSnapshot>, GatewayError> {
        self.app()
            .read_provider_transport_snapshot(provider_id, endpoint_id, key_id)
            .await
    }
}
