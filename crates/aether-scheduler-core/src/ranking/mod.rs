mod modes;
mod priority;
mod reasons;
mod types;

use modes::compare_rankable_candidates;
use priority::candidate_priority_slot;
use reasons::promoted_by as ranking_promoted_by;
pub use reasons::RANKING_REASON_CACHED_AFFINITY;
pub use types::{
    SchedulerRankableCandidate, SchedulerRankingContext, SchedulerRankingMode,
    SchedulerRankingOutcome,
};

fn compare_candidate_identity_for_ranking(
    left: &SchedulerRankableCandidate,
    right: &SchedulerRankableCandidate,
) -> std::cmp::Ordering {
    left.provider_id
        .cmp(&right.provider_id)
        .then(left.endpoint_id.cmp(&right.endpoint_id))
        .then(left.key_id.cmp(&right.key_id))
        .then(
            left.selected_provider_model_name
                .cmp(&right.selected_provider_model_name),
        )
}

fn scheduler_candidate_ranking_order(
    candidates: &[SchedulerRankableCandidate],
    context: SchedulerRankingContext,
) -> Vec<usize> {
    let mut order = (0..candidates.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        compare_rankable_candidates(&candidates[*left], &candidates[*right], context)
    });
    order
}

fn scheduler_ranking_outcomes(
    candidates: &[SchedulerRankableCandidate],
    context: SchedulerRankingContext,
) -> Vec<SchedulerRankingOutcome> {
    scheduler_candidate_ranking_order(candidates, context)
        .into_iter()
        .enumerate()
        .map(|(ranking_index, original_index)| {
            let candidate = &candidates[original_index];
            SchedulerRankingOutcome {
                original_index,
                ranking_index,
                priority_mode: context.priority_mode,
                ranking_mode: context.ranking_mode,
                priority_slot: candidate_priority_slot(candidate, context.priority_mode),
                promoted_by: ranking_promoted_by(candidate, context.ranking_mode),
            }
        })
        .collect()
}

pub fn apply_scheduler_candidate_ranking<T>(
    items: &mut [T],
    candidates: &[SchedulerRankableCandidate],
    context: SchedulerRankingContext,
) -> Vec<SchedulerRankingOutcome> {
    let outcomes = scheduler_ranking_outcomes(candidates, context);
    apply_order(
        items,
        outcomes
            .iter()
            .map(|outcome| outcome.original_index)
            .collect(),
    );
    outcomes
}

