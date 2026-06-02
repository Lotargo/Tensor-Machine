use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    routing::get,
};
use prometheus::{Encoder, Histogram, HistogramOpts, Opts, Registry, TextEncoder};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct MetricsHandle {
    inner: Arc<Metrics>,
}

pub struct Metrics {
    registry: Registry,
    request_counter: prometheus::Counter,
    request_duration_seconds: Histogram,
    active_requests: prometheus::Gauge,
    search_results_total: prometheus::Counter,
    failed_requests_total: prometheus::Counter,
    ready: AtomicBool,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let request_counter = prometheus::Counter::with_opts(Opts::new(
            "tensor_rag_requests_total",
            "Total number of search requests",
        ))
        .expect("failed to create request counter");

        let request_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "tensor_rag_request_duration_seconds",
                "Request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
        )
        .expect("failed to create latency histogram");

        let active_requests = prometheus::Gauge::with_opts(Opts::new(
            "tensor_rag_active_requests",
            "Number of active requests",
        ))
        .expect("failed to create active requests gauge");

        let search_results_total = prometheus::Counter::with_opts(Opts::new(
            "tensor_rag_search_results_total",
            "Total number of results returned",
        ))
        .expect("failed to create results counter");

        let failed_requests_total = prometheus::Counter::with_opts(Opts::new(
            "tensor_rag_failed_requests_total",
            "Total number of failed requests",
        ))
        .expect("failed to create failure counter");

        registry
            .register(Box::new(request_counter.clone()))
            .expect("failed to register request counter");
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .expect("failed to register latency histogram");
        registry
            .register(Box::new(active_requests.clone()))
            .expect("failed to register active requests gauge");
        registry
            .register(Box::new(search_results_total.clone()))
            .expect("failed to register results counter");
        registry
            .register(Box::new(failed_requests_total.clone()))
            .expect("failed to register failure counter");

        Self {
            registry,
            request_counter,
            request_duration_seconds,
            active_requests,
            search_results_total,
            failed_requests_total,
            ready: AtomicBool::new(true),
        }
    }

    pub fn inc_requests(&self) {
        self.request_counter.inc();
    }

    pub fn observe_latency(&self, seconds: f64) {
        self.request_duration_seconds.observe(seconds);
    }

    pub fn inc_active(&self) {
        self.active_requests.inc();
    }

    pub fn dec_active(&self) {
        self.active_requests.dec();
    }

    pub fn inc_results(&self, count: u64) {
        self.search_results_total.inc_by(count as f64);
    }

    pub fn inc_failures(&self) {
        self.failed_requests_total.inc();
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

async fn metrics_handler(State(state): State<MetricsHandle>) -> impl axum::response::IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = state.inner.registry.gather();
    let mut buffer = Vec::new();
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(()) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            buffer,
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("Metrics encoding error: {e}").into_bytes(),
        ),
    }
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

async fn ready_handler(State(state): State<MetricsHandle>) -> StatusCode {
    if state.inner.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

pub async fn serve(
    metrics: Arc<Metrics>,
    port: u16,
    cancel: CancellationToken,
) -> Result<(), std::io::Error> {
    let state = MetricsHandle { inner: metrics };

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            cancel.cancelled().await;
        })
        .await
}
