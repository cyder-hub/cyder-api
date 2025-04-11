use std::{
    io::Read,
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, Request},
    http::{HeaderMap, HeaderValue},
    response::Response,
    routing::any,
    Router,
};
use bytes::BytesMut;
use chrono::Utc;
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST},
    Method, Proxy, StatusCode, Url,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::database::provider::CustomField;

use crate::{
    config::CONFIG,
    database::{
        api_key::ApiKey,
        model::{Model, Price}, // Import Price
        model_transform::ModelTransform,
        record::{ModelInfo, Record, TimeInfo, UsageInfo},
    },
    utils::{process_stream_options, remove_data_field, split_chunks},
};

fn check_header_auth(headers: &HeaderMap) -> Result<i64, (StatusCode, String)> {
    let auth = match headers.get(AUTHORIZATION) {
        Some(value) => value,
        None => return Err((StatusCode::UNAUTHORIZED, "no api key".to_string())),
    };
    let auth_str = auth.to_str().unwrap_or("");
    let auth_str = auth_str.strip_prefix("Bearer ");

    if let Some(auth_str) = auth_str {
        let api_key = ApiKey::query_by_key(auth_str);
        if let Ok(api_key) = api_key {
            return Ok(api_key.id);
        }
    }
    Err((StatusCode::UNAUTHORIZED, "api key is invalid".to_string()))
}

fn build_new_headers(
    pre_headers: &HeaderMap,
    api_key: &str,
) -> Result<HeaderMap, (StatusCode, String)> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in pre_headers.iter() {
        if name != HOST && name != CONTENT_LENGTH && name != ACCEPT_ENCODING {
            headers.insert(name.clone(), value.clone());
        }
    }
    let request_key = format!("Bearer {}", api_key);
    headers.insert(AUTHORIZATION, HeaderValue::try_from(request_key).unwrap());
    Ok(headers)
}

fn parse_usage_info(usage: Option<&Value>) -> Option<UsageInfo> {
    if let Some(usage) = usage {
        if usage.is_null() {
            None
        } else {
            let prompt = usage.get("prompt_tokens").unwrap().as_i64().unwrap_or(0);
            let completion = usage
                .get("completion_tokens")
                .unwrap()
                .as_i64()
                .unwrap_or(0);
            Some(UsageInfo {
                prompt_tokens: prompt as i32,
                prompt_cache_tokens: 0,
                completion_tokens: completion as i32,
                prompt_audio_tokens: 0,
                reasoning_tokens: 0,
            })
        }
    } else {
        None
    }
}

struct RequestInfo {
    api_key_id: i64,
    provider_id: i64,
    provider_key: String,
    model_id: Option<i64>,
    model_name: String,
    real_model_name: String,
    price: Option<Price>, // Add price field
}

impl RequestInfo {
    fn to_model_info(self: &Self) -> ModelInfo {
        ModelInfo {
            provider_id: self.provider_id,
            model_id: self.model_id,
            model_name: self.model_name.to_string(),
            real_model_name: self.real_model_name.to_string(),
        }
    }
}

