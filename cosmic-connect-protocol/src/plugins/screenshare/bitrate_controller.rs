//! Adaptive bitrate controller based on viewer network conditions.
//!
//! Uses an AIMD (Additive Increase / Multiplicative Decrease) algorithm
//! driven by per-viewer throughput measurements from [`super::stream_sender::StreamSender`].
//!
//! The encoder produces a single bitrate for all viewers, so the controller
//! targets the **weakest** viewer's throughput to avoid frame buildup.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracing::debug;

/// How long a viewer report is considered fresh before being ignored.
const REPORT_STALENESS: Duration = Duration::from_secs(10);

/// Congestion is detected when throughput falls below this fraction of the encoder bitrate.
const CONGESTION_THRESHOLD: f32 = 0.8;

/// Multiplicative decrease factor on congestion.
const DECREASE_FACTOR: f32 = 0.5;

/// Additive increase per check cycle (kbps).
const INCREASE_STEP_KBPS: u32 = 100;

/// Cooldown after a decrease before increases are allowed.
const COOLDOWN_DURATION: Duration = Duration::from_secs(4);

/// Per-viewer network condition snapshot.
#[derive(Debug, Clone)]
pub struct ViewerNetworkReport {
    /// Measured throughput from [`super::stream_sender::StreamSender::throughput_kbps`].
    pub throughput_kbps: u32,
    /// Cumulative broadcast lag frame count for this viewer.
    pub lagged_frames: u64,
    /// When this report was last updated.
    pub reported_at: Instant,
}

/// Thread-safe collection of per-viewer network reports.
///
/// Shared between the capture task (reader) and each sender task (writer).
#[derive(Debug, Clone)]
pub struct ViewerNetworkReports {
    inner: Arc<Mutex<HashMap<String, ViewerNetworkReport>>>,
}

impl ViewerNetworkReports {
    /// Create an empty report collection.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Insert or update a viewer's network report.
    pub fn update(&self, viewer_id: &str, throughput_kbps: u32, lagged_frames: u64) {
        let mut map = self.inner.lock().expect("viewer reports lock poisoned");
        let report = map.entry(viewer_id.to_string()).or_insert_with(|| {
            ViewerNetworkReport {
                throughput_kbps,
                lagged_frames,
                reported_at: Instant::now(),
            }
        });
        report.throughput_kbps = throughput_kbps;
        report.lagged_frames = lagged_frames;
        report.reported_at = Instant::now();
    }

    /// Remove a viewer's report (call on disconnect).
    pub fn remove(&self, viewer_id: &str) {
        let mut map = self.inner.lock().expect("viewer reports lock poisoned");
        map.remove(viewer_id);
    }

    /// Minimum throughput across all non-stale viewers, or `None` if no fresh reports.
    pub fn worst_throughput_kbps(&self) -> Option<u32> {
        let map = self.inner.lock().expect("viewer reports lock poisoned");
        let now = Instant::now();
        map.values()
            .filter(|r| now.duration_since(r.reported_at) < REPORT_STALENESS)
            .map(|r| r.throughput_kbps)
            .min()
    }

    /// Whether any non-stale viewer has lagged frames.
    pub fn any_lagging(&self) -> bool {
        let map = self.inner.lock().expect("viewer reports lock poisoned");
        let now = Instant::now();
        map.values()
            .filter(|r| now.duration_since(r.reported_at) < REPORT_STALENESS)
            .any(|r| r.lagged_frames > 0)
    }

    /// Number of viewers with reports (including stale).
    pub fn viewer_count(&self) -> usize {
        let map = self.inner.lock().expect("viewer reports lock poisoned");
        map.len()
    }

    /// Clear all reports.
    pub fn clear(&self) {
        let mut map = self.inner.lock().expect("viewer reports lock poisoned");
        map.clear();
    }
}

impl Default for ViewerNetworkReports {
    fn default() -> Self {
        Self::new()
    }
}

/// AIMD bitrate controller.
///
/// Called periodically (every 2 s) by the capture task. Returns a new bitrate
/// only when a change is warranted, avoiding unnecessary encoder reconfiguration.
pub struct BitrateController {
    target_kbps: u32,
    min_kbps: u32,
    max_kbps: u32,
    last_decrease: Option<Instant>,
}

impl BitrateController {
    /// Create a new controller with the given bitrate bounds.
    pub fn new(target_kbps: u32, min_kbps: u32, max_kbps: u32) -> Self {
        Self {
            target_kbps,
            min_kbps,
            max_kbps,
            last_decrease: None,
        }
    }

    /// Evaluate network reports and return a new bitrate if one is needed.
    ///
    /// Returns `None` when the current bitrate should be kept.
    pub fn update(
        &mut self,
        reports: &ViewerNetworkReports,
        current_kbps: u32,
        receiver_count: usize,
    ) -> Option<u32> {
        // No viewers → drop to minimum
        if receiver_count == 0 {
            return if current_kbps > self.min_kbps {
                debug!(
                    "BitrateController: no viewers, dropping {} -> {} kbps",
                    current_kbps, self.min_kbps
                );
                Some(self.min_kbps)
            } else {
                None
            };
        }

        let congested = self.is_congested(reports, current_kbps);

        if congested {
            // Multiplicative decrease
            let new_kbps = ((current_kbps as f32) * DECREASE_FACTOR) as u32;
            let new_kbps = new_kbps.max(self.min_kbps);
            self.last_decrease = Some(Instant::now());

            if new_kbps != current_kbps {
                debug!(
                    "BitrateController: congestion detected, {} -> {} kbps",
                    current_kbps, new_kbps
                );
                return Some(new_kbps);
            }
            return None;
        }

        // In cooldown → hold steady
        if let Some(last) = self.last_decrease {
            if last.elapsed() < COOLDOWN_DURATION {
                return None;
            }
        }

        // Additive increase (capped at max and target*2)
        if current_kbps < self.max_kbps {
            let new_kbps = current_kbps.saturating_add(INCREASE_STEP_KBPS).min(self.max_kbps);
            if new_kbps != current_kbps {
                debug!(
                    "BitrateController: good conditions, {} -> {} kbps",
                    current_kbps, new_kbps
                );
                return Some(new_kbps);
            }
        }

        None
    }

