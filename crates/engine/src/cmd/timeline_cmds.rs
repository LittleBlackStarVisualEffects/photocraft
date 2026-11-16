//! Window › Timeline: create and drive the document timeline (playhead over N frames at a frame
//! rate). Navigation doesn't add history steps. Video layers (Layer › Video Layers, added later)
//! read `current` to show their frame. Headless and scriptable so agents can render frame ranges.
//!
//! Frames are indices from 0 (`frame`, `current`, `workStart`, `workEnd`); the numbers an artist
//! sees count from the timeline's `startNumber` (1001 for a plate), so `timeline.setFrame
//! {"number": 1013}` is frame 12 of a timeline starting at 1001.

use std::sync::Arc;

use photocraft_doc::{Document, LayerContent, MAX_START_NUMBER, Timeline, VideoData};
use photocraft_raster::Surface;
use serde_json::{Value, json};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// Longest timeline, in frames: every frame becomes a surface in a new video layer and a file in
/// Render Video, so `duration` is capped well before it can exhaust memory or overflow (#1009).
pub(crate) const MAX_DURATION: usize = 1_000_000;
/// Highest frame rate; larger values only come from mistakes and become `inf` as `f32` (#1009).
const MAX_FPS: f32 = 1000.0;

fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document".into())
}

fn has_timeline(s: &Session) -> std::result::Result<(), String> {
    match s.active() {
        Some(d) if d.doc.timeline.is_some() => Ok(()),
        Some(_) => Err("no timeline (create one first)".into()),
        None => Err("no document".into()),
    }
}

/// Mutate the document timeline without a history step (navigation / metadata).
fn with_timeline(s: &mut Session, f: impl FnOnce(&mut Option<Timeline>)) -> Result<Value> {
    let st = s.active_mut().ok_or(EngineError::NoDocument)?;
    let generation = photocraft_raster::spill::read_error_generation();
    let before = Arc::clone(&st.doc);
    let mut doc = (*st.doc).clone();
    crate::video_cmds::store(&mut doc);
    f(&mut doc.timeline);
    if let Some(t) = &mut doc.timeline {
        t.clamp();
    }
    crate::video_cmds::sync(&mut doc);
    // Storing and syncing video frames bypasses edit_with: apply the same scratch guard.
    photocraft_raster::spill::check_since(generation).and_then(|()| doc.scratch_health()).map_err(EngineError::Other)?;
    st.doc = Arc::new(doc);
    // Moving the playhead swaps the pixels of every video layer. Report what changed: without
    // this the canvas reused the previous command's damage (e.g. the empty area of a layer
    // selection) and redrew nothing while Play stepped the frame counter.
    st.last_damage = crate::layer_multi_cmds::step_damage(&before, &st.doc);
    st.revision += 1;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    // Navigation replies leave out `tracks`: playback steps every frame, and `timeline.info` has them.
    Ok(json!({ "timeline": timeline_json(&st.doc, false) }))
}

fn bad(cmd: &str, msg: String) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg }
}

/// The optional `duration` param, rejected unless it's a whole number in `1..=MAX_DURATION`.
/// Before #1009 any `u64` was kept and stepping panicked on it.
fn duration_param(cmd: &str, p: &Value) -> Result<Option<usize>> {
    let Some(v) = p.get("duration").filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_u64()
        .and_then(|d| usize::try_from(d).ok())
        .filter(|d| (1..=MAX_DURATION).contains(d))
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("`duration` must be a whole number of frames from 1 to {MAX_DURATION}, got {v}")))
}

/// The optional `fps` param, rejected unless it's a finite number in `(0, MAX_FPS]` (#1009).
pub(crate) fn fps_param(cmd: &str, p: &Value) -> Result<Option<f32>> {
    let Some(v) = p.get("fps").filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_f64()
        .filter(|f| f.is_finite() && *f > 0.0 && *f <= f64::from(MAX_FPS))
        .map(|f| Some(f as f32))
        .ok_or_else(|| bad(cmd, format!("`fps` must be a number above 0 and at most {MAX_FPS}, got {v}")))
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let duration = duration_param("timeline.create", p)?.unwrap_or(30);
    let fps = fps_param("timeline.create", p)?.unwrap_or(30.0);
    with_timeline(s, |t| *t = Some(Timeline::new(duration, fps)))
}

