use std::time::Duration;
#[derive(Clone, Debug)]
pub struct RetryTicket {
    pub delay: Duration,
    token: u64,
}
#[derive(Debug, Default)]
pub struct RetryPolicy {
    enabled: bool,
    attempts: u32,
    token: u64,
    ready_since: Option<tokio::time::Instant>,
}
impl RetryPolicy {
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
    pub fn enable(&mut self) {
        self.token = self.token.wrapping_add(1);
        self.enabled = true;
        self.attempts = 0;
        self.ready_since = None;
    }
    pub(crate) fn enable_retry(&mut self) {
        self.token = self.token.wrapping_add(1);
        self.enabled = true;
        if self
            .ready_since
            .is_some_and(|started| started.elapsed() >= Duration::from_secs(30))
        {
            self.attempts = 0;
        }
        self.ready_since = None;
    }
    pub fn disable(&mut self) {
        self.token = self.token.wrapping_add(1);
        self.enabled = false;
        self.ready_since = None;
    }
    pub fn schedule(&mut self, retryable: bool, random: f64) -> Option<RetryTicket> {
        if !self.enabled || !retryable || self.attempts >= 6 || !random.is_finite() {
            return None;
        }
        let window = (1000u32 * 2u32.pow(self.attempts)).min(30000);
        let millis =
            (f64::from(window) / 2.0 + random.clamp(0.0, 1.0) * f64::from(window) / 2.0).floor();
        self.attempts += 1;
        self.token = self.token.wrapping_add(1);
        Some(RetryTicket {
            delay: Duration::from_secs_f64(millis / 1000.0),
            token: self.token,
        })
    }
    pub const fn is_current(&self, ticket: &RetryTicket) -> bool {
        self.enabled && self.token == ticket.token
    }
    pub fn mark_ready(&mut self) {
        self.ready_since = Some(tokio::time::Instant::now());
    }
    pub fn stable(&mut self, ready_for: Duration) {
        if ready_for >= Duration::from_secs(30) {
            self.attempts = 0;
        }
    }
}