    /// Check if congestion is indicated by viewer reports.
    fn is_congested(&self, reports: &ViewerNetworkReports, current_kbps: u32) -> bool {
        if reports.any_lagging() {
            return true;
        }
        if let Some(worst) = reports.worst_throughput_kbps() {
            let threshold = (current_kbps as f32 * CONGESTION_THRESHOLD) as u32;
            if worst < threshold {
                return true;
            }
        }
        false
    }

    /// Get the configured target bitrate.
    pub fn target_kbps(&self) -> u32 {
        self.target_kbps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reports() -> ViewerNetworkReports {
        ViewerNetworkReports::new()
    }

    fn make_controller() -> BitrateController {
        // target=2000, min=200, max=4000
        BitrateController::new(2000, 200, 4000)
    }

    #[test]
    fn test_aimd_decrease_on_congestion() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        // Viewer throughput well below 80% of current (2000 * 0.8 = 1600)
        reports.update("v1", 1000, 0);

        let result = ctrl.update(&reports, 2000, 1);
        assert_eq!(result, Some(1000)); // 2000 * 0.5
    }

    #[test]
    fn test_aimd_increase_on_good_network() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        // Viewer throughput above congestion threshold
        reports.update("v1", 2500, 0);

        // First call: no prior decrease, should increase
        let result = ctrl.update(&reports, 2000, 1);
        assert_eq!(result, Some(2100)); // 2000 + 100
    }

    #[test]
    fn test_hysteresis_cooldown() {
        let mut ctrl = make_controller();
        let reports = make_reports();

        // Trigger congestion first
        reports.update("v1", 500, 0);
        let _ = ctrl.update(&reports, 2000, 1); // decreases, sets cooldown

        // Now conditions improve but we're in cooldown
        reports.update("v1", 3000, 0);
        let result = ctrl.update(&reports, 1000, 1);
        assert_eq!(result, None); // blocked by cooldown
    }

    #[test]
    fn test_min_bitrate_floor() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        reports.update("v1", 50, 0);

        // Already at minimum — decrease shouldn't go below
        let result = ctrl.update(&reports, 200, 1);
        assert_eq!(result, None); // 200 * 0.5 = 100, but clamped to 200 → no change
    }

    #[test]
    fn test_max_bitrate_ceiling() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        reports.update("v1", 5000, 0);

        // At max already
        let result = ctrl.update(&reports, 4000, 1);
        assert_eq!(result, None); // can't go above 4000
    }

    #[test]
    fn test_no_viewers_drops_to_min() {
        let mut ctrl = make_controller();
        let reports = make_reports();

        let result = ctrl.update(&reports, 2000, 0);
        assert_eq!(result, Some(200));
    }

    #[test]
    fn test_worst_viewer_governs() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        // Fast viewer and slow viewer
        reports.update("v1", 3000, 0);
        reports.update("v2", 800, 0); // below threshold

        let result = ctrl.update(&reports, 2000, 2);
        assert_eq!(result, Some(1000)); // decrease triggered by v2
    }

    #[test]
    fn test_lag_triggers_decrease() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        // Good throughput but lagging
        reports.update("v1", 3000, 5);

        let result = ctrl.update(&reports, 2000, 1);
        assert_eq!(result, Some(1000)); // lag = congestion
    }

    #[test]
    fn test_stale_report_ignored() {
        let mut ctrl = make_controller();
        let reports = make_reports();
        // Insert a report and manually backdate it
        reports.update("v1", 100, 10);
        {
            let mut map = reports.inner.lock().unwrap();
            map.get_mut("v1").unwrap().reported_at =
                Instant::now() - Duration::from_secs(15);
        }

        // Stale report should be ignored → no congestion → increase
        let result = ctrl.update(&reports, 2000, 1);
        assert_eq!(result, Some(2100)); // additive increase, stale ignored
    }

    #[test]
    fn test_viewer_report_lifecycle() {
        let reports = make_reports();
        assert_eq!(reports.viewer_count(), 0);
        assert_eq!(reports.worst_throughput_kbps(), None);

        reports.update("v1", 1500, 0);
        assert_eq!(reports.viewer_count(), 1);
        assert_eq!(reports.worst_throughput_kbps(), Some(1500));

        reports.update("v2", 800, 0);
        assert_eq!(reports.viewer_count(), 2);
        assert_eq!(reports.worst_throughput_kbps(), Some(800));

        reports.remove("v2");
        assert_eq!(reports.viewer_count(), 1);
        assert_eq!(reports.worst_throughput_kbps(), Some(1500));

        reports.remove("v1");
        assert_eq!(reports.viewer_count(), 0);
        assert_eq!(reports.worst_throughput_kbps(), None);
    }
}
