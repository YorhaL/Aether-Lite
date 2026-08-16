use crate::handlers::admin::shared::query_param_value;

pub(super) fn parse_admin_monitoring_offset(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "offset") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| "offset must be a non-negative integer".to_string()),
        None => Ok(0),
    }
}

pub(super) fn parse_admin_monitoring_limit(query: Option<&str>) -> Result<usize, String> {
    match query_param_value(query, "limit") {
        Some(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "limit must be an integer between 1 and 1000".to_string())?;
            if (1..=1000).contains(&parsed) {
                Ok(parsed)
            } else {
                Err("limit must be an integer between 1 and 1000".to_string())
            }
        }
        None => Ok(100),
    }
}
