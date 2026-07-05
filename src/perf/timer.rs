use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct PerfBucket {
    pub name: &'static str,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfDestination {
    Stderr,
    File(PathBuf),
}

pub struct PerfLog {
    enabled: bool,
    buckets: Vec<PerfBucket>,
    started_at: Instant,
    writer: Option<Box<dyn Write>>,
}

impl PerfLog {
    pub fn new(enabled: bool, destination: PerfDestination) -> io::Result<Self> {
        let writer = if enabled {
            Some(open_writer(destination)?)
        } else {
            None
        };
        Ok(Self {
            enabled,
            buckets: Vec::new(),
            started_at: Instant::now(),
            writer,
        })
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
            let bucket = PerfBucket { name, duration };
            self.write_bucket(&bucket);
            self.buckets.push(bucket);
        }
    }

    pub fn event(&mut self, payload: &str) {
        if !self.enabled {
            return;
        }
        let line = format!(
            "[perf] t={:.3} {payload}\n",
            self.started_at.elapsed().as_secs_f64()
        );
        self.write_line(&line);
    }

    pub fn elapsed_event(&mut self, phase: &str, started: Instant, detail: impl std::fmt::Display) {
        if !self.enabled {
            return;
        }
        self.event(&format!(
            "phase={phase} elapsed_ms={} {detail}",
            started.elapsed().as_millis()
        ));
    }

    pub fn into_buckets(self) -> Vec<PerfBucket> {
        self.buckets
    }

    fn write_bucket(&mut self, bucket: &PerfBucket) {
        let line = format!(
            "[perf] {}={:.3}s\n",
            bucket.name,
            bucket.duration.as_secs_f64()
        );
        self.write_line(&line);
    }

    fn write_line(&mut self, line: &str) {
        if let Some(writer) = self.writer.as_mut() {
            writer
                .write_all(line.as_bytes())
                .expect("write perf log event");
            writer.flush().expect("flush perf log event");
        }
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

pub fn append_bucket(path: Option<&Path>, bucket: &PerfBucket) -> io::Result<()> {
    match path {
        Some(path) => {
            let mut file = OpenOptions::new().create(true).append(true).open(path)?;
            write_bucket_line(&mut file, bucket)?;
            file.flush()
        }
        None => {
            let mut stderr = io::stderr();
            write_bucket_line(&mut stderr, bucket)?;
            stderr.flush()
        }
    }
}

fn open_writer(destination: PerfDestination) -> io::Result<Box<dyn Write>> {
    match destination {
        PerfDestination::Stderr => Ok(Box::new(io::stderr())),
        PerfDestination::File(path) => Ok(Box::new(File::create(path)?)),
    }
}

fn write_bucket_line(writer: &mut dyn Write, bucket: &PerfBucket) -> io::Result<()> {
    writeln!(
        writer,
        "[perf] {}={:.3}s",
        bucket.name,
        bucket.duration.as_secs_f64()
    )
}
