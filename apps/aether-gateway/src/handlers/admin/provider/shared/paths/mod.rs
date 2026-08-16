mod crud;
mod endpoint_keys;

pub(crate) use self::crud::{
    admin_provider_assign_global_models_path, admin_provider_available_source_models_path,
    admin_provider_delete_task_parts, admin_provider_id_for_health_monitor,
    admin_provider_id_for_manage_path, admin_provider_id_for_mapping_preview,
    admin_provider_id_for_models_list, admin_provider_id_for_summary,
    admin_provider_import_models_path, admin_provider_model_route_parts,
    admin_provider_models_batch_path, is_admin_providers_root,
};
pub(crate) use self::endpoint_keys::{
    admin_provider_id_for_keys, admin_reveal_key_id, admin_update_key_id,
};
