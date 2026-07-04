use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct PerfBucket {
    pub name: &'static str,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct PerfLog {
    enabled: bool,
    buckets: Vec<PerfBucket>,
}

impl PerfLog {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            buckets: Vec::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn time<T, F>(&mut self, name: &'static str, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let start = Instant::now();
        let result = f();
        self.record(name, start.elapsed());
        result
    }

    pub fn time_result<T, E, F>(&mut self, name: &'static str, f: F) -> std::result::Result<T, E>
    where
        F: FnOnce() -> std::result::Result<T, E>,
    {
        let start = Instant::now();
        let result = f();
        self.record(name, start.elapsed());
        result
    }

    pub fn record(&mut self, name: &'static str, duration: Duration) {
        if self.enabled {
            self.buckets.push(PerfBucket { name, duration });
        }
    }

    pub fn into_buckets(self) -> Vec<PerfBucket> {
        self.buckets
    }
}

pub fn emit_buckets_stderr(buckets: &[PerfBucket]) {
    for bucket in buckets {
        eprintln!(
            "[perf] {}={:.3}s",
            bucket.name,
            bucket.duration.as_secs_f64()
        );
    }
}
