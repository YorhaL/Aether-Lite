use aether_scheduler_core::SchedulerMinimalCandidateSelectionCandidate;

pub(crate) fn resolve_candidate_mapped_model(
    candidate: &SchedulerMinimalCandidateSelectionCandidate,
) -> Result<String, &'static str> {
    let mapped_model = candidate.selected_provider_model_name.trim().to_string();
    if mapped_model.is_empty() {
        return Err("mapped_model_missing");
    }
    Ok(mapped_model)
}
