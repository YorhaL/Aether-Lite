mod memory;

pub use aether_data_contracts::repository::oauth_providers::{
    EncryptedSecretUpdate, OAuthProviderReadRepository, OAuthProviderRepository,
    OAuthProviderWriteRepository, StoredOAuthProviderConfig, UpsertOAuthProviderConfigRecord,
};
#[cfg(feature = "postgres")]
pub use aether_data_postgres::SqlxOAuthProviderRepository;
#[cfg(feature = "sqlite")]
pub use aether_data_sqlite::SqliteOAuthProviderRepository;
pub use memory::InMemoryOAuthProviderRepository;
