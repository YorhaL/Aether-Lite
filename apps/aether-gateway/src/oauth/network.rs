use crate::AppState;
use aether_oauth::network::OAuthNetworkContext;

pub(crate) async fn resolve_identity_oauth_network_context(
    _state: &AppState,
) -> OAuthNetworkContext {
    OAuthNetworkContext::direct_identity()
}