fn delete(s: &mut Session) -> Result<Value> {
    with_timeline(s, |t| *t = None)
}

/// An optional whole-number frame index param (`frame`, `workStart`, `workEnd`).
fn frame_param(cmd: &str, p: &Value, key: &str) -> Result<Option<usize>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_u64()
        .map(|f| Some(usize::try_from(f).unwrap_or(usize::MAX)))
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a timeline frame, a whole number from 0, got {v}")))
}

/// The optional `startNumber` param: the frame number of frame 0.
fn start_number_param(cmd: &str, p: &Value) -> Result<Option<i64>> {
    let Some(v) = p.get("startNumber").filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_i64()
        .filter(|n| (-MAX_START_NUMBER..=MAX_START_NUMBER).contains(n))
        .map(Some)
        .ok_or_else(|| bad(cmd, format!("`startNumber` must be a whole number from -{MAX_START_NUMBER} to {MAX_START_NUMBER}, got {v}")))
}

/// The optional `number` param: a frame number, counted from the timeline's `startNumber`.
fn number_param(cmd: &str, p: &Value) -> Result<Option<i64>> {
    let Some(v) = p.get("number").filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_i64().map(Some).ok_or_else(|| bad(cmd, format!("`number` must be a whole frame number, got {v}")))
}

fn bool_param(cmd: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    v.as_bool().map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be true or false, got {v}")))
}

/// A timeline frame given as `frame` (an index from 0) or `number` (a frame number).
enum At {
    Frame(usize),
    Number(i64),
}

impl At {
    /// The `frame` or `number` param, `None` when neither is given.
    fn param(cmd: &str, p: &Value) -> Result<Option<At>> {
        match (frame_param(cmd, p, "frame")?, number_param(cmd, p)?) {
            (Some(_), Some(_)) => Err(bad(cmd, "give `frame` (a timeline frame from 0) or `number` (a frame number), not both".into())),
            (Some(f), None) => Ok(Some(At::Frame(f))),
            (None, Some(n)) => Ok(Some(At::Number(n))),
            (None, None) => Ok(None),
        }
    }

    /// The timeline frame, kept inside the timeline.
    fn frame(&self, t: &Timeline) -> usize {
        match *self {
            At::Frame(f) => f.min(t.duration.saturating_sub(1)),
            At::Number(n) => t.frame_of(n),
        }
    }
}

fn set_frame(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.setFrame";
    let at = At::param(cmd, p)?.ok_or_else(|| bad(cmd, "need `frame` (a timeline frame from 0) or `number` (a frame number, counted from `startNumber`)".into()))?;
    with_timeline(s, |t| {
        if let Some(t) = t {
            t.current = at.frame(t);
        }
    })
}

fn step(s: &mut Session, delta: i64) -> Result<Value> {
    with_timeline(s, |t| {
        if let Some(t) = t {
            t.step(delta);
        }
    })
}

/// First Frame / Last Frame.
fn go_to_end(s: &mut Session, last: bool) -> Result<Value> {
    with_timeline(s, |t| {
        if let Some(t) = t {
            t.current = if last { t.duration.saturating_sub(1) } else { 0 };
        }
    })
}

/// Set Start / Set End of Work Area at `frame` / `number`, else at the playhead. The other end
/// moves too when the work area would be empty.
fn set_work_area_end(s: &mut Session, p: &Value, end: bool) -> Result<Value> {
    let cmd = if end { "timeline.setWorkAreaEnd" } else { "timeline.setWorkAreaStart" };
    let at = At::param(cmd, p)?;
    with_timeline(s, |t| {
        if let Some(t) = t {
            let f = at.as_ref().map_or(t.current.min(t.duration.saturating_sub(1)), |a| a.frame(t));
            if end {
                t.work_end = f + 1;
                t.work_start = t.work_start.min(f);
            } else {
                t.work_start = f;
                if t.work_end <= f {
                    t.work_end = f + 1;
                }
            }
        }
    })
}

fn reset_work_area(s: &mut Session) -> Result<Value> {
    with_timeline(s, |t| {
        if let Some(t) = t {
            t.reset_work_area();
        }
    })
}