async fn proxy_request(
    url: &str,
    data: String,
    method: Method,
    headers: HeaderMap,
    request_info: RequestInfo,
    use_proxy: bool,
    start_time: i64,
) -> Result<Response<Body>, (StatusCode, String)> {
    let model_str = format!(
        "{}/{}{}",
        &request_info.provider_key,
        &request_info.model_name,
        if request_info.model_name.eq(&request_info.real_model_name) {
            "".to_string()
        } else {
            format!("({})", &request_info.real_model_name)
        }
    );
    let api_key_id = request_info.api_key_id;
    let model_info: ModelInfo = request_info.to_model_info();
    // build http client with proxy
    let client = if use_proxy {
        let proxy = Proxy::https(&CONFIG.proxy.url).unwrap();
        reqwest::Client::builder().proxy(proxy).build().unwrap()
    } else {
        reqwest::Client::new()
    };

    // proxy request
    let response = client
        .request(method, url)
        .headers(headers)
        .body(data)
        .send()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "bad gateway".to_string()))?;
    // get content_type add content_length
    let is_sse = response
        .headers()
        .get(CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/event-stream");

    let is_gzip = if let Some(encoding) = response.headers().get(CONTENT_ENCODING) {
        encoding.to_str().unwrap_or("").contains("gzip")
    } else {
        false
    };
    let status_code = response.status();

    // build new header
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response.headers().iter() {
        if name != CONTENT_LENGTH {
            // remove content length, or grok will not stop after request
            response_builder = response_builder.header(name, value);
        }
    }
    let mut first_response: i64 = 0;
    let mut total_bytes = BytesMut::new();

    let latest_chunk: Arc<Mutex<Option<Bytes>>> = Arc::new(Mutex::new(None::<Bytes>));
    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let _ = tx.send(chunk).await;
        }
    });

    let monitored_stream = async_stream::stream! {
        while let Some(chunk) = rx.recv().await {
            match chunk {
                Ok(chunk) => {
                    if is_sse {
                        let now = Utc::now().timestamp_millis() - start_time;
                        if first_response == 0 {
                            first_response = now;
                        }
                        let multi_chunks = split_chunks(chunk.slice(0..chunk.len()));
                        for chunk in multi_chunks {
                            // ignore messages which length > 20
                            let line_str = if chunk.len() < 20 {
                                String::from_utf8_lossy(&chunk).to_string()
                            } else {
                                "".to_string()
                            };
                            if !line_str.is_empty() && line_str.trim() == "data: [DONE]" {
                                if let Some(latest_chunk) = latest_chunk.lock().unwrap().take() {
                                    let line_str = String::from_utf8_lossy(&latest_chunk).to_string();
                                    if line_str.starts_with("data:") {
                                        let data: &str = &line_str[6..].trim();
                                        let data: Value = serde_json::from_str(data).unwrap();
                                        let usage = data.get("usage");
                                        let usage = parse_usage_info(usage);
                                        let price = request_info.price.as_ref(); // Get price from request_info

                                        let usage_str = match (&usage, price) {
                                            (Some(u), Some(p)) => {
                                                // Calculate cost assuming price is USD per 1M tokens scaled by 10000
                                                let cost = (u.prompt_tokens as f64 * p.input_price as f64 + u.completion_tokens as f64 * p.output_price as f64) / 10_000_000_000.0;
                                                let tps = u.completion_tokens as f64 / (now - first_response) as f64 * 1000f64;
                                                format!(" ({}, {}, {}, {tps:.3}t/s, {}{cost:.9})", u.prompt_tokens, u.completion_tokens, u.prompt_tokens + u.completion_tokens, p.currency) // Added currency
                                            }
                                            (Some(u), None) => {
                                                // Price not available
                                                let tps = u.completion_tokens as f64 / (now - first_response) as f64 * 1000f64;
                                                format!(" ({}, {}, {}, {tps:.3}t/s)", u.prompt_tokens, u.completion_tokens, u.prompt_tokens + u.completion_tokens)
                                            }
                                            _ => "".to_string(), // No usage info
                                        };

                                        println!("{model_str}: {first_response} {now}{usage_str}");
                                        Record::insert_one(&Record::new(
                                            api_key_id,
                                            &model_info,
                                            usage.as_ref(),
                                            &TimeInfo {
                                                start_time,
                                                first_token_time: Some(first_response as i32),
                                                response_time: now as i32
                                            },
                                            true
                                        )).unwrap();
                                    }
                                }
                            } else {
                                let mut guard: std::sync::MutexGuard<'_, Option<Bytes>> =
                                    latest_chunk.lock().unwrap();
                                *guard = Some(chunk);
                            }
                        }
                    } else {
                        total_bytes.extend_from_slice(&chunk);
                    }
                    yield Ok::<_, std::io::Error>(chunk);
                }
                Err(e) => {
                    println!("Stream error: {}", e);
                    break;
                }
            }
        }
        if status_code == 200 {
            if !is_sse {
                let now = Utc::now().timestamp_millis() - start_time;
                if total_bytes.len() > 0 {
                    if is_gzip {
                        let mut gz = GzDecoder::new(&total_bytes[..]);
                        let mut decompressed_data = Vec::new();
                        gz.read_to_end(&mut decompressed_data).unwrap();
                        total_bytes.clear();
                        total_bytes.extend_from_slice(&Bytes::from(decompressed_data));
                    }
                    let data: Value = serde_json::from_slice(&total_bytes).unwrap();
                    let usage = data.get("usage");
                    let usage = parse_usage_info(usage);
                    let price = request_info.price.as_ref(); // Get price from request_info

                    let usage_str = match (&usage, price) {
                        (Some(u), Some(p)) => {
                            // Calculate cost assuming price is USD per 1M tokens scaled by 10000
                            let cost = (u.prompt_tokens as f64 * p.input_price as f64 + u.completion_tokens as f64 * p.output_price as f64) / 10_000_000_000.0;
                            let tps = u.completion_tokens as f64 / now as f64 * 1000f64;
                            format!(" ({}, {}, {}, {tps:.3}t/s, {}{cost:.9})", u.prompt_tokens, u.completion_tokens, u.prompt_tokens + u.completion_tokens, p.currency) // Added currency
                        }
                        (Some(u), None) => {
                            // Price not available
                            let tps = u.completion_tokens as f64 / now as f64 * 1000f64;
                            format!(" ({}, {}, {}, {tps:.3}t/s)", u.prompt_tokens, u.completion_tokens, u.prompt_tokens + u.completion_tokens)
                        }
                        _ => "".to_string(), // No usage info
                    };

                    println!("{model_str}: {now}{usage_str} ");
                    Record::insert_one(&Record::new(
                        api_key_id,
                        &model_info,
                        usage.as_ref(),
                        &TimeInfo {
                            start_time: start_time,
                            first_token_time: None,
                            response_time: now as i32
                        },
                        false
                    )).unwrap();
                } else {
                    println!("{model_str}: {now}");
                    Record::insert_one(&Record::new(
                        api_key_id,
                        &model_info,
                        None,
                        &TimeInfo {
                            start_time: start_time,
                            first_token_time: None,
                            response_time: now as i32
                        },
                        true
                    )).unwrap();
                }
            }
        } else {
            if is_gzip {
                let mut gz = GzDecoder::new(&total_bytes[..]);
                let mut decompressed_data = Vec::new();
                gz.read_to_end(&mut decompressed_data).unwrap();
                total_bytes.clear();
                total_bytes.extend_from_slice(&Bytes::from(decompressed_data));
            }
            let data = String::from_utf8_lossy(&total_bytes).to_string();
            println!("request failed ({}) {}", status_code, data);
        }
    };

    response_builder
        .body(Body::from_stream(monitored_stream))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to transform response".to_string(),
            )
        })
}

