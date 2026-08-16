use super::{AdminAppState, AdminCancelVideoTaskError};
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

    pub(crate) async fn count_distinct_video_task_users(
        &self,
        filter: &aether_data_contracts::repository::video_tasks::VideoTaskQueryFilter,
    ) -> Result<u64, GatewayError> {
        self.app.count_distinct_video_task_users(filter).await
    }

    pub(crate) async fn read_video_task_page(
        &self,
        filter: &aether_data_contracts::repository::video_tasks::VideoTaskQueryFilter,
        page: usize,
        page_size: usize,
    ) -> Result<crate::async_task::VideoTaskPageResponse, GatewayError> {
        crate::async_task::read_video_task_page(self.app, filter, page, page_size).await
    }

    pub(crate) async fn read_video_task_page_summary(
        &self,
        filter: &aether_data_contracts::repository::video_tasks::VideoTaskQueryFilter,
        page: usize,
        page_size: usize,
    ) -> Result<crate::async_task::VideoTaskPageResponse, GatewayError> {
        crate::async_task::read_video_task_page_summary(self.app, filter, page, page_size).await
    }

    pub(crate) async fn read_video_task_stats(
        &self,
        filter: &aether_data_contracts::repository::video_tasks::VideoTaskQueryFilter,
        now_unix_secs: u64,
    ) -> Result<crate::async_task::VideoTaskStatsResponse, GatewayError> {
        crate::async_task::read_video_task_stats(self.app, filter, now_unix_secs).await
    }

    pub(crate) async fn cancel_video_task_record(
        &self,
        task_id: &str,
    ) -> Result<
        aether_data_contracts::repository::video_tasks::StoredVideoTask,
        AdminCancelVideoTaskError,
    > {
        crate::async_task::cancel_video_task_record(self.app, task_id)
            .await
            .map_err(|err| match err {
                crate::async_task::CancelVideoTaskError::NotFound => {
                    AdminCancelVideoTaskError::NotFound
                }
                crate::async_task::CancelVideoTaskError::InvalidStatus(status) => {
                    AdminCancelVideoTaskError::InvalidStatus(status)
                }
                crate::async_task::CancelVideoTaskError::Response(response) => {
                    AdminCancelVideoTaskError::Response(response)
                }
                crate::async_task::CancelVideoTaskError::Gateway(err) => {
                    AdminCancelVideoTaskError::Gateway(err)
                }
            })
    }

    pub(crate) async fn read_video_task_detail(
        &self,
        task_id: &str,
    ) -> Result<Option<aether_data_contracts::repository::video_tasks::StoredVideoTask>, GatewayError>
    {
        crate::async_task::read_video_task_detail(self.app, task_id).await
    }

    pub(crate) async fn read_video_task_video_source(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::async_task::VideoTaskVideoSource>, GatewayError> {
        crate::async_task::read_video_task_video_source(self.app, task_id).await
    }

    pub(crate) async fn build_video_task_video_response(
        &self,
        source: crate::async_task::VideoTaskVideoSource,
    ) -> Result<axum::response::Response, GatewayError> {
        crate::async_task::build_video_task_video_response(source).await
    }
}
