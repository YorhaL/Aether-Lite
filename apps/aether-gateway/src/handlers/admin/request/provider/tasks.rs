use super::*;

impl<'a> AdminAppState<'a> {
    pub(crate) fn put_provider_delete_task(&self, task: crate::LocalProviderDeleteTaskState) {
        self.app.put_provider_delete_task(task)
    }

    pub(crate) async fn run_admin_provider_delete_task(
        &self,
        provider_id: &str,
        task_id: &str,
    ) -> Result<crate::LocalProviderDeleteTaskState, GatewayError> {
        crate::handlers::admin::provider::delete_task::run_admin_provider_delete_task(
            self,
            provider_id,
            task_id,
        )
        .await
    }

    pub(crate) fn get_provider_delete_task(
        &self,
        task_id: &str,
    ) -> Option<crate::LocalProviderDeleteTaskState> {
        self.app.get_provider_delete_task(task_id)
    }
}