/// **Deprecated**
async fn proxy_single_handler(
    Path(params): Path<(String, String)>,
    request: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let start_time: i64 = Utc::now().timestamp_millis();

    let method = request.method().as_str().to_string();

    // check and parse api_key_id
    let pre_headers: &axum::http::HeaderMap = &request.headers().clone();
    let api_key_id = check_header_auth(pre_headers)?;

    let (provider_key, path) = params;

    // parse body, and get provider and model info
    let axum_body = request.into_body();
    let body = axum::body::to_bytes(axum_body, usize::MAX).await.unwrap();
    let mut data: Value = serde_json::from_slice(&body).unwrap();
    let model_name = data.get("model").unwrap().to_string();
    let model_name = &model_name[1..&model_name.len() - 1];

    let (provider, provider_keys, _, model) =
        Model::query_provider_model(&provider_key, model_name).unwrap();

    let model_id = match &model {
        Some(model) => Some(model.id),
        None => None,
    };
    // Fetch the latest price for the model_id if it exists
    let price = match model_id {
        Some(id) => Model::get_latest_by_model_id(id).ok(), // Use ok() to convert Result to Option
        None => None,
    };

    if provider.limit_model && model.is_none() {
        return Err((StatusCode::BAD_REQUEST, "model not found".to_string()));
    }

    let real_model_name = match model {
        Some(model) => &model.real_model_name.unwrap_or(model.model_name),
        None => model_name,
    };

    if let Some(obj) = data.as_object_mut() {
        obj.insert("model".to_string(), json!(real_model_name));
    }

    let provider_key = &provider_keys[0];

    // build new request headers
    let headers = build_new_headers(pre_headers, &provider_key.api_key)?;

    process_stream_options(&mut data);

    // build target url
    let target_url = format!("{}/{}", &provider.endpoint, path);
    let url = Url::parse(&target_url).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "failed to parse target url".to_string(),
        )
    })?;

    let method: &str = &method;
    let method = reqwest::Method::try_from(method).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to parse method".to_string(),
        )
    })?;

    let request_info = RequestInfo {
        api_key_id,
        provider_id: provider.id,
        provider_key: provider.provider_key,
        model_id,
        model_name: model_name.to_string(),
        real_model_name: real_model_name.to_string(),
        price, // Pass the fetched price
    };

    proxy_request(
        url.as_str(),
        serde_json::to_string(&data).unwrap(),
        method,
        headers,
        request_info,
        provider.use_proxy,
        start_time,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to transform response".to_string(),
        )
    })
}

fn parse_provider_model(pm: &str) -> (&str, &str) {
    let mut parts = pm.splitn(2, '/');
    let provider = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    (provider, model_id)
}

