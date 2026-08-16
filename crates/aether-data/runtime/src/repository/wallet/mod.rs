mod memory;

pub use aether_data_contracts::repository::wallet::{
    AdjustWalletBalanceInput, AdminWalletListQuery, StoredAdminWalletListItem,
    StoredAdminWalletListPage, StoredWalletSnapshot, WalletLookupKey, WalletReadRepository,
    WalletRepository, WalletWriteRepository,
};
#[cfg(feature = "postgres")]
pub use aether_data_postgres::SqlxWalletRepository;
#[cfg(feature = "sqlite")]
pub use aether_data_sqlite::SqliteWalletReadRepository;
pub use memory::InMemoryWalletRepository;
