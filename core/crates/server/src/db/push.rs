//! Push notification token storage (stub — not yet implemented).

/// Look up the push pseudonym registered for a device, if any.
/// Returns `None` if the device has no push token registered.
pub async fn pseudonym_for_device(
    _conn: &mut sqlx::PgConnection,
    _device_pk: i64,
) -> Result<Option<String>, sqlx::Error> {
    Ok(None)
}