fn toggle_loop(s: &mut Session) -> Result<Value> {
    with_timeline(s, |t| {
        if let Some(t) = t {
            t.loop_playback = !t.loop_playback;
        }
    })
}

fn set_props(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.setProps";
    let fps = fps_param(cmd, p)?;
    let duration = duration_param(cmd, p)?;
    let work_start = frame_param(cmd, p, "workStart")?;
    let work_end = frame_param(cmd, p, "workEnd")?;
    let start_number = start_number_param(cmd, p)?;
    let looping = bool_param(cmd, p, "loop")?;
    with_timeline(s, |t| {
        if let Some(t) = t {
            if let Some(fps) = fps {
                t.fps = fps;
            }
            if let Some(d) = duration {
                // A work area that reached the old end follows the new one, as in
                // `video_cmds::grow_duration` (#4170); a shrink is left to `Timeline::clamp`.
                if t.work_end >= t.duration {
                    t.work_end = d;
                }
                t.duration = d;
            }
            if let Some(ws) = work_start {
                t.work_start = ws;
            }
            if let Some(we) = work_end {
                t.work_end = we;
            }
            if let Some(n) = start_number {
                t.start_number = n;
            }
            if let Some(l) = looping {
                t.loop_playback = l;
            }
        }
    })
}

fn info(s: &mut Session) -> Result<Value> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    Ok(json!({ "timeline": timeline_json(&st.doc, true) }))
}

/// The document timeline as `timeline.info` reports it, `null` without one: `{fps, duration,
/// current, workStart, workEnd (exclusive), time, startNumber, loop}`, and with `tracks` one
/// entry per video layer, top layer first: `{layerId, name, visible, frames, start, altered}`,
/// `altered` being the timeline frames Restore Frame would change.
pub fn timeline_json(doc: &Document, tracks: bool) -> Value {
    let Some(t) = &doc.timeline else { return Value::Null };
    let mut v = json!({
        "fps": t.fps,
        "duration": t.duration,
        "current": t.current,
        "workStart": t.work_start,
        "workEnd": t.work_end,
        "time": t.time(),
        "startNumber": t.start_number,
        "loop": t.loop_playback,
    });
    if tracks {
        v["tracks"] = Value::Array(video_tracks(doc, t.current));
    }
    v
}

fn video_tracks(doc: &Document, current: usize) -> Vec<Value> {
    doc.walk()
        .into_iter()
        .rev()
        .filter_map(|(_, _, l)| {
            let v = l.video.as_ref()?;
            let live = match &l.content {
                LayerContent::Raster(s) => Some(s),
                _ => None,
            };
            Some(json!({
                "layerId": l.id.0,
                "name": l.name,
                "visible": l.visible,
                "frames": v.len(),
                // Video layers start on the timeline's first frame.
                "start": 0,
                "altered": altered_frames(v, live, current),
            }))
        })
        .collect()
}

/// The frames of a video layer Restore Frame would change: painted over their footage, or on a
/// blank video layer painted at all. The frame at the playhead is read from `live`, the layer's
/// content, which holds paint not yet stored back in the frame (`video_cmds::store`).
pub fn altered_frames(v: &VideoData, live: Option<&Surface>, current: usize) -> Vec<usize> {
    let at = current.min(v.frames.len().saturating_sub(1));
    let live = live.filter(|_| v.show_altered || v.original_frames.is_none());
    v.frames
        .iter()
        .enumerate()
        .filter(|&(i, frame)| {
            let shown = if i == at { live.unwrap_or(frame) } else { frame };
            match v.original_frames.as_ref().and_then(|o| o.get(i)) {
                Some(original) => !same_pixels(shown, original),
                // Restore Frame leaves a blank frame.
                None => shown.tile_count() > 0,
            }
        })
        .map(|(i, _)| i)
        .collect()
}

/// Whether two surfaces hold the same pixels. Footage frames share their tiles with the original
/// footage until painted (and a reopened native file shares them again), so this is mostly
/// pointer comparisons; tiles that differ are compared by their bytes.
fn same_pixels(a: &Surface, b: &Surface) -> bool {
    a.format() == b.format()
        && a.tile_count() == b.tile_count()
        && a.default_pixel() == b.default_pixel()
        && a.tiles().zip(b.tiles()).all(|((ca, ta), (cb, tb))| ca == cb && (Arc::ptr_eq(ta, tb) || **ta == **tb))
}

