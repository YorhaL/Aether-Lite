#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OAuthTimeouts {
    pub connect_ms: u64,
    pub read_ms: u64,
    pub write_ms: u64,
    pub total_ms: u64,
}

impl OAuthTimeouts {
    pub const DIRECT_DEFAULT: Self = Self {
        connect_ms: 30_000,
        read_ms: 30_000,
        write_ms: 30_000,
        total_ms: 30_000,
    };
}

#[derive(Debug, Clone, PartialEq)]
pub struct OAuthNetworkContext {
    pub timeouts: OAuthTimeouts,
}

impl OAuthNetworkContext {
    pub fn direct_identity() -> Self {
        Self {
            timeouts: OAuthTimeouts::DIRECT_DEFAULT,
        }
    }
}
