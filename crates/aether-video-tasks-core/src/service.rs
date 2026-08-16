use std::path::PathBuf;
use std::sync::Arc;

use aether_contracts::ExecutionPlan;
use aether_data_contracts::repository::video_tasks::StoredVideoTask;
use serde_json::{Map, Value};

use crate::{
    extract_gemini_short_id_from_cancel_path, extract_gemini_short_id_from_path,
    extract_openai_task_id_from_cancel_path, extract_openai_task_id_from_content_path,
    extract_openai_task_id_from_path, extract_openai_task_id_from_remix_path,
    resolve_local_video_registry_mutation, FileVideoTaskStore, InMemoryVideoTaskStore,
    LocalVideoTaskContentAction, LocalVideoTaskFollowUpPlan, LocalVideoTaskProjectionTarget,
    LocalVideoTaskReadRefreshPlan, LocalVideoTaskReadResponse, LocalVideoTaskSeed,
    LocalVideoTaskSnapshot, LocalVideoTaskSuccessPlan, VideoTaskStore,
};

#[derive(Debug)]
pub struct VideoTaskService {
    store: Arc<dyn VideoTaskStore>,
}

impl VideoTaskService {
    pub fn new() -> Self {
        Self::with_store(Arc::new(InMemoryVideoTaskStore::default()))
    }