macro_rules! spec {
    ($id:expr, $label:expr, $params:expr, $en:expr, $run:expr) => {
        CommandSpec { id: $id, label: $label, menu: &[], shortcut: None, params: $params, enabled: $en, journal: false, run: $run }
    };
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!("timeline.create", "Create Video Timeline", r#"{duration?:30, fps?:30} → {timeline}"#, has_doc, |s, p| create(s, p)),
        spec!("timeline.delete", "Delete Timeline", "{} → {timeline:null}", has_timeline, |s, _| delete(s)),
        spec!("timeline.setFrame", "Go to Frame", r#"{"frame":index?, "number":frameNumber?} → {timeline}"#, has_timeline, |s, p| set_frame(s, p)),
        spec!("timeline.nextFrame", "Next Frame", "{} → {timeline}", has_timeline, |s, _| step(s, 1)),
        spec!("timeline.previousFrame", "Previous Frame", "{} → {timeline}", has_timeline, |s, _| step(s, -1)),
        spec!("timeline.firstFrame", "First Frame", "{} → {timeline}", has_timeline, |s, _| go_to_end(s, false)),
        spec!("timeline.lastFrame", "Last Frame", "{} → {timeline}", has_timeline, |s, _| go_to_end(s, true)),
        spec!("timeline.back10Frames", "Back 10 Frames", "{} → {timeline}", has_timeline, |s, _| step(s, -10)),
        spec!("timeline.forward10Frames", "Forward 10 Frames", "{} → {timeline}", has_timeline, |s, _| step(s, 10)),
        spec!(
            "timeline.setWorkAreaStart",
            "Set Start of Work Area",
            r#"{"frame":index?, "number":frameNumber?} (default: the playhead) → {timeline}"#,
            has_timeline,
            |s, p| set_work_area_end(s, p, false)
        ),
        spec!(
            "timeline.setWorkAreaEnd",
            "Set End of Work Area",
            r#"{"frame":index?, "number":frameNumber?} (its last frame; default: the playhead) → {timeline}"#,
            has_timeline,
            |s, p| set_work_area_end(s, p, true)
        ),
        spec!("timeline.resetWorkArea", "Reset Work Area", "{} → {timeline}", has_timeline, |s, _| reset_work_area(s)),
        spec!("timeline.toggleLoop", "Loop Playback", "{} → {timeline}", has_timeline, |s, _| toggle_loop(s)),
        spec!(
            "timeline.setProps",
            "Timeline Settings",
            r#"{"fps":number?, "duration":frames?, "workStart":index?, "workEnd":index? (exclusive), "startNumber":int?, "loop":bool?} → {timeline}"#,
            has_timeline,
            |s, p| set_props(s, p)
        ),
        spec!("timeline.info", "Timeline Info", "{} → {timeline: {fps, duration, current, workStart, workEnd, time, startNumber, loop, tracks}}", has_doc, |s, _| info(s)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_navigate_delete() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
        assert_eq!(s.execute("timeline.info", json!({})).unwrap()["timeline"], Value::Null);
        let r = s.execute("timeline.create", json!({"duration": 24, "fps": 24.0})).unwrap();
        assert_eq!(r["timeline"]["duration"], 24);
        assert_eq!(r["timeline"]["current"], 0);
        // Navigate.
        s.execute("timeline.setFrame", json!({"frame": 10})).unwrap();
        s.execute("timeline.nextFrame", json!({})).unwrap();
        assert_eq!(s.execute("timeline.info", json!({})).unwrap()["timeline"]["current"], 11);
        s.execute("timeline.previousFrame", json!({})).unwrap();
        assert_eq!(s.execute("timeline.info", json!({})).unwrap()["timeline"]["current"], 10);
        // Clamp beyond the end.
        let r = s.execute("timeline.setFrame", json!({"frame": 999})).unwrap();
        assert_eq!(r["timeline"]["current"], 23);
        // Props.
        s.execute("timeline.setProps", json!({"fps": 30.0})).unwrap();
        assert_eq!(s.execute("timeline.info", json!({})).unwrap()["timeline"]["fps"], 30.0);
        // Delete.
        s.execute("timeline.delete", json!({})).unwrap();
        assert_eq!(s.execute("timeline.info", json!({})).unwrap()["timeline"], Value::Null);
    }

    /// The document's timeline, revision and history depth, to show a rejected command changed nothing.
    fn snapshot(s: &Session) -> (Option<Timeline>, u64, usize) {
        let st = s.active().unwrap();
        (st.doc.timeline.clone(), st.revision, st.history.past_len())
    }

    fn current(s: &mut Session) -> Value {
        s.execute("timeline.info", json!({})).unwrap()["timeline"]["current"].clone()
    }

    /// #1009: `timeline.create` kept any `duration` (even `u64::MAX`) and `fps`, and stepping then
    /// panicked in `i64::clamp`. Bad values are now rejected before anything changes.
    #[test]
    fn create_rejects_out_of_range_params() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        s.execute("timeline.create", json!({"duration": 24, "fps": 24.0})).unwrap();
        s.execute("timeline.setFrame", json!({"frame": 5})).unwrap();
        let before = snapshot(&s);
        let too_long = MAX_DURATION as u64 + 1;
        for p in [
            json!({"duration": 0}),
            json!({"duration": u64::MAX}),
            json!({"duration": i64::MAX as u64 + 1}),
            json!({"duration": too_long}),
            json!({"duration": -1}),
            json!({"duration": 2.5}),
            json!({"duration": "24"}),
            json!({"fps": 0.0}),
            json!({"fps": -24.0}),
            json!({"fps": MAX_FPS as f64 + 1.0}),
            json!({"fps": 1e300}),
            json!({"fps": "fast"}),
        ] {
            let e = s.execute("timeline.create", p.clone()).expect_err(&format!("{p} must be rejected"));
            assert!(matches!(e, EngineError::BadParams { .. }), "{p}: {e}");
            assert_eq!(snapshot(&s), before, "{p}: nothing changes");
        }
        // The issue's repro: stepping still works on the surviving timeline.
        s.execute("timeline.nextFrame", json!({})).unwrap();
        assert_eq!(current(&mut s), 6);
        s.execute("timeline.previousFrame", json!({})).unwrap();
        assert_eq!(current(&mut s), 5);
        // Control: the largest accepted values still create a timeline that steps at both ends.
        let r = s.execute("timeline.create", json!({"duration": MAX_DURATION, "fps": MAX_FPS as f64})).unwrap();
        assert_eq!(r["timeline"]["duration"], MAX_DURATION);
        s.execute("timeline.previousFrame", json!({})).unwrap();
        assert_eq!(current(&mut s), 0);
        s.execute("timeline.setFrame", json!({"frame": MAX_DURATION})).unwrap();
        s.execute("timeline.nextFrame", json!({})).unwrap();
        assert_eq!(current(&mut s), MAX_DURATION - 1);
        // Control: `null` means "use the default", as before.
        let r = s.execute("timeline.create", json!({"duration": null, "fps": null})).unwrap();
        assert_eq!((r["timeline"]["duration"].as_u64(), r["timeline"]["fps"].as_f64()), (Some(30), Some(30.0)));
    }

    /// #1009: `timeline.setProps` could store the same out-of-range `duration` and `fps`.
    #[test]
    fn set_props_rejects_out_of_range_params() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        s.execute("timeline.create", json!({"duration": 24, "fps": 24.0})).unwrap();
        let before = snapshot(&s);
        for p in [json!({"duration": 0}), json!({"duration": u64::MAX}), json!({"fps": 0.0}), json!({"fps": -1.0}), json!({"fps": 30.0, "duration": 0})] {
            let e = s.execute("timeline.setProps", p.clone()).expect_err(&format!("{p} must be rejected"));
            assert!(matches!(e, EngineError::BadParams { .. }), "{p}: {e}");
            assert_eq!(snapshot(&s), before, "{p}: nothing changes");
        }
        s.execute("timeline.nextFrame", json!({})).unwrap();
        assert_eq!(current(&mut s), 1);
    }

    /// A longer `duration` moves a work area that reached the old end to the new end, like
    /// `video_cmds::grow_duration` (#4170); a narrowed one and an explicit `workEnd` stay put.
    #[test]
    fn set_props_longer_duration_moves_full_work_area() {
        let work = |r: &Value| (r["timeline"]["workStart"].as_u64(), r["timeline"]["workEnd"].as_u64());
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        s.execute("timeline.create", json!({"duration": 30})).unwrap();
        let r = s.execute("timeline.setProps", json!({"duration": 60})).unwrap();
        assert_eq!(work(&r), (Some(0), Some(60)), "the full work area follows the new end");
        let r = s.execute("timeline.setProps", json!({"duration": 90, "workEnd": 45})).unwrap();
        assert_eq!(work(&r), (Some(0), Some(45)), "an explicit workEnd wins");
        s.execute("timeline.setProps", json!({"workStart": 2, "workEnd": 10})).unwrap();
        let r = s.execute("timeline.setProps", json!({"duration": 120})).unwrap();
        assert_eq!(work(&r), (Some(2), Some(10)), "a narrowed work area stays put");
    }

    /// #1009: a timeline stored with `duration` 0 or `u64::MAX` (an old or hand-edited file) steps
    /// without panicking and lands on a valid frame.
    #[test]
    fn step_is_safe_on_any_stored_timeline() {
        for duration in [0, 1, usize::MAX] {
            for current in [0, usize::MAX] {
                for (cmd, delta) in [("timeline.nextFrame", 1i64), ("timeline.previousFrame", -1)] {
                    let mut s = Session::new();
                    s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
                    let st = s.active_mut().unwrap();
                    let mut doc = (*st.doc).clone();
                    doc.timeline = Some(Timeline { fps: f32::NAN, duration, current, work_start: usize::MAX, work_end: 0, start_number: 0, loop_playback: true });
                    st.doc = Arc::new(doc);
                    let r = s.execute(cmd, json!({})).unwrap_or_else(|e| panic!("{cmd} d={duration} c={current}: {e}"));
                    let t = &r["timeline"];
                    let d = t["duration"].as_u64().unwrap();
                    let c = t["current"].as_u64().unwrap();
                    assert!(d >= 1 && c < d, "{cmd} d={duration} c={current}: {t}");
                    if duration == usize::MAX && current == 0 {
                        assert_eq!(c, if delta > 0 { 1 } else { 0 });
                    }
                }
            }
        }
    }

    fn new_timeline(duration: u64) -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        s.execute("timeline.create", json!({"duration": duration, "fps": 24.0})).unwrap();
        s
    }

    fn work(r: &Value) -> (u64, u64) {
        (r["timeline"]["workStart"].as_u64().unwrap(), r["timeline"]["workEnd"].as_u64().unwrap())
    }

    /// Plate frame numbers: `startNumber` numbers frame 0, `setFrame {number}` goes by it and
    /// `timeline.info` reports both; frame indices work as before.
    #[test]
    fn set_frame_by_plate_number() {
        let mut s = new_timeline(28);
        let r = s.execute("timeline.info", json!({})).unwrap();
        assert_eq!((r["timeline"]["startNumber"].as_i64(), r["timeline"]["loop"].as_bool()), (Some(0), Some(true)), "{r}");
        s.execute("timeline.setProps", json!({"startNumber": 1001})).unwrap();
        assert_eq!(s.execute("timeline.setFrame", json!({"number": 1020})).unwrap()["timeline"]["current"], 19);
        let r = s.execute("timeline.info", json!({})).unwrap();
        assert_eq!((r["timeline"]["current"].as_u64(), r["timeline"]["startNumber"].as_i64()), (Some(19), Some(1001)));
        // Numbers outside the timeline land on its ends.
        assert_eq!(s.execute("timeline.setFrame", json!({"number": 5})).unwrap()["timeline"]["current"], 0);
        assert_eq!(s.execute("timeline.setFrame", json!({"number": 99_999})).unwrap()["timeline"]["current"], 27);
        assert_eq!(s.execute("timeline.setFrame", json!({"frame": 12})).unwrap()["timeline"]["current"], 12);
        let before = snapshot(&s);
        for p in [json!({}), json!({"frame": 1, "number": 1002}), json!({"number": "1002"}), json!({"frame": -1}), json!({"frames": 3})] {
            let e = s.execute("timeline.setFrame", p.clone()).expect_err(&format!("{p} must be rejected"));
            assert!(matches!(e, EngineError::BadParams { .. }), "{p}: {e}");
            assert_eq!(snapshot(&s), before, "{p}: nothing changes");
        }
    }

    /// `timeline.setProps` takes `startNumber` and `loop`, and names a key it doesn't know.
    #[test]
    fn set_props_takes_start_number_and_loop_and_refuses_unknown_keys() {
        let mut s = new_timeline(28);
        let r = s.execute("timeline.setProps", json!({"startNumber": 1001, "loop": false})).unwrap();
        assert_eq!((r["timeline"]["startNumber"].as_i64(), r["timeline"]["loop"].as_bool()), (Some(1001), Some(false)), "{r}");
        let before = snapshot(&s);
        for p in [
            json!({"startFrame": 1001}),
            json!({"startNumber": 1.5}),
            json!({"startNumber": "1001"}),
            json!({"startNumber": MAX_START_NUMBER + 1}),
            json!({"loop": 1}),
            json!({"workStart": -2}),
            json!({"workEnd": "10"}),
        ] {
            let e = s.execute("timeline.setProps", p.clone()).expect_err(&format!("{p} must be rejected"));
            assert!(matches!(e, EngineError::BadParams { .. }), "{p}: {e}");
            assert_eq!(snapshot(&s), before, "{p}: nothing changes");
        }
        let e = s.execute("timeline.setProps", json!({"startFrame": 1001})).unwrap_err().to_string();
        assert!(e.contains("startFrame") && e.contains("startNumber"), "{e}");
        // Negative start numbers are allowed (some pipelines count from -10).
        let r = s.execute("timeline.setProps", json!({"startNumber": -10, "loop": true})).unwrap();
        assert_eq!(r["timeline"]["startNumber"], -10);
    }

    #[test]
    fn work_area_commands_follow_the_playhead_and_reset() {
        let mut s = new_timeline(28);
        s.execute("timeline.setProps", json!({"startNumber": 1001})).unwrap();
        s.execute("timeline.setFrame", json!({"number": 1009})).unwrap();
        assert_eq!(work(&s.execute("timeline.setWorkAreaStart", json!({})).unwrap()), (8, 28));
        s.execute("timeline.setFrame", json!({"number": 1025})).unwrap();
        assert_eq!(work(&s.execute("timeline.setWorkAreaEnd", json!({})).unwrap()), (8, 25), "1009-1025 is 17 frames");
        // An end before the start, or a start after the end, moves the other end too.
        assert_eq!(work(&s.execute("timeline.setWorkAreaEnd", json!({"frame": 4})).unwrap()), (4, 5));
        assert_eq!(work(&s.execute("timeline.setWorkAreaStart", json!({"number": 1020})).unwrap()), (19, 20));
        assert_eq!(work(&s.execute("timeline.setWorkAreaEnd", json!({"number": 99_999})).unwrap()), (19, 28));
        assert_eq!(work(&s.execute("timeline.resetWorkArea", json!({})).unwrap()), (0, 28));
        // Transport.
        let current = |r: Value| r["timeline"]["current"].as_u64().unwrap();
        assert_eq!(current(s.execute("timeline.lastFrame", json!({})).unwrap()), 27);
        assert_eq!(current(s.execute("timeline.back10Frames", json!({})).unwrap()), 17);
        assert_eq!(current(s.execute("timeline.forward10Frames", json!({})).unwrap()), 27, "stops at the last frame");
        assert_eq!(current(s.execute("timeline.firstFrame", json!({})).unwrap()), 0);
        assert_eq!(current(s.execute("timeline.back10Frames", json!({})).unwrap()), 0, "stops at the first frame");
        let r = s.execute("timeline.toggleLoop", json!({})).unwrap();
        assert_eq!(r["timeline"]["loop"], false);
        let r = s.execute("timeline.toggleLoop", json!({})).unwrap();
        assert_eq!(r["timeline"]["loop"], true);
        assert!(s.execute("timeline.setWorkAreaStart", json!({"frames": 2})).is_err(), "unknown keys are refused");
    }

    #[test]
    fn navigation_without_timeline_errors() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        assert!(s.execute("timeline.setFrame", json!({"frame": 1})).is_err());
    }
}
