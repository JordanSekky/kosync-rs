use axum::{Json, extract::State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    db::Db, error::Result, extract::AppQuery, models::admin::Device, time::datetime_from_ns,
};

#[derive(Deserialize, JsonSchema, Debug)]
pub struct ListDevicesQuery {
    /// Only devices this user has pushed from
    user_id: Option<i64>,
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct DeviceResponse {
    device: String,
    /// Null for clients that don't send one
    device_id: Option<String>,
    /// Number of progress history entries pushed from the device
    entry_count: i64,
    last_seen_at: DateTime<Utc>,
}

impl From<Device> for DeviceResponse {
    fn from(device: Device) -> Self {
        Self {
            device: device.device,
            device_id: device.device_id,
            entry_count: device.entry_count,
            last_seen_at: datetime_from_ns(device.last_seen_ns),
        }
    }
}

#[derive(Serialize, JsonSchema, Debug)]
pub struct ListDevicesResponse {
    /// Most recently seen first
    devices: Vec<DeviceResponse>,
}

#[tracing::instrument(skip_all)]
pub async fn list(
    State(db): State<Db>,
    AppQuery(query): AppQuery<ListDevicesQuery>,
) -> Result<Json<ListDevicesResponse>> {
    let devices = Device::list(&db, query.user_id).await?;

    Ok(Json(ListDevicesResponse {
        devices: devices.into_iter().map(Into::into).collect(),
    }))
}