    pub fn with_file_store(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self::with_store(Arc::new(FileVideoTaskStore::new(path)?)))
    }

    fn with_store(store: Arc<dyn VideoTaskStore>) -> Self {
        Self { store }
    }

    pub fn prepare_sync_success(
        &self,
        report_kind: &str,
        provider_body: &Map<String, Value>,
        report_context: &Map<String, Value>,
        plan: &ExecutionPlan,
    ) -> Option<LocalVideoTaskSuccessPlan> {
        let seed = LocalVideoTaskSeed::from_sync_finalize(
            report_kind,
            provider_body,
            report_context,
            plan,
        )?;
        Some(LocalVideoTaskSuccessPlan { seed })
    }

    pub fn record_snapshot(&self, snapshot: LocalVideoTaskSnapshot) {
        self.store.insert(snapshot);
    }

    pub fn hydrate_from_stored_task(&self, task: &StoredVideoTask) -> bool {
        let Some(snapshot) = LocalVideoTaskSnapshot::from_stored_task(task) else {
            return false;
        };
        self.store.insert(snapshot);
        true
    }

    pub fn apply_finalize_mutation(&self, request_path: &str, report_kind: &str) {
        let Some(mutation) = resolve_local_video_registry_mutation(request_path, report_kind)
        else {
            return;
        };
        self.store.apply_mutation(mutation);
    }

    pub fn read_response(
        &self,
        route_family: Option<&str>,
        request_path: &str,
    ) -> Option<LocalVideoTaskReadResponse> {
        match route_family {
            Some("openai") => extract_openai_task_id_from_path(request_path)
                .and_then(|task_id| self.store.read_openai(task_id)),
            Some("gemini") => extract_gemini_short_id_from_path(request_path)
                .and_then(|short_id| self.store.read_gemini(short_id)),
            _ => None,
        }
    }

    pub fn snapshot_for_route(
        &self,
        route_family: Option<&str>,
        request_path: &str,
    ) -> Option<LocalVideoTaskSnapshot> {
        match route_family {
            Some("openai") => extract_openai_task_id_from_path(request_path)
                .and_then(|task_id| self.store.clone_openai(task_id))
                .map(LocalVideoTaskSnapshot::OpenAi),
            Some("gemini") => extract_gemini_short_id_from_path(request_path)
                .and_then(|short_id| self.store.clone_gemini(short_id))
                .map(LocalVideoTaskSnapshot::Gemini),
            _ => None,
        }
    }

    pub fn prepare_openai_content_stream_action(
        &self,
        request_path: &str,
        query_string: Option<&str>,
        trace_id: &str,
    ) -> Option<LocalVideoTaskContentAction> {
        let task_id = extract_openai_task_id_from_content_path(request_path)?;
        let seed = self.store.clone_openai(task_id)?;
        seed.build_content_stream_action(query_string, trace_id)
    }

    pub fn snapshot_for_refresh_plan(
        &self,
        refresh_plan: &LocalVideoTaskReadRefreshPlan,
    ) -> Option<LocalVideoTaskSnapshot> {
        match &refresh_plan.projection_target {
            LocalVideoTaskProjectionTarget::OpenAi { task_id } => self
                .store
                .clone_openai(task_id)
                .map(LocalVideoTaskSnapshot::OpenAi),
            LocalVideoTaskProjectionTarget::Gemini { short_id } => self
                .store
                .clone_gemini(short_id)
                .map(LocalVideoTaskSnapshot::Gemini),
        }
    }

    pub fn project_openai_task_response(
        &self,
        task_id: &str,
        provider_body: &Map<String, Value>,
    ) -> bool {
        self.store.project_openai(task_id, provider_body)
    }

    pub fn project_gemini_task_response(
        &self,
        short_id: &str,
        provider_body: &Map<String, Value>,
    ) -> bool {
        self.store.project_gemini(short_id, provider_body)
    }

    pub fn prepare_read_refresh_sync_plan(
        &self,
        route_family: Option<&str>,
        request_path: &str,
        trace_id: &str,
    ) -> Option<LocalVideoTaskReadRefreshPlan> {
        match route_family {
            Some("openai") => {
                let task_id = extract_openai_task_id_from_path(request_path)?;
                let seed = self.store.clone_openai(task_id)?;
                Some(LocalVideoTaskReadRefreshPlan {
                    plan: seed.build_get_follow_up_plan(trace_id)?,
                    projection_target: LocalVideoTaskProjectionTarget::OpenAi {
                        task_id: task_id.to_string(),
                    },
                })
            }
            Some("gemini") => {
                let short_id = extract_gemini_short_id_from_path(request_path)?;
                let seed = self.store.clone_gemini(short_id)?;
                Some(LocalVideoTaskReadRefreshPlan {
                    plan: seed.build_get_follow_up_plan(trace_id)?,
                    projection_target: LocalVideoTaskProjectionTarget::Gemini {
                        short_id: short_id.to_string(),
                    },
                })
            }
            _ => None,
        }
    }

    pub fn prepare_poll_refresh_batch(
        &self,
        limit: usize,
        trace_prefix: &str,
    ) -> Vec<LocalVideoTaskReadRefreshPlan> {
        if limit == 0 {
            return Vec::new();
        }

        self.store
            .list_active_snapshots(limit)
            .into_iter()
            .enumerate()
            .filter_map(|(index, snapshot)| {
                let trace_id = format!("{trace_prefix}-{index}");
                match snapshot {
                    LocalVideoTaskSnapshot::OpenAi(seed) => Some(LocalVideoTaskReadRefreshPlan {
                        plan: seed.build_get_follow_up_plan(&trace_id)?,
                        projection_target: LocalVideoTaskProjectionTarget::OpenAi {
                            task_id: seed.local_task_id.clone(),
                        },
                    }),
                    LocalVideoTaskSnapshot::Gemini(seed) => Some(LocalVideoTaskReadRefreshPlan {
                        plan: seed.build_get_follow_up_plan(&trace_id)?,
                        projection_target: LocalVideoTaskProjectionTarget::Gemini {
                            short_id: seed.local_short_id.clone(),
                        },
                    }),
                }
            })
            .collect()
    }

    pub fn prepare_poll_refresh_plan_for_stored_task(
        &self,
        task: &StoredVideoTask,
        trace_id: &str,
    ) -> Option<LocalVideoTaskReadRefreshPlan> {
        let snapshot = LocalVideoTaskSnapshot::from_stored_task(task)?;
        match snapshot {
            LocalVideoTaskSnapshot::OpenAi(seed) => Some(LocalVideoTaskReadRefreshPlan {
                plan: seed.build_get_follow_up_plan(trace_id)?,
                projection_target: LocalVideoTaskProjectionTarget::OpenAi {
                    task_id: seed.local_task_id.clone(),
                },
            }),
            LocalVideoTaskSnapshot::Gemini(seed) => Some(LocalVideoTaskReadRefreshPlan {
                plan: seed.build_get_follow_up_plan(trace_id)?,
                projection_target: LocalVideoTaskProjectionTarget::Gemini {
                    short_id: seed.local_short_id.clone(),
                },
            }),
        }
    }

    pub fn apply_read_refresh_projection(
        &self,
        refresh_plan: &LocalVideoTaskReadRefreshPlan,
        provider_body: &Map<String, Value>,
    ) -> bool {
        match &refresh_plan.projection_target {
            LocalVideoTaskProjectionTarget::OpenAi { task_id } => {
                self.project_openai_task_response(task_id, provider_body)
            }
            LocalVideoTaskProjectionTarget::Gemini { short_id } => {
                self.project_gemini_task_response(short_id, provider_body)
            }
        }
    }

    pub fn prepare_follow_up_sync_plan(
        &self,
        plan_kind: &str,
        request_path: &str,
        body_json: Option<&Value>,
        fallback_user_id: Option<&str>,
        fallback_api_key_id: Option<&str>,
        trace_id: &str,
    ) -> Option<LocalVideoTaskFollowUpPlan> {
        match plan_kind {
            "openai_video_remix_sync" => {
                let task_id = extract_openai_task_id_from_remix_path(request_path)?;
                let seed = self.store.clone_openai(task_id)?;
                seed.build_remix_follow_up_plan(
                    body_json?,
                    fallback_user_id,
                    fallback_api_key_id,
                    trace_id,
                )
            }
            "openai_video_delete_sync" => {
                let task_id = extract_openai_task_id_from_path(request_path)?;
                let seed = self.store.clone_openai(task_id)?;
                seed.build_delete_follow_up_plan(fallback_user_id, fallback_api_key_id, trace_id)
            }
            "openai_video_cancel_sync" => {
                let task_id = extract_openai_task_id_from_cancel_path(request_path)?;
                let seed = self.store.clone_openai(task_id)?;
                seed.build_cancel_follow_up_plan(fallback_user_id, fallback_api_key_id, trace_id)
            }
            "gemini_video_cancel_sync" => {
                let short_id = extract_gemini_short_id_from_cancel_path(request_path)?;
                let seed = self.store.clone_gemini(short_id)?;
                seed.build_cancel_follow_up_plan(fallback_user_id, fallback_api_key_id, trace_id)
            }
            _ => None,
        }
    }
}
