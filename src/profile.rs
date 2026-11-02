//! Opt-in stage events for the isolated profiling script; silent by default.
use std::time::Instant;

pub struct Stage {
    name: &'static str,
    start: Option<Instant>,
}

impl Stage {
    pub fn new(name: &'static str) -> Self {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let enabled = *ENABLED.get_or_init(|| std::env::var_os("PYIVT_PROFILE").is_some());
        if enabled {
            eprintln!("{{\"stage\":\"{name}\",\"event\":\"start\"}}");
        }
        Self {
            name,
            start: enabled.then(Instant::now),
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            eprintln!(
                "{{\"stage\":\"{}\",\"event\":\"end\",\"seconds\":{}}}",
                self.name,
                start.elapsed().as_secs_f64()
            );
        }
    }
}
