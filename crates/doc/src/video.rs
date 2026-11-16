//! Window › Timeline. A document-level playhead over a fixed number of frames at a frame rate.
//! Video layers (Layer › Video Layers) show their content for the current frame; this type is the
//! shared clock. Frame-animation and video-timeline modes both reduce to "which frame is current".

use serde::{Deserialize, Serialize};

use photocraft_raster::Surface;

/// Where a video layer's footage came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VideoSource {
    /// Created blank (Layer › Video Layers › New Blank Video Layer).
    Blank,
    /// Loaded from a file or image sequence.
    File { path: String },
}

/// A video layer's frame stack (Layer › Video Layers). The layer's displayed content surface is a
/// cache of `frames[timeline.current]`, which the engine keeps in sync as the playhead moves.
///
/// Frames are full raster surfaces held in memory and saved in native documents. The
/// optional original footage is kept separately from painted frames for comparison/restore.
#[derive(Clone, Debug)]
pub struct VideoData {
    pub frames: Vec<Surface>,
    /// Immutable original footage of a file-backed sequence. COW surfaces share unchanged
    /// pixel tiles with edited frames; blank video layers have no original source.
    pub original_frames: Option<Vec<Surface>>,
    pub source: VideoSource,
    pub fps: f32,
    /// Show the altered (painted) frames rather than the original footage.
    pub show_altered: bool,
}

impl VideoData {
    pub fn new(frames: Vec<Surface>, fps: f32) -> Self {
        VideoData { frames, original_frames: None, source: VideoSource::Blank, fps, show_altered: true }
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

// Surfaces aren't `PartialEq` (and pixel comparison would be pointless here), so compare the stack
// by shape, not contents — enough for change detection.
impl PartialEq for VideoData {
    fn eq(&self, o: &Self) -> bool {
        self.frames.len() == o.frames.len()
            && self.original_frames.as_ref().map(Vec::len) == o.original_frames.as_ref().map(Vec::len)
            && self.source == o.source
            && self.fps == o.fps
            && self.show_altered == o.show_altered
    }
}

/// The document timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    /// Frames per second.
    pub fps: f32,
    /// Total number of frames (>= 1).
    pub duration: usize,
    /// The playhead (0-based frame index).
    pub current: usize,
    /// Work-area start frame (inclusive).
    #[serde(default)]
    pub work_start: usize,
    /// Work-area end frame (exclusive).
    pub work_end: usize,
    /// The number shown for frame 0: VFX plates start at 1001 and artists talk in plate frames.
    /// The ruler, the frame field and Render Video's file numbers count from it. Files saved
    /// before it existed open with 0.
    #[serde(default)]
    pub start_number: i64,
    /// Playback loops inside the work area; off, it stops at the work area's last frame. On in
    /// files saved before it existed (playback always looped).
    #[serde(default = "loops_by_default")]
    pub loop_playback: bool,
}

/// Largest `start_number` either way: far beyond any frame number, small enough that
/// `start_number + frame` never overflows.
pub const MAX_START_NUMBER: i64 = 1_000_000_000;

fn loops_by_default() -> bool {
    true
}

impl Timeline {
    pub fn new(duration: usize, fps: f32) -> Self {
        let d = duration.max(1);
        Timeline { fps: valid_fps(fps), duration: d, current: 0, work_start: 0, work_end: d, start_number: 0, loop_playback: true }
    }

    /// Keep every index inside `0..duration` and the work area non-empty.
    pub fn clamp(&mut self) {
        self.duration = self.duration.max(1);
        self.fps = valid_fps(self.fps);
        self.current = self.current.min(self.duration - 1);
        self.work_start = self.work_start.min(self.duration - 1);
        self.work_end = self.work_end.clamp(self.work_start + 1, self.duration);
        self.start_number = self.start_number.clamp(-MAX_START_NUMBER, MAX_START_NUMBER);
    }

    /// The frame number shown for timeline frame `frame` (`start_number + frame`).
    pub fn number(&self, frame: usize) -> i64 {
        self.start_number.saturating_add(i64::try_from(frame).unwrap_or(i64::MAX))
    }

    /// The timeline frame showing frame number `number`, kept inside the timeline.
    pub fn frame_of(&self, number: i64) -> usize {
        let index = number.saturating_sub(self.start_number).max(0);
        usize::try_from(index).unwrap_or(usize::MAX).min(self.duration.saturating_sub(1))
    }

