pub(in super::super) fn normalize_admin_wallet_non_zero_amount(
    value: f64,
    field_name: &str,
) -> Result<f64, String> {
    if !value.is_finite() || value == 0.0 {
        return Err(format!("{field_name} 不能为 0，且必须为有限数字"));
    }
    Ok(value)
}
