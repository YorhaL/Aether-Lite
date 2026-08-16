use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const ADMISSION_POLICY_SCHEMA_VERSION: u16 = 1;
pub const SYSTEM_ADMISSION_POLICY_SUBJECT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionScopeKind {
    System,
    UserGroup,
    User,
    ApiKey,
}

impl AdmissionScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::UserGroup => "user_group",
            Self::User => "user",
            Self::ApiKey => "api_key",
        }
    }

    pub fn parse(value: &str) -> Result<Self, crate::DataLayerError> {
        match value {
            "system" => Ok(Self::System),
            "user_group" => Ok(Self::UserGroup),
            "user" => Ok(Self::User),
            "api_key" => Ok(Self::ApiKey),
            _ => Err(crate::DataLayerError::UnexpectedValue(format!(
                "unknown admission policy scope kind: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdmissionPolicyScope {
    pub kind: AdmissionScopeKind,
    pub subject_id: String,
}

impl AdmissionPolicyScope {
    pub fn new(
        kind: AdmissionScopeKind,
        subject_id: impl Into<String>,
    ) -> Result<Self, crate::DataLayerError> {
        let subject_id = subject_id.into();
        if subject_id.trim().is_empty() {
            return Err(crate::DataLayerError::InvalidInput(
                "admission policy subject_id must not be empty".to_string(),
            ));
        }
        Ok(Self { kind, subject_id })
    }

    pub fn system() -> Self {
        Self {
            kind: AdmissionScopeKind::System,
            subject_id: SYSTEM_ADMISSION_POLICY_SUBJECT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionUsagePeriod {
    CalendarDay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdmissionRule {
    RequestCount {
        max_requests: u32,
        window_seconds: u32,
    },
    ConcurrentRequests {
        max_concurrent: u32,
    },
    UsageCostUsd {
        max_usd: f64,
        period: AdmissionUsagePeriod,
    },
}

impl AdmissionRule {
    fn validate(&self) -> Result<(), crate::DataLayerError> {
        match self {
            Self::RequestCount { window_seconds, .. } if *window_seconds == 0 => {
                Err(crate::DataLayerError::InvalidInput(
                    "request-count admission window must be greater than zero".to_string(),
                ))
            }
            Self::UsageCostUsd { max_usd, .. } if !max_usd.is_finite() || *max_usd < 0.0 => {
                Err(crate::DataLayerError::InvalidInput(
                    "usage-cost admission limit must be a finite non-negative value".to_string(),
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdmissionPolicyDocument {
    pub schema_version: u16,
    #[serde(default)]
    pub rules: Vec<AdmissionRule>,
}

impl Default for AdmissionPolicyDocument {
    fn default() -> Self {
        Self {
            schema_version: ADMISSION_POLICY_SCHEMA_VERSION,
            rules: Vec::new(),
        }
    }
}

impl AdmissionPolicyDocument {
    pub fn validate(&self) -> Result<(), crate::DataLayerError> {
        if self.schema_version != ADMISSION_POLICY_SCHEMA_VERSION {
            return Err(crate::DataLayerError::UnexpectedValue(format!(
                "unsupported admission policy schema version: {}",
                self.schema_version
            )));
        }
        for (index, rule) in self.rules.iter().enumerate() {
            rule.validate()?;
            if self.rules[index + 1..]
                .iter()
                .any(|candidate| rules_match(rule, candidate))
            {
                return Err(crate::DataLayerError::InvalidInput(
                    "admission policy contains duplicate rules for the same scope".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn requests_per_minute(&self) -> Option<u32> {
        self.rules.iter().find_map(|rule| match rule {
            AdmissionRule::RequestCount {
                max_requests,
                window_seconds: 60,
            } => Some(*max_requests),
            _ => None,
        })
    }

    pub fn concurrent_requests(&self) -> Option<u32> {
        self.rules.iter().find_map(|rule| match rule {
            AdmissionRule::ConcurrentRequests { max_concurrent } => Some(*max_concurrent),
            _ => None,
        })
    }

    pub fn daily_usage_limit_usd(&self) -> Option<f64> {
        self.rules.iter().find_map(|rule| match rule {
            AdmissionRule::UsageCostUsd {
                max_usd,
                period: AdmissionUsagePeriod::CalendarDay,
            } => Some(*max_usd),
            _ => None,
        })
    }

    pub fn with_requests_per_minute(mut self, value: Option<u32>) -> Self {
        self.rules.retain(|rule| {
            !matches!(
                rule,
                AdmissionRule::RequestCount {
                    window_seconds: 60,
                    ..
                }
            )
        });
        if let Some(max_requests) = value {
            self.rules.push(AdmissionRule::RequestCount {
                max_requests,
                window_seconds: 60,
            });
        }
        self
    }

    pub fn with_concurrent_requests(mut self, value: Option<u32>) -> Self {
        self.rules
            .retain(|rule| !matches!(rule, AdmissionRule::ConcurrentRequests { .. }));
        if let Some(max_concurrent) = value {
            self.rules
                .push(AdmissionRule::ConcurrentRequests { max_concurrent });
        }
        self
    }

    pub fn with_daily_usage_limit_usd(mut self, value: Option<f64>) -> Self {
        self.rules.retain(|rule| {
            !matches!(
                rule,
                AdmissionRule::UsageCostUsd {
                    period: AdmissionUsagePeriod::CalendarDay,
                    ..
                }
            )
        });
        if let Some(max_usd) = value {
            self.rules.push(AdmissionRule::UsageCostUsd {
                max_usd,
                period: AdmissionUsagePeriod::CalendarDay,
            });
        }
        self
    }

    pub fn overlay(mut self, overlay: &Self) -> Self {
        for rule in &overlay.rules {
            self.remove_matching_rule(rule);
            self.rules.push(rule.clone());
        }
        self
    }

    pub fn union_grants(mut self, other: &Self) -> Self {
        for incoming in &other.rules {
            let current = self
                .rules
                .iter()
                .position(|rule| rules_match(rule, incoming));
            if let Some(index) = current {
                self.rules[index] = union_grant(&self.rules[index], incoming);
            } else {
                self.rules.push(incoming.clone());
            }
        }
        self
    }

    fn remove_matching_rule(&mut self, incoming: &AdmissionRule) {
        self.rules.retain(|rule| !rules_match(rule, incoming));
    }
}

fn rules_match(left: &AdmissionRule, right: &AdmissionRule) -> bool {
    match (left, right) {
        (
            AdmissionRule::RequestCount {
                window_seconds: left,
                ..
            },
            AdmissionRule::RequestCount {
                window_seconds: right,
                ..
            },
        ) => left == right,
        (AdmissionRule::ConcurrentRequests { .. }, AdmissionRule::ConcurrentRequests { .. }) => {
            true
        }
        (
            AdmissionRule::UsageCostUsd { period: left, .. },
            AdmissionRule::UsageCostUsd { period: right, .. },
        ) => left == right,
        _ => false,
    }
}

fn union_grant(left: &AdmissionRule, right: &AdmissionRule) -> AdmissionRule {
    match (left, right) {
        (
            AdmissionRule::RequestCount {
                max_requests: left,
                window_seconds,
            },
            AdmissionRule::RequestCount {
                max_requests: right,
                ..
            },
        ) => AdmissionRule::RequestCount {
            max_requests: grant_u32(*left, *right),
            window_seconds: *window_seconds,
        },
        (
            AdmissionRule::ConcurrentRequests {
                max_concurrent: left,
            },
            AdmissionRule::ConcurrentRequests {
                max_concurrent: right,
            },
        ) => AdmissionRule::ConcurrentRequests {
            max_concurrent: grant_u32(*left, *right),
        },
        (
            AdmissionRule::UsageCostUsd {
                max_usd: left,
                period,
            },
            AdmissionRule::UsageCostUsd { max_usd: right, .. },
        ) => AdmissionRule::UsageCostUsd {
            max_usd: grant_f64(*left, *right),
            period: *period,
        },
        _ => right.clone(),
    }
}

fn grant_u32(left: u32, right: u32) -> u32 {
    if left == 0 || right == 0 {
        0
    } else {
        left.max(right)
    }
}

fn grant_f64(left: f64, right: f64) -> f64 {
    if left == 0.0 || right == 0.0 {
        0.0
    } else {
        left.max(right)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolvedAdmissionPolicy {
    pub principal: AdmissionPolicyDocument,
    pub api_key: AdmissionPolicyDocument,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredAdmissionPolicy {
    pub scope: AdmissionPolicyScope,
    pub document: AdmissionPolicyDocument,
}

#[async_trait]
pub trait AdmissionPolicyReadRepository: Send + Sync {
    async fn get_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<Option<StoredAdmissionPolicy>, crate::DataLayerError>;

    async fn list_policies(
        &self,
        scopes: &[AdmissionPolicyScope],
    ) -> Result<Vec<StoredAdmissionPolicy>, crate::DataLayerError>;
}

#[async_trait]
pub trait AdmissionPolicyWriteRepository: Send + Sync {
    async fn put_policy(
        &self,
        scope: &AdmissionPolicyScope,
        document: &AdmissionPolicyDocument,
    ) -> Result<StoredAdmissionPolicy, crate::DataLayerError>;

    async fn delete_policy(
        &self,
        scope: &AdmissionPolicyScope,
    ) -> Result<bool, crate::DataLayerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_rule_editing_preserves_other_rule_kinds() {
        let document = AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(120))
            .with_concurrent_requests(Some(4))
            .with_daily_usage_limit_usd(Some(12.5))
            .with_requests_per_minute(None);

        assert_eq!(document.requests_per_minute(), None);
        assert_eq!(document.concurrent_requests(), Some(4));
        assert_eq!(document.daily_usage_limit_usd(), Some(12.5));
        document.validate().expect("document should remain valid");
    }

    #[test]
    fn group_grants_choose_the_higher_limit_and_preserve_unlimited() {
        let basic = AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(60))
            .with_daily_usage_limit_usd(Some(10.0));
        let pro = AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(120))
            .with_daily_usage_limit_usd(Some(25.0));
        let unlimited = AdmissionPolicyDocument::default().with_requests_per_minute(Some(0));

        let grant = basic.union_grants(&pro).union_grants(&unlimited);
        assert_eq!(grant.requests_per_minute(), Some(0));
        assert_eq!(grant.daily_usage_limit_usd(), Some(25.0));
    }

    #[test]
    fn lower_scope_rule_overrides_the_same_upper_scope_rule() {
        let system = AdmissionPolicyDocument::default()
            .with_requests_per_minute(Some(120))
            .with_concurrent_requests(Some(8));
        let user = AdmissionPolicyDocument::default().with_requests_per_minute(Some(0));

        let resolved = system.overlay(&user);
        assert_eq!(resolved.requests_per_minute(), Some(0));
        assert_eq!(resolved.concurrent_requests(), Some(8));
    }
}
