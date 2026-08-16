use super::types::{SchedulerRankableCandidate, SchedulerRankingMode};

pub const RANKING_REASON_CACHED_AFFINITY: &str = "cached_affinity";

pub fn promoted_by(
    candidate: &SchedulerRankableCandidate,
    ranking_mode: SchedulerRankingMode,
) -> Option<&'static str> {
    if ranking_mode == SchedulerRankingMode::CacheAffinity && candidate.cached_affinity_match {
        return Some(RANKING_REASON_CACHED_AFFINITY);
    }
    None
}
