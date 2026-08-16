use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiCandidateResolutionRequest<'a> {
    pub client_api_format: &'a str,
    pub requested_model: Option<&'a str>,
}

impl<'a> AiCandidateResolutionRequest<'a> {
    pub fn standard(client_api_format: &'a str, requested_model: Option<&'a str>) -> Self {
        Self {
            client_api_format,
            requested_model,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiCandidateResolutionOutcome<Eligible, Skipped> {
    pub eligible_candidates: Vec<Eligible>,
    pub skipped_candidates: Vec<Skipped>,
}

#[async_trait]
pub trait AiCandidateResolutionPort: Send + Sync {
    type Candidate: Send;
    type Transport: Send + Sync;
    type Eligible: Send + Sync;
    type Skipped: Send;
    type Error: Send;

    async fn read_candidate_transport(
        &self,
        candidate: &Self::Candidate,
    ) -> Result<Option<Self::Transport>, Self::Error>;

    fn build_missing_transport_skipped_candidate(
        &self,
        candidate: Self::Candidate,
    ) -> Self::Skipped;

    fn candidate_common_skip_reason(
        &self,
        candidate: &Self::Candidate,
        transport: &Self::Transport,
        requested_model: Option<&str>,
    ) -> Option<&'static str>;

    fn candidate_transport_pair_skip_reason(
        &self,
        candidate: &Self::Candidate,
        transport: &Self::Transport,
        normalized_client_api_format: &str,
        requested_model: &str,
    ) -> Option<&'static str>;

    fn build_skipped_candidate(
        &self,
        candidate: Self::Candidate,
        transport: Self::Transport,
        skip_reason: &'static str,
    ) -> Self::Skipped;

    fn build_eligible_candidate(
        &self,
        candidate: Self::Candidate,
        transport: Self::Transport,
    ) -> Self::Eligible;

    async fn rank_eligible_candidates(
        &self,
        candidates: Vec<Self::Eligible>,
        normalized_client_api_format: &str,
    ) -> Result<Vec<Self::Eligible>, Self::Error>;
}

pub async fn run_ai_candidate_resolution<Port>(
    port: &Port,
    candidates: Vec<Port::Candidate>,
    request: AiCandidateResolutionRequest<'_>,
) -> Result<AiCandidateResolutionOutcome<Port::Eligible, Port::Skipped>, Port::Error>
where
    Port: AiCandidateResolutionPort,
{
    let normalized_client_api_format = request.client_api_format.trim().to_ascii_lowercase();
    let requested_model = request
        .requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut eligible = Vec::with_capacity(candidates.len());
    let mut skipped = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let Some(transport) = port.read_candidate_transport(&candidate).await? else {
            skipped.push(port.build_missing_transport_skipped_candidate(candidate));
            continue;
        };

        match candidate_skip_reason(
            port,
            &candidate,
            &transport,
            normalized_client_api_format.as_str(),
            requested_model,
        ) {
            Some(skip_reason) => {
                skipped.push(port.build_skipped_candidate(candidate, transport, skip_reason));
            }
            None => {
                eligible.push(port.build_eligible_candidate(candidate, transport));
            }
        }
    }

    let ranked = port
        .rank_eligible_candidates(eligible, normalized_client_api_format.as_str())
        .await?;
    Ok(AiCandidateResolutionOutcome {
        eligible_candidates: ranked,
        skipped_candidates: skipped,
    })
}

fn candidate_skip_reason<Port>(
    port: &Port,
    candidate: &Port::Candidate,
    transport: &Port::Transport,
    normalized_client_api_format: &str,
    requested_model: Option<&str>,
) -> Option<&'static str>
where
    Port: AiCandidateResolutionPort,
{
    port.candidate_common_skip_reason(candidate, transport, requested_model)
        .or_else(|| {
            port.candidate_transport_pair_skip_reason(
                candidate,
                transport,
                normalized_client_api_format,
                requested_model.unwrap_or_default(),
            )
        })
}