    /// The work area as timeline frames, `work_start..work_end`, kept inside the timeline and
    /// never empty, whatever was stored.
    pub fn work_frames(&self) -> std::ops::Range<usize> {
        let d = self.duration.max(1);
        let start = self.work_start.min(d - 1);
        start..self.work_end.clamp(start + 1, d)
    }

    /// The work area covers the whole timeline again.
    pub fn reset_work_area(&mut self) {
        self.work_start = 0;
        self.work_end = self.duration.max(1);
    }

    /// Move the playhead `delta` frames, stopping at the first and last frame. Saturating, so it's
    /// safe on any stored timeline, even one loaded with `duration` 0 or `usize::MAX` (#1009).
    pub fn step(&mut self, delta: i64) {
        let n = usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX);
        let moved = if delta < 0 { self.current.saturating_sub(n) } else { self.current.saturating_add(n) };
        self.current = moved.min(self.duration.saturating_sub(1));
    }

    /// Playhead time in seconds.
    pub fn time(&self) -> f32 {
        self.current as f32 / self.fps.max(1e-3)
    }
}

/// A usable frame rate: positive and finite, else the 30 fps default.
fn valid_fps(fps: f32) -> f32 {
    if fps.is_finite() && fps > 0.0 { fps } else { 30.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_clamp() {
        let t = Timeline::new(0, 0.0);
        assert_eq!(t.duration, 1);
        assert_eq!(t.fps, 30.0);
        let mut t = Timeline::new(24, 24.0);
        t.current = 100;
        t.work_end = 999;
        t.clamp();
        assert_eq!(t.current, 23);
        assert_eq!(t.work_end, 24);
        assert!((t.time() - 23.0 / 24.0).abs() < 1e-6);
    }

    /// #1009: stepping never panics or leaves the timeline, whatever was stored.
    #[test]
    fn step_saturates_at_both_ends() {
        let mut t = Timeline::new(3, 24.0);
        t.step(-1);
        assert_eq!(t.current, 0);
        t.step(1);
        t.step(i64::MAX);
        assert_eq!(t.current, 2);
        t.step(i64::MIN);
        assert_eq!(t.current, 0);
        for duration in [0, usize::MAX] {
            let mut t = Timeline { fps: f32::INFINITY, duration, current: usize::MAX, work_start: 0, work_end: 0, start_number: i64::MIN, loop_playback: true };
            t.step(1);
            assert_eq!(t.current, duration.saturating_sub(1));
            t.clamp();
            assert_eq!(t.fps, 30.0);
            assert!(t.current < t.duration);
        }
    }

    #[test]
    fn round_trips() {
        let mut t = Timeline::new(48, 25.0);
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Timeline>(&j).unwrap(), t);
        t.start_number = 1001;
        t.loop_playback = false;
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Timeline>(&j).unwrap(), t);
    }

    /// A timeline saved before plate numbers and the loop switch counts from 0 and loops.
    #[test]
    fn older_timelines_count_from_zero_and_loop() {
        let t: Timeline = serde_json::from_str(r#"{"fps":24.0,"duration":28,"current":3,"work_start":0,"work_end":28}"#).unwrap();
        assert_eq!((t.start_number, t.loop_playback), (0, true));
    }

    #[test]
    fn frame_numbers_count_from_the_start_number() {
        let mut t = Timeline::new(28, 24.0);
        t.start_number = 1001;
        assert_eq!((t.number(0), t.number(12), t.number(27)), (1001, 1013, 1028));
        assert_eq!((t.frame_of(1020), t.frame_of(1001)), (19, 0));
        assert_eq!((t.frame_of(5), t.frame_of(9999)), (0, 27), "numbers outside the timeline land on its ends");
        t.work_start = 8;
        t.work_end = 25;
        assert_eq!(t.work_frames(), 8..25);
        t.reset_work_area();
        assert_eq!(t.work_frames(), 0..28);
        t.start_number = i64::MAX;
        t.clamp();
        assert_eq!(t.start_number, MAX_START_NUMBER);
        t.start_number = 0;
        assert_eq!(t.number(usize::MAX), i64::MAX, "saturates");
    }
}