fn handle_custom_fields(data: &mut Value, custom_fields: &Vec<CustomField>) {
    for custom_field in custom_fields {
        match custom_field.field_type.as_str() {
            "unset" => {
                data.as_object_mut().map(|obj| {
                    obj.remove(&custom_field.field_name);
                });
            }
            "text" => {
                if let Some(text_value) = &custom_field.text_value {
                    data.as_object_mut().map(|obj| {
                        obj.insert(
                            custom_field.field_name.clone(),
                            Value::String(text_value.clone()),
                        );
                    });
                }
            }
            "integer" => {
                if let Some(int_value) = custom_field.integer_value {
                    data.as_object_mut().map(|obj| {
                        obj.insert(
                            custom_field.field_name.clone(),
                            Value::Number(int_value.into()),
                        );
                    });
                }
            }
            "float" => {
                if let Some(float_value) = custom_field.float_value {
                    data.as_object_mut().map(|obj| {
                        obj.insert(
                            custom_field.field_name.clone(),
                            serde_json::Number::from_f64(float_value as f64).map(Value::Number).unwrap_or(Value::Null),
                        );
                    });
                }
            }
            "boolean" => {
                if let Some(bool_value) = custom_field.boolean_value {
                    data.as_object_mut().map(|obj| {
                        obj.insert(
                            custom_field.field_name.clone(),
                            Value::Bool(bool_value),
                        );
                    });
                }
            }
            _ => {}
        }
    }
}

async fn proxy_all_handler(
    Path(path): Path<String>,
    request: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let start_time: i64 = Utc::now().timestamp_millis();
    // get auth header
    let pre_headers: &axum::http::HeaderMap = &request.headers().clone();
    let api_key_id = check_header_auth(pre_headers)?;

    let method = request.method().as_str().to_string();

    // parse body, and get provider and model info
    let axum_body = request.into_body();
    let body = axum::body::to_bytes(axum_body, usize::MAX).await.unwrap();
    let mut data: Value = serde_json::from_slice(&body).unwrap();
    process_stream_options(&mut data);

    let pre_model = data.get("model").unwrap().to_string();
    let pre_model_str = pre_model.trim_matches('"');

    // Apply global model transform
    let mut model_name = pre_model_str.to_string();
    if let Ok(Some(model_transform)) =
        ModelTransform::query_one_by_model_name(pre_model_str.to_string())
    {
        model_name = model_transform.map_model_name;
    }

    let (provider_key, model_name) = parse_provider_model(&model_name);

    let (provider, provider_keys, custom_fields, model) =
        Model::query_provider_model(provider_key, model_name).unwrap();

    handle_custom_fields(&mut data, &custom_fields);

    let model_id = match &model {
        Some(model) => Some(model.id),
        None => None,
    };

    // Fetch the latest price for the model_id if it exists
    let price = match model_id {
        Some(id) => Model::get_latest_by_model_id(id).ok(), // Use ok() to convert Result to Option
        None => None,
    };


    if provider.limit_model && model.is_none() {
        return Err((StatusCode::BAD_REQUEST, "model not found".to_string()));
    }

    let real_model_name = match &model {
        Some(model) => model
            .real_model_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&model.model_name),
        None => model_name,
    };

    // if let Some(omit_config) = request_info.0.omit_config.as_ref() {
    //     remove_data_field(&mut data, Some(&omit_config.data));
    // }

    if let Some(obj) = data.as_object_mut() {
        obj.insert("model".to_string(), json!(real_model_name));
    }

    // build target url
    let target_url = format!("{}/{}", provider.endpoint, path);
    let url = Url::parse(&target_url).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "failed to parse target url".to_string(),
        )
    })?;

    let provider_key = &provider_keys[0];

    // todo
    let headers = build_new_headers(pre_headers, &provider_key.api_key)?;
    // if let Some(omit_config) = request_info.0.omit_config {
    //     if omit_config.header.len() > 0 {
    //         for header_key in &omit_config.header {
    //             headers.remove(header_key);
    //         }
    //     }
    // }

    let method: &str = &method;
    let method = reqwest::Method::try_from(method).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to parse method".to_string(),
        )
    })?;

    let request_info = RequestInfo {
        api_key_id,
        provider_id: provider.id,
        provider_key: provider.provider_key.to_string(),
        model_id: model_id,
        model_name: model_name.to_string(),
        real_model_name: real_model_name.to_string(),
        price, // Pass the fetched price
    };

    proxy_request(
        url.as_str(),
        serde_json::to_string(&data).unwrap(),
        method,
        headers,
        request_info,
        provider.use_proxy,
        start_time,
    )
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to transform response".to_string(),
        )
    })
}

pub fn create_proxy_router() -> Router {
    Router::new().nest(
        "/proxy",
        Router::new()
            .route("/all/{*path}", any(proxy_all_handler))
            .route("/{:provider}/{*path}", any(proxy_single_handler)),
    )
}