fn apply_order<T>(items: &mut [T], sorted_old_indices: Vec<usize>) {
    if items.len() < 2 {
        return;
    }

    let mut target_positions = vec![0usize; sorted_old_indices.len()];
    for (new_position, old_position) in sorted_old_indices.into_iter().enumerate() {
        target_positions[old_position] = new_position;
    }

    for index in 0..items.len() {
        while target_positions[index] != index {
            let target = target_positions[index];
            items.swap(index, target);
            target_positions.swap(index, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SchedulerPriorityMode;

    fn candidate(
        id: &str,
        provider_priority: i32,
        key_priority: i32,
        global_key_priority: Option<i32>,
    ) -> SchedulerRankableCandidate {
        SchedulerRankableCandidate {
            provider_id: format!("provider-{id}"),
            endpoint_id: format!("endpoint-{id}"),
            key_id: format!("key-{id}"),
            selected_provider_model_name: "gpt-5".to_string(),
            provider_priority,
            key_internal_priority: key_priority,
            key_global_priority_for_format: global_key_priority,
            capability_priority: (0, 0),
            cached_affinity_match: false,
            affinity_hash: None,
            health_bucket: None,
            health_score: 1.0,
            original_index: 0,
        }
    }

    fn ranked_ids(
        candidates: &[SchedulerRankableCandidate],
        context: SchedulerRankingContext,
    ) -> Vec<String> {
        scheduler_candidate_ranking_order(candidates, context)
            .into_iter()
            .map(|index| candidates[index].provider_id.clone())
            .collect()
    }

    fn ranked_keys(
        candidates: &[SchedulerRankableCandidate],
        context: SchedulerRankingContext,
    ) -> Vec<String> {
        scheduler_candidate_ranking_order(candidates, context)
            .into_iter()
            .map(|index| candidates[index].key_id.clone())
            .collect()
    }

    #[test]
    fn provider_priority_mode_prefers_provider_priority_slot() {
        let candidates = vec![
            candidate("global", 10, 0, Some(0)),
            candidate("provider", 0, 10, Some(10)),
        ];

        assert_eq!(
            ranked_ids(
                &candidates,
                SchedulerRankingContext {
                    priority_mode: SchedulerPriorityMode::Provider,
                    ranking_mode: SchedulerRankingMode::FixedOrder,
                    include_health: false,
                    load_balance_seed: 0,
                },
            ),
            vec!["provider-provider", "provider-global"]
        );
    }

    #[test]
    fn global_key_priority_mode_prefers_global_key_priority_slot() {
        let candidates = vec![
            candidate("provider", 0, 10, Some(10)),
            candidate("global", 10, 0, Some(0)),
        ];

        assert_eq!(
            ranked_ids(
                &candidates,
                SchedulerRankingContext {
                    priority_mode: SchedulerPriorityMode::GlobalKey,
                    ranking_mode: SchedulerRankingMode::FixedOrder,
                    include_health: false,
                    load_balance_seed: 0,
                },
            ),
            vec!["provider-global", "provider-provider"]
        );
    }

    #[test]
    fn capability_priority_precedes_provider_priority() {
        let matching_capability = candidate("matching", 10, 0, None);
        let mut missing_compatible_capability = candidate("missing", 0, 0, None);
        missing_compatible_capability.capability_priority = (0, 1);

        assert_eq!(
            ranked_ids(
                &[missing_compatible_capability, matching_capability],
                SchedulerRankingContext {
                    priority_mode: SchedulerPriorityMode::Provider,
                    ranking_mode: SchedulerRankingMode::FixedOrder,
                    include_health: false,
                    load_balance_seed: 0,
                },
            ),
            vec!["provider-matching", "provider-missing"]
        );
    }

    #[test]
    fn cache_affinity_can_promote_cached_candidate_and_reports_reason() {
        let high_priority = candidate("high", 0, 0, Some(0));
        let mut cached = candidate("cached", 10, 0, Some(10));
        cached.cached_affinity_match = true;
        let candidates = vec![high_priority, cached];
        let context = SchedulerRankingContext {
            priority_mode: SchedulerPriorityMode::Provider,
            ranking_mode: SchedulerRankingMode::CacheAffinity,
            include_health: false,
            load_balance_seed: 0,
        };

        let outcomes = scheduler_ranking_outcomes(&candidates, context);
        assert_eq!(outcomes[0].original_index, 1);
        assert_eq!(
            outcomes[0].promoted_by,
            Some(RANKING_REASON_CACHED_AFFINITY)
        );
    }

    #[test]
    fn fixed_order_randomizes_equal_priority_ties_without_crossing_priority_slots() {
        let first = candidate("first", 0, 0, Some(0));
        let second = candidate("second", 0, 0, Some(0));
        let lower_priority = candidate("lower", 10, 0, Some(10));

        let first_seed_order = ranked_ids(
            &[first.clone(), second.clone(), lower_priority.clone()],
            SchedulerRankingContext {
                priority_mode: SchedulerPriorityMode::Provider,
                ranking_mode: SchedulerRankingMode::FixedOrder,
                include_health: false,
                load_balance_seed: 0,
            },
        );
        let alternate_order = (1..128)
            .map(|seed| {
                ranked_ids(
                    &[first.clone(), second.clone(), lower_priority.clone()],
                    SchedulerRankingContext {
                        priority_mode: SchedulerPriorityMode::Provider,
                        ranking_mode: SchedulerRankingMode::FixedOrder,
                        include_health: false,
                        load_balance_seed: seed,
                    },
                )
            })
            .find(|order| order[0] != first_seed_order[0])
            .expect("equal priority tie should vary by seed");

        assert_eq!(first_seed_order[2], "provider-lower");
        assert_eq!(alternate_order[2], "provider-lower");
    }

    #[test]
    fn load_balance_provider_mode_randomizes_providers_then_uses_internal_key_priority() {
        let mut provider_a_primary = candidate("a-primary", 0, 0, Some(0));
        provider_a_primary.provider_id = "provider-a".to_string();
        provider_a_primary.key_id = "key-a-primary".to_string();
        let mut provider_a_secondary = candidate("a-secondary", 0, 10, Some(10));
        provider_a_secondary.provider_id = "provider-a".to_string();
        provider_a_secondary.key_id = "key-a-secondary".to_string();
        let mut provider_b = candidate("b", 100, 0, Some(100));
        provider_b.provider_id = "provider-b".to_string();
        provider_b.key_id = "key-b".to_string();
        let candidates = vec![provider_a_secondary, provider_b, provider_a_primary];

        let seed = (0..512)
            .find(|seed| {
                ranked_keys(
                    &candidates,
                    SchedulerRankingContext {
                        priority_mode: SchedulerPriorityMode::Provider,
                        ranking_mode: SchedulerRankingMode::LoadBalance,
                        include_health: false,
                        load_balance_seed: *seed,
                    },
                )
                .first()
                .is_some_and(|key| key == "key-b")
            })
            .expect("test seed should put provider-b first");

        let order = ranked_keys(
            &candidates,
            SchedulerRankingContext {
                priority_mode: SchedulerPriorityMode::Provider,
                ranking_mode: SchedulerRankingMode::LoadBalance,
                include_health: false,
                load_balance_seed: seed,
            },
        );

        assert_eq!(order[0], "key-b");
        let primary_index = order
            .iter()
            .position(|key| key == "key-a-primary")
            .expect("primary key should be ranked");
        let secondary_index = order
            .iter()
            .position(|key| key == "key-a-secondary")
            .expect("secondary key should be ranked");
        assert!(primary_index < secondary_index);
    }

    #[test]
    fn load_balance_global_key_mode_randomizes_keys_ignoring_global_priority() {
        let high_priority = candidate("high", 100, 0, Some(0));
        let low_priority = candidate("low", 0, 0, Some(100));
        let candidates = vec![high_priority, low_priority];

        let seed = (0..512)
            .find(|seed| {
                ranked_ids(
                    &candidates,
                    SchedulerRankingContext {
                        priority_mode: SchedulerPriorityMode::GlobalKey,
                        ranking_mode: SchedulerRankingMode::LoadBalance,
                        include_health: false,
                        load_balance_seed: *seed,
                    },
                )
                .first()
                .is_some_and(|provider| provider == "provider-low")
            })
            .expect("test seed should put lower global-priority key first");

        assert_eq!(
            ranked_ids(
                &candidates,
                SchedulerRankingContext {
                    priority_mode: SchedulerPriorityMode::GlobalKey,
                    ranking_mode: SchedulerRankingMode::LoadBalance,
                    include_health: false,
                    load_balance_seed: seed,
                },
            ),
            vec!["provider-low", "provider-high"]
        );
    }
}
