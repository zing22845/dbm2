//! Performance monitor feature state.
//!
//! `perf_monitor` is a **passive** feature: it is driven by the run loop on
//! every frame (not by user messages), so it has no `Msg`/`Intent`/`Effect`
//! of its own. The run loop samples each rendered frame and calls
//! [`PerfState::record_frame`] / [`PerfState::record_redundancy`] to feed the
//! smoothed FPS and redundant-redraw ratio.

use std::time::{Duration, Instant};

/// Smoothed performance metrics for the last render window.
#[derive(Debug, Clone)]
pub struct PerfState {
    /// Exponentially-averaged frames per second.
    pub fps: f64,
    /// `Instant` of the previous frame, for FPS smoothing. Updated by both
    /// real redraws and forced counter-refresh repaints (see `touch_frame`).
    last_frame_instant: Option<Instant>,
    /// `Instant` of the last *real* frame (a redraw that feeds the estimates).
    /// Forced counter-refresh repaints do NOT update this, so the FPS/waste
    /// decay and the idle stop never get fed by the decay repaints themselves.
    last_real_frame_instant: Option<Instant>,
    /// Redundant-redraw ratio in `[0,1]` (1 = fully redundant).
    pub redundancy_rate: f64,
    /// Time-decayed accumulators backing the redundancy ratio.
    redundant_weight: f64,
    total_weight: f64,
    /// `Instant` of the previous redundancy sample.
    last_redundancy_instant: Option<Instant>,
}

impl Default for PerfState {
    fn default() -> Self {
        PerfState {
            fps: 0.0,
            last_frame_instant: None,
            last_real_frame_instant: None,
            redundancy_rate: 0.0,
            redundant_weight: 0.0,
            total_weight: 0.0,
            last_redundancy_instant: None,
        }
    }
}

impl PerfState {
    /// Sample a frame boundary and update the smoothed FPS.
    ///
    /// Exponential moving average: smoothed but still tracks sustained rate
    /// changes.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame_instant {
            let dt = now.saturating_duration_since(last).as_secs_f64();
            if dt > 0.0 {
                let inst = 1.0 / dt;
                self.fps = if self.fps <= 0.0 {
                    inst
                } else {
                    self.fps * 0.8 + inst * 0.2
                };
            }
        }
        self.last_frame_instant = Some(now);
        self.last_real_frame_instant = Some(now);
    }

    /// Bump the frame timestamp without recording a frame. Used for a forced
    /// counter-refresh repaint (idle decay) that must not feed the FPS / waste
    /// estimates, so later real redraws still count from this instant.
    ///
    /// This updates `last_frame_instant` only — the "last real frame" instant
    /// used for the idle-stop decision is untouched, so a decay repaint can
    /// never keep the counter loop alive by itself.
    pub fn touch_frame(&mut self) {
        self.last_frame_instant = Some(Instant::now());
    }

    /// How long since the last recorded frame, if any.
    pub fn last_frame_elapsed(&self) -> Option<Duration> {
        self.last_frame_instant.map(|l| l.elapsed())
    }

    /// How long since the last *real* frame (one that fed the estimates), if
    /// any. Forced counter-refresh repaints do not advance this.
    pub fn last_real_frame_elapsed(&self) -> Option<Duration> {
        self.last_real_frame_instant.map(|l| l.elapsed())
    }

    /// Record whether the just-rendered frame changed anything on screen and
    /// update the redundant-redraw ratio. `changed_cells` is the number of
    /// cells that differed from the previous frame (`0` == fully redundant).
    ///
    /// The ratio is backed by two accumulators (`redundant_weight`,
    /// `total_weight`) that decay exponentially with *real elapsed time*
    /// (`REDUNDANCY_TAU`), not per frame. This keeps the value stable across
    /// short idle gaps between redraw bursts and lets it fade gracefully after
    /// a long idle instead of snapping to 0 and back.
    pub fn record_redundancy(&mut self, changed_cells: usize) {
        const REDUNDANCY_TAU: f64 = 1.5; // seconds; window length of the ratio
        let now = Instant::now();
        let dt = now
            .saturating_duration_since(self.last_redundancy_instant.unwrap_or(now))
            .as_secs_f64();
        let factor = (-dt / REDUNDANCY_TAU).exp();
        self.redundant_weight *= factor;
        self.total_weight *= factor;
        self.total_weight += 1.0;
        if changed_cells == 0 {
            self.redundant_weight += 1.0;
        }
        self.redundancy_rate = if self.total_weight > 0.0 {
            self.redundant_weight / self.total_weight
        } else {
            0.0
        };
        self.last_redundancy_instant = Some(now);
    }
}
