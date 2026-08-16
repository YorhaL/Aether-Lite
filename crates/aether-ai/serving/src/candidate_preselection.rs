#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiCandidatePreselectionOutcome<Candidate, Skipped> {
    pub candidates: Vec<Candidate>,
    pub skipped_candidates: Vec<Skipped>,
}
