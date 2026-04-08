use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use cyder_api::schema::enum_def::LlmApiType;
use cyder_api::service::transform::quality::{BenchmarkScenarioMetrics, BenchmarkSummary};
use cyder_api::service::transform::{StreamTransformer, transform_request_data, transform_result};
use cyder_api::utils::sse::SseEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[global_allocator]
static GLOBAL: TrackingAllocator = TrackingAllocator::new();

#[derive(Default)]
struct TrackingAllocator {
    allocations: AtomicU64,
    deallocations: AtomicU64,
    reallocations: AtomicU64,
    bytes_allocated: AtomicU64,
    bytes_deallocated: AtomicU64,
    current_bytes: AtomicU64,
    peak_bytes: AtomicU64,
}

impl TrackingAllocator {
    const fn new() -> Self {
        Self {
            allocations: AtomicU64::new(0),
            deallocations: AtomicU64::new(0),
            reallocations: AtomicU64::new(0),
            bytes_allocated: AtomicU64::new(0),
            bytes_deallocated: AtomicU64::new(0),
            current_bytes: AtomicU64::new(0),
            peak_bytes: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> AllocSnapshot {
        AllocSnapshot {
            allocations: self.allocations.load(Ordering::Relaxed),
            deallocations: self.deallocations.load(Ordering::Relaxed),
            reallocations: self.reallocations.load(Ordering::Relaxed),
            bytes_allocated: self.bytes_allocated.load(Ordering::Relaxed),
            bytes_deallocated: self.bytes_deallocated.load(Ordering::Relaxed),
            current_bytes: self.current_bytes.load(Ordering::Relaxed),
        }
    }

    fn reset_peak(&self) {
        let current = self.current_bytes.load(Ordering::Relaxed);
        self.peak_bytes.store(current, Ordering::Relaxed);
    }

    fn peak_bytes(&self) -> u64 {
        self.peak_bytes.load(Ordering::Relaxed)
    }

    fn note_peak(&self, current: u64) {
        let mut observed = self.peak_bytes.load(Ordering::Relaxed);
        while current > observed {
            match self.peak_bytes.compare_exchange_weak(
                observed,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => observed = actual,
            }
        }
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let size = layout.size() as u64;
            self.allocations.fetch_add(1, Ordering::Relaxed);
            self.bytes_allocated.fetch_add(size, Ordering::Relaxed);
            let current = self.current_bytes.fetch_add(size, Ordering::Relaxed) + size;
            self.note_peak(current);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        let size = layout.size() as u64;
        self.deallocations.fetch_add(1, Ordering::Relaxed);
        self.bytes_deallocated.fetch_add(size, Ordering::Relaxed);
        self.current_bytes.fetch_sub(size, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old_size = layout.size() as u64;
            let new_size = new_size as u64;
            self.reallocations.fetch_add(1, Ordering::Relaxed);
            if new_size >= old_size {
                let delta = new_size - old_size;
                self.bytes_allocated.fetch_add(delta, Ordering::Relaxed);
                let current = self.current_bytes.fetch_add(delta, Ordering::Relaxed) + delta;
                self.note_peak(current);
            } else {
                let delta = old_size - new_size;
                self.bytes_deallocated.fetch_add(delta, Ordering::Relaxed);
                self.current_bytes.fetch_sub(delta, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ScenarioKind {
    Request,
    Response,
    Stream,
}

impl ScenarioKind {
    fn label(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Stream => "stream",
        }
    }
}

struct Scenario {
    name: &'static str,
    kind: ScenarioKind,
    batch_size: usize,
    input_bytes: usize,
    run: Box<dyn Fn() + Send + Sync>,
}

#[derive(Clone, Copy, Default)]
struct AllocSnapshot {
    allocations: u64,
    deallocations: u64,
    reallocations: u64,
    bytes_allocated: u64,
    bytes_deallocated: u64,
    current_bytes: u64,
}

impl AllocSnapshot {
    fn diff(self, before: Self) -> Self {
        Self {
            allocations: self.allocations.saturating_sub(before.allocations),
            deallocations: self.deallocations.saturating_sub(before.deallocations),
            reallocations: self.reallocations.saturating_sub(before.reallocations),
            bytes_allocated: self.bytes_allocated.saturating_sub(before.bytes_allocated),
            bytes_deallocated: self
                .bytes_deallocated
                .saturating_sub(before.bytes_deallocated),
            current_bytes: self.current_bytes.saturating_sub(before.current_bytes),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct SampleMetrics {
    nanos_per_op: f64,
    allocations_per_op: f64,
    reallocations_per_op: f64,
    bytes_allocated_per_op: f64,
    peak_bytes_per_op: f64,
}

#[derive(Clone, Serialize)]
struct ScenarioResult {
    name: &'static str,
    kind: ScenarioKind,
    throughput_ops_per_sec: f64,
    throughput_mib_per_sec: f64,
    p50_ms: f64,
    p95_ms: f64,
    avg_allocations: f64,
    avg_reallocations: f64,
    avg_allocated_bytes: f64,
    avg_peak_bytes: f64,
}

fn main() {
    let scenarios = build_scenarios();
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|arg| arg == "--quick");
    let json_out = arg_value(&args, "--json-out");
    let warmup_rounds = if quick { 2 } else { 5 };
    let sample_rounds = if quick { 8 } else { 24 };
    let mut summary_results = Vec::with_capacity(scenarios.len());

    println!(
        "Transform benchmark baseline (warmup={}, samples={})",
        warmup_rounds, sample_rounds
    );
    println!(
        "{:<9} {:<44} {:>12} {:>12} {:>10} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "kind",
        "scenario",
        "ops/s",
        "MiB/s",
        "p50 ms",
        "p95 ms",
        "allocs",
        "reallocs",
        "alloc MiB",
        "peak KiB"
    );

    for scenario in scenarios {
        let result = run_scenario(&scenario, warmup_rounds, sample_rounds);
        summary_results.push(BenchmarkScenarioMetrics {
            kind: result.kind.label().to_string(),
            name: result.name.to_string(),
            throughput_ops_per_sec: result.throughput_ops_per_sec,
            throughput_mib_per_sec: result.throughput_mib_per_sec,
            p50_ms: result.p50_ms,
            p95_ms: result.p95_ms,
            avg_allocations: result.avg_allocations,
            avg_reallocations: result.avg_reallocations,
            avg_allocated_bytes: result.avg_allocated_bytes,
            avg_peak_bytes: result.avg_peak_bytes,
        });
        println!(
            "{:<9} {:<44} {:>12.0} {:>12.2} {:>10.3} {:>10.3} {:>10.1} {:>10.1} {:>12.3} {:>12.1}",
            result.kind.label(),
            result.name,
            result.throughput_ops_per_sec,
            result.throughput_mib_per_sec,
            result.p50_ms,
            result.p95_ms,
            result.avg_allocations,
            result.avg_reallocations,
            result.avg_allocated_bytes / (1024.0 * 1024.0),
            result.avg_peak_bytes / 1024.0,
        );
    }

    if let Some(path) = json_out {
        let summary = BenchmarkSummary {
            format_version: 1,
            quick,
            warmup_rounds,
            sample_rounds,
            scenarios: summary_results,
        };
        let payload =
            serde_json::to_vec_pretty(&summary).expect("serialize transform benchmark summary");
        std::fs::write(&path, payload).expect("write transform benchmark summary");
        eprintln!("Wrote transform benchmark summary to {}", path);
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn run_scenario(scenario: &Scenario, warmup_rounds: usize, sample_rounds: usize) -> ScenarioResult {
    for _ in 0..warmup_rounds {
        (scenario.run)();
    }

    let mut samples = Vec::with_capacity(sample_rounds);
    let started = Instant::now();
    for _ in 0..sample_rounds {
        samples.push(measure_sample(scenario));
    }
    let total_elapsed = started.elapsed();

    let mut sorted_latencies: Vec<f64> = samples.iter().map(|sample| sample.nanos_per_op).collect();
    sorted_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let total_ops = scenario.batch_size * sample_rounds;
    let total_input_bytes = scenario.input_bytes * total_ops;
    let avg_allocations = samples
        .iter()
        .map(|sample| sample.allocations_per_op)
        .sum::<f64>()
        / samples.len() as f64;
    let avg_reallocations = samples
        .iter()
        .map(|sample| sample.reallocations_per_op)
        .sum::<f64>()
        / samples.len() as f64;
    let avg_allocated_bytes = samples
        .iter()
        .map(|sample| sample.bytes_allocated_per_op)
        .sum::<f64>()
        / samples.len() as f64;
    let avg_peak_bytes = samples
        .iter()
        .map(|sample| sample.peak_bytes_per_op)
        .sum::<f64>()
        / samples.len() as f64;

    ScenarioResult {
        name: scenario.name,
        kind: scenario.kind,
        throughput_ops_per_sec: total_ops as f64 / total_elapsed.as_secs_f64(),
        throughput_mib_per_sec: total_input_bytes as f64
            / (1024.0 * 1024.0)
            / total_elapsed.as_secs_f64(),
        p50_ms: percentile(&sorted_latencies, 0.50) / 1_000_000.0,
        p95_ms: percentile(&sorted_latencies, 0.95) / 1_000_000.0,
        avg_allocations,
        avg_reallocations,
        avg_allocated_bytes,
        avg_peak_bytes,
    }
}

fn measure_sample(scenario: &Scenario) -> SampleMetrics {
    GLOBAL.reset_peak();
    let before = GLOBAL.snapshot();
    let baseline_current = before.current_bytes;
    let started = Instant::now();
    (scenario.run)();
    let elapsed = started.elapsed();
    let after = GLOBAL.snapshot();
    let diff = after.diff(before);
    let peak_delta = GLOBAL.peak_bytes().saturating_sub(baseline_current);
    let ops = scenario.batch_size as f64;

    SampleMetrics {
        nanos_per_op: elapsed.as_nanos() as f64 / ops,
        allocations_per_op: diff.allocations as f64 / ops,
        reallocations_per_op: diff.reallocations as f64 / ops,
        bytes_allocated_per_op: diff.bytes_allocated as f64 / ops,
        peak_bytes_per_op: peak_delta as f64 / ops,
    }
}

fn percentile(sorted_values: &[f64], quantile: f64) -> f64 {
    let index = ((sorted_values.len() - 1) as f64 * quantile).round() as usize;
    sorted_values[index]
}

fn build_scenarios() -> Vec<Scenario> {
    let request_large_text = build_openai_request_large_text();
    let request_large_text_bytes = serde_json::to_vec(&request_large_text).unwrap().len();

    let request_multi_tool = build_gemini_request_multi_tool();
    let request_multi_tool_bytes = serde_json::to_vec(&request_multi_tool).unwrap().len();

    let response_openai_large = build_openai_response_large_text();
    let response_openai_large_bytes = serde_json::to_vec(&response_openai_large).unwrap().len();

    let response_responses_reasoning = build_responses_response_reasoning_tool();
    let response_responses_reasoning_bytes = serde_json::to_vec(&response_responses_reasoning)
        .unwrap()
        .len();

    let anthropic_stream = load_sse_fixture(include_str!(
        "../src/service/transform/testdata/anthropic_tool_use_json_delta.json"
    ));
    let anthropic_stream_bytes = total_sse_bytes(&anthropic_stream);

    let responses_stream = build_long_responses_stream_fixture(16);
    let responses_stream_bytes = total_sse_bytes(&responses_stream);

    let openai_stream = build_openai_long_text_stream_fixture(256);
    let openai_stream_bytes = total_sse_bytes(&openai_stream);

    vec![
        Scenario {
            name: "openai_to_gemini_large_text",
            kind: ScenarioKind::Request,
            batch_size: 48,
            input_bytes: request_large_text_bytes,
            run: Box::new(move || {
                for _ in 0..48 {
                    let output = transform_request_data(
                        black_box(request_large_text.clone()),
                        LlmApiType::Openai,
                        LlmApiType::Gemini,
                        false,
                    );
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "gemini_to_openai_multi_tool",
            kind: ScenarioKind::Request,
            batch_size: 32,
            input_bytes: request_multi_tool_bytes,
            run: Box::new(move || {
                for _ in 0..32 {
                    let output = transform_request_data(
                        black_box(request_multi_tool.clone()),
                        LlmApiType::Gemini,
                        LlmApiType::Openai,
                        true,
                    );
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "openai_to_gemini_large_text",
            kind: ScenarioKind::Response,
            batch_size: 48,
            input_bytes: response_openai_large_bytes,
            run: Box::new(move || {
                for _ in 0..48 {
                    let output = transform_result(
                        black_box(response_openai_large.clone()),
                        LlmApiType::Openai,
                        LlmApiType::Gemini,
                    );
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "responses_to_openai_reasoning_tool",
            kind: ScenarioKind::Response,
            batch_size: 40,
            input_bytes: response_responses_reasoning_bytes,
            run: Box::new(move || {
                for _ in 0..40 {
                    let output = transform_result(
                        black_box(response_responses_reasoning.clone()),
                        LlmApiType::Responses,
                        LlmApiType::Openai,
                    );
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "anthropic_to_responses_fixture",
            kind: ScenarioKind::Stream,
            batch_size: 12,
            input_bytes: anthropic_stream_bytes,
            run: Box::new(move || {
                for _ in 0..12 {
                    let mut transformer =
                        StreamTransformer::new(LlmApiType::Anthropic, LlmApiType::Responses);
                    let output: Vec<SseEvent> = anthropic_stream
                        .iter()
                        .flat_map(|event| {
                            transformer
                                .transform_event(event.clone())
                                .unwrap_or_default()
                        })
                        .collect();
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "responses_to_openai_long_session",
            kind: ScenarioKind::Stream,
            batch_size: 10,
            input_bytes: responses_stream_bytes,
            run: Box::new(move || {
                for _ in 0..10 {
                    let mut transformer =
                        StreamTransformer::new(LlmApiType::Responses, LlmApiType::Openai);
                    let output: Vec<SseEvent> = responses_stream
                        .iter()
                        .flat_map(|event| {
                            transformer
                                .transform_event(event.clone())
                                .unwrap_or_default()
                        })
                        .collect();
                    black_box(output);
                }
            }),
        },
        Scenario {
            name: "openai_to_gemini_long_text_stream",
            kind: ScenarioKind::Stream,
            batch_size: 8,
            input_bytes: openai_stream_bytes,
            run: Box::new(move || {
                for _ in 0..8 {
                    let mut transformer =
                        StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);
                    let output: Vec<SseEvent> = openai_stream
                        .iter()
                        .flat_map(|event| {
                            transformer
                                .transform_event(event.clone())
                                .unwrap_or_default()
                        })
                        .collect();
                    black_box(output);
                }
            }),
        },
    ]
}

fn build_openai_request_large_text() -> Value {
    let large_text = std::iter::repeat_n(
        "Stage 2 benchmark text block with tool context and provider metadata retention. ",
        160,
    )
    .collect::<String>();

    let messages: Vec<Value> = (0..18)
        .map(|index| {
            let role = if index == 0 {
                "system"
            } else if index % 2 == 0 {
                "assistant"
            } else {
                "user"
            };
            json!({
                "role": role,
                "content": format!("{} message #{index}", large_text),
            })
        })
        .collect();

    json!({
        "model": "gpt-4.1",
        "messages": messages,
        "temperature": 0.4,
        "top_p": 0.92,
        "max_tokens": 2048,
        "stop": ["<END_BLOCK>", "<END_TOOL>"]
    })
}

fn build_gemini_request_multi_tool() -> Value {
    let tools: Vec<Value> = (0..12)
        .map(|index| {
            json!({
                "name": format!("lookup_dataset_{index}"),
                "description": format!("Lookup dataset {index} and return normalized analytics."),
                "parameters": {
                    "type": "OBJECT",
                    "properties": {
                        "dataset": { "type": "STRING" },
                        "window": { "type": "STRING" },
                        "limit": { "type": "INTEGER" }
                    },
                    "required": ["dataset", "window"]
                }
            })
        })
        .collect();

    let contents: Vec<Value> = vec![
        json!({
            "role": "user",
            "parts": [{"text": "Aggregate the latest dashboard data and call tools as needed."}]
        }),
        json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "lookup_dataset_0",
                        "args": {
                            "dataset": "revenue",
                            "window": "30d",
                            "limit": 5
                        }
                    }
                },
                {
                    "functionCall": {
                        "name": "lookup_dataset_3",
                        "args": {
                            "dataset": "latency",
                            "window": "7d",
                            "limit": 10
                        }
                    }
                }
            ]
        }),
        json!({
            "role": "user",
            "parts": [
                {
                    "functionResponse": {
                        "name": "lookup_dataset_0",
                        "response": {
                            "result": "{\"series\":[1,2,3,4,5]}"
                        }
                    }
                },
                {
                    "functionResponse": {
                        "name": "lookup_dataset_3",
                        "response": {
                            "result": "{\"series\":[14,13,18,11,10]}"
                        }
                    }
                }
            ]
        }),
    ];

    json!({
        "contents": contents,
        "tools": [
            {
                "functionDeclarations": tools
            }
        ],
        "generationConfig": {
            "temperature": 0.2,
            "topP": 0.8,
            "maxOutputTokens": 1024
        }
    })
}

fn build_openai_response_large_text() -> Value {
    let content = std::iter::repeat_n(
        "This response body is intentionally large to benchmark unified response conversion. ",
        220,
    )
    .collect::<String>();

    json!({
        "id": "chatcmpl-bench-response",
        "object": "chat.completion",
        "created": 1_744_000_000u64,
        "model": "gpt-4.1",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 512,
            "completion_tokens": 1536,
            "total_tokens": 2048,
            "completion_tokens_details": {
                "reasoning_tokens": 128
            }
        }
    })
}

fn build_responses_response_reasoning_tool() -> Value {
    json!({
        "id": "resp-bench-1",
        "object": "response",
        "created_at": 1_744_000_000u64,
        "status": "completed",
        "model": "gpt-4.1",
        "output": [
            {
                "type": "reasoning",
                "id": "rs_1",
                "summary": [
                    {
                        "type": "summary_text",
                        "text": "Reviewing cached dashboard data before calling tools."
                    }
                ]
            },
            {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_analytics",
                "name": "lookup_analytics",
                "arguments": "{\"dataset\":\"revenue\",\"window\":\"30d\"}"
            },
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [
                    {
                        "type": "output_text",
                        "text": "I gathered the analytics baseline and prepared the tool call."
                    }
                ]
            }
        ],
        "usage": {
            "input_tokens": 256,
            "output_tokens": 96,
            "total_tokens": 352,
            "output_tokens_details": {
                "reasoning_tokens": 48
            }
        }
    })
}

fn build_openai_long_text_stream_fixture(chunk_count: usize) -> Vec<SseEvent> {
    let mut events = Vec::with_capacity(chunk_count + 3);
    events.push(SseEvent {
        data: json!({
            "id": "chatcmpl-bench-stream",
            "object": "chat.completion.chunk",
            "created": 1_744_000_000u64,
            "model": "gpt-4.1",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant"
                }
            }]
        })
        .to_string(),
        ..Default::default()
    });

    for index in 0..chunk_count {
        events.push(SseEvent {
            data: json!({
                "id": "chatcmpl-bench-stream",
                "object": "chat.completion.chunk",
                "created": 1_744_000_000u64,
                "model": "gpt-4.1",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": format!("chunk-{index:03}-benchmark-text ")
                    }
                }]
            })
            .to_string(),
            ..Default::default()
        });
    }

    events.push(SseEvent {
        data: json!({
            "id": "chatcmpl-bench-stream",
            "object": "chat.completion.chunk",
            "created": 1_744_000_000u64,
            "model": "gpt-4.1",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 300,
                "completion_tokens": 900,
                "total_tokens": 1200
            }
        })
        .to_string(),
        ..Default::default()
    });
    events.push(SseEvent {
        data: "[DONE]".to_string(),
        ..Default::default()
    });
    events
}

fn build_long_responses_stream_fixture(repetitions: usize) -> Vec<SseEvent> {
    let template = load_sse_fixture(include_str!(
        "../src/service/transform/testdata/responses_reasoning_function_call.json"
    ));
    let mut events = Vec::with_capacity(template.len() * repetitions);

    for repetition in 0..repetitions {
        for event in &template {
            let mut value: Value = serde_json::from_str(&event.data).unwrap();
            if let Some(id) = value.get_mut("id") {
                *id = Value::String(format!("resp_bench_{repetition}"));
            }
            if let Some(delta) = value.get_mut("delta").and_then(Value::as_object_mut) {
                if let Some(id) = delta.get_mut("id") {
                    *id = Value::String(format!("resp_bench_{repetition}"));
                }
                if delta.get("type").and_then(Value::as_str) == Some("response.tool_call.start")
                    || delta.get("type").and_then(Value::as_str)
                        == Some("response.tool_call.arguments.delta")
                    || delta.get("type").and_then(Value::as_str) == Some("response.tool_call.stop")
                {
                    if let Some(id) = delta.get_mut("id") {
                        *id = Value::String(format!("call_{repetition}"));
                    }
                }
            }

            events.push(SseEvent {
                event: event.event.clone(),
                data: value.to_string(),
                ..Default::default()
            });
        }
    }

    events
}

fn total_sse_bytes(events: &[SseEvent]) -> usize {
    events
        .iter()
        .map(|event| {
            event.data.len()
                + event.event.as_ref().map_or(0, |value| value.len())
                + event.id.as_ref().map_or(0, |value| value.len())
        })
        .sum()
}

fn load_sse_fixture(raw: &str) -> Vec<SseEvent> {
    #[derive(Deserialize)]
    struct FixtureEvent {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        event: Option<String>,
        data: String,
        #[serde(default)]
        retry: Option<u32>,
    }

    serde_json::from_str::<Vec<FixtureEvent>>(raw)
        .unwrap()
        .into_iter()
        .map(|event| SseEvent {
            id: event.id,
            event: event.event,
            data: event.data,
            retry: event.retry,
        })
        .collect()
}
