use super::AdminAppState;
use crate::GatewayError;

impl<'a> AdminAppState<'a> {
    pub(crate) async fn list_background_task_runs(
        &self,
        query: &aether_data_contracts::repository::background_tasks::BackgroundTaskListQuery,
    ) -> Result<
        aether_data_contracts::repository::background_tasks::StoredBackgroundTaskRunPage,
        GatewayError,
    > {
        self.app.list_background_task_runs(query).await
    }

    pub(crate) async fn find_background_task_run(
        &self,
        run_id: &str,
    ) -> Result<
        Option<aether_data_contracts::repository::background_tasks::StoredBackgroundTaskRun>,
        GatewayError,
    > {
        self.app.find_background_task_run(run_id).await
    }

    pub(crate) async fn list_background_task_events(
        &self,
        run_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<
        Vec<aether_data_contracts::repository::background_tasks::StoredBackgroundTaskEvent>,
        GatewayError,
    > {
        self.app
            .list_background_task_events(run_id, offset, limit)
            .await
    }

    pub(crate) async fn summarize_background_task_runs(
        &self,
    ) -> Result<
        aether_data_contracts::repository::background_tasks::BackgroundTaskSummary,
        GatewayError,
    > {
        self.app.summarize_background_task_runs().await
    }

    pub(crate) async fn request_cancel_background_task_run(
        &self,
        run_id: &str,
        updated_at_unix_secs: u64,
    ) -> Result<bool, GatewayError> {
        self.app
            .request_cancel_background_task_run(run_id, updated_at_unix_secs)
            .await
    }
}
