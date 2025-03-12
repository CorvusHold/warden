use chrono::Utc;
use reqwest::{Client, Method, RequestBuilder};
use std::time::{Duration, Instant};

use super::ssl;
use crate::error::Error;
use crate::models::service::{HttpRequestMethod, MonitorResult, MonitorType, Service};

pub async fn exec(service: &Service) -> Result<MonitorResult, Error> {
    if service.monitor_type != MonitorType::HTTP {
        return Err(Error::InvalidServiceConfig(
            "Service is not an HTTP monitor".to_string(),
        ));
    } else if service.http_method.is_none() {
        return Err(Error::InvalidServiceConfig(
            "HTTP method not specified".to_string(),
        ));
    }

    let beginning = Instant::now();
    let result = match service.http_method.as_ref() {
        Some(HttpRequestMethod::GET) => get(service).await,
        Some(HttpRequestMethod::POST) => post(service).await,
        Some(HttpRequestMethod::PUT) => put(service).await,
        Some(HttpRequestMethod::PATCH) => patch(service).await,
        Some(HttpRequestMethod::DELETE) => delete(service).await,
        _ => Err(Error::InvalidServiceConfig(
            "Invalid HTTP method".to_string(),
        )),
    };

    let response_time = beginning.elapsed().as_millis();

    match result {
        Ok(details) => Ok(MonitorResult {
            service_id: service.id.clone(),
            timestamp: Utc::now(),
            success: true,
            response_time,
            error: None,
            details: Some(details),
        }),
        Err(err) => Ok(MonitorResult {
            service_id: service.id.clone(),
            timestamp: Utc::now(),
            success: false,
            response_time,
            error: Some(err.to_string()),
            details: None,
        }),
    }
}

async fn build_client(service: &Service) -> Result<Client, Error> {
    let mut client_builder = Client::builder().timeout(Duration::from_secs(service.timeout as u64));

    // Handle SSL verification if specified
    if let Some(verify_ssl) = service.verify_ssl {
        if !verify_ssl {
            client_builder = client_builder.danger_accept_invalid_certs(true);
        } else {
            // Check SSL certificate validity
            if let Err(e) = ssl::verify_ssl(&service.url).await {
                return Err(e);
            }
        }
    }

    client_builder.build().map_err(Error::Reqwest)
}

async fn build_request(client: &Client, service: &Service, method: Method) -> RequestBuilder {
    // Clone the method to avoid ownership issues
    let method_clone = method.clone();
    let mut request = client.request(method, &service.url);

    // Add headers if specified
    if let Some(headers) = &service.headers {
        for (name, value) in headers {
            request = request.header(name, value);
        }
    }

    // Add payload if specified and method is not GET
    if method_clone != Method::GET {
        if let Some(payload) = &service.payload {
            request = request.body(payload.clone());
        }
    }

    request
}

async fn validate_response(
    service: &Service,
    response: reqwest::Response,
) -> Result<String, Error> {
    let status = response.status();
    let expected_status = service.expected_status_code.unwrap_or(200);

    if status.as_u16() != expected_status {
        return Err(Error::UnexpectedStatusCode {
            expected: expected_status,
            actual: status.as_u16(),
        });
    }

    let body = response.text().await.map_err(Error::Reqwest)?;

    // Check expected body if specified
    if let Some(expected_body) = &service.expected_body {
        if !body.contains(expected_body) {
            return Err(Error::ExpectedBodyNotFound);
        }
    }

    Ok(format!("Status: {}, Body length: {}", status, body.len()))
}

async fn get(service: &Service) -> Result<String, Error> {
    let client = build_client(service).await?;
    let request = build_request(&client, service, Method::GET).await;
    let response = request.send().await.map_err(Error::Reqwest)?;
    validate_response(service, response).await
}

async fn post(service: &Service) -> Result<String, Error> {
    let client = build_client(service).await?;
    let request = build_request(&client, service, Method::POST).await;
    let response = request.send().await.map_err(Error::Reqwest)?;
    validate_response(service, response).await
}

async fn put(service: &Service) -> Result<String, Error> {
    let client = build_client(service).await?;
    let request = build_request(&client, service, Method::PUT).await;
    let response = request.send().await.map_err(Error::Reqwest)?;
    validate_response(service, response).await
}

async fn patch(service: &Service) -> Result<String, Error> {
    let client = build_client(service).await?;
    let request = build_request(&client, service, Method::PATCH).await;
    let response = request.send().await.map_err(Error::Reqwest)?;
    validate_response(service, response).await
}

async fn delete(service: &Service) -> Result<String, Error> {
    let client = build_client(service).await?;
    let request = build_request(&client, service, Method::DELETE).await;
    let response = request.send().await.map_err(Error::Reqwest)?;
    validate_response(service, response).await
}
