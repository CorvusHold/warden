use crate::error::Error;
use crate::monitors;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpRequestMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitorType {
    HTTP,
    DNS,
    PING,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub monitor_type: MonitorType,
    pub url: String,

    // HTTP specific fields
    pub http_method: Option<HttpRequestMethod>,
    pub payload: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub verify_ssl: Option<bool>,
    pub expected_status_code: Option<u16>,
    pub expected_body: Option<String>,

    // DNS specific fields
    pub dns_record_type: Option<String>,
    pub expected_ip: Option<String>,

    // PING specific fields
    pub ping_count: Option<u8>,

    // Common fields
    pub interval: u32,
    pub timeout: u32,
    pub retry: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorResult {
    pub service_id: String,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub response_time: u128,
    pub error: Option<String>,
    pub details: Option<String>,
}

impl Service {
    pub async fn exec(&self) -> Result<MonitorResult, Error> {
        match self.monitor_type {
            MonitorType::HTTP => monitors::http::exec(self).await,
            MonitorType::DNS => monitors::dns::exec(self).await,
            MonitorType::PING => monitors::ping::exec(self).await,
        }
    }
}
