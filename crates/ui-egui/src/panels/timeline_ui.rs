//! Window › Timeline: the document timeline, laid out like Photoshop's Timeline panel for paint and
//! cleanup work. A frame ruler numbered from the timeline's start number (1001 for a plate) with a
//! playhead to click or drag, zoom and scroll; the work area's brackets; transport (first,
//! previous, play/pause, next, last, loop); one track per video layer, with a tick on each painted
//! frame; a panel menu; and, with Enable Timeline Shortcut Keys, Space, the arrows, Home and End.
//! Docked full width along the bottom of the window by default, or floating.
//!
//! Every action runs an engine `timeline.*` (or layer) command, so the control channel drives the
//! timeline the same way. Render Video… runs File › Export › Render Video… in the range and format
//! chosen next to it.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use photocraft_doc::{DocId, LayerContent, LayerId, Timeline};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::Tokens;

/// The image format Render Video writes each frame in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderFormat {
    #[default]
    Png,
    Jpg,
    Exr,
}

impl RenderFormat {
    /// Every format, with the name the Timeline's format dropdown shows.
    pub const OPTIONS: [(RenderFormat, &'static str); 3] = [(RenderFormat::Png, "PNG"), (RenderFormat::Jpg, "JPG"), (RenderFormat::Exr, "OpenEXR")];

    /// The `format` param of `file.export.renderVideo`, which is also the frames' extension.
    pub fn ext(self) -> &'static str {
        match self {
            RenderFormat::Png => "png",
            RenderFormat::Jpg => "jpg",
            RenderFormat::Exr => "exr",
        }
    }
}

/// The frames Render Video writes from the panel and from File › Export › Render Video….
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderRange {
    /// The work area.
    #[default]
    WorkArea,
    /// Every frame of the timeline.
    All,
}

impl RenderRange {
    pub const OPTIONS: [(RenderRange, &'static str); 2] = [(RenderRange::WorkArea, "Work Area"), (RenderRange::All, "All Frames")];

    /// The `range` param of `file.export.renderVideo`.
    pub fn param(self) -> &'static str {
        match self {
            RenderRange::WorkArea => "workArea",
            RenderRange::All => "all",
        }
    }
}

/// Play/Pause: the panel's play button, and a command keys can be bound to.
pub const PLAY_PAUSE: &str = "timeline.playPause";

/// The commands Edit › Keyboard Shortcuts lists under Timeline. None has a key of its own: any can
/// be bound there, and Enable Timeline Shortcut Keys gives the panel Photoshop's keys
/// ([`TIMELINE_KEYS`]).
pub const SHORTCUT_COMMANDS: [(&str, &str); 11] = [
    (PLAY_PAUSE, "Play/Pause"),
    ("timeline.previousFrame", "Previous Frame"),
    ("timeline.nextFrame", "Next Frame"),
    ("timeline.back10Frames", "Back 10 Frames"),
    ("timeline.forward10Frames", "Forward 10 Frames"),
    ("timeline.firstFrame", "First Frame"),
    ("timeline.lastFrame", "Last Frame"),
    ("timeline.setWorkAreaStart", "Set Start of Work Area"),
    ("timeline.setWorkAreaEnd", "Set End of Work Area"),
    ("timeline.resetWorkArea", "Reset Work Area"),
    ("timeline.toggleLoop", "Loop Playback"),
];

/// The keys Enable Timeline Shortcut Keys gives the timeline, as in Photoshop. ⇧ first:
/// `consume_key` ignores an extra ⇧.
const TIMELINE_KEYS: [(egui::Modifiers, egui::Key, &str); 7] = [
    (egui::Modifiers::SHIFT, egui::Key::ArrowLeft, "timeline.back10Frames"),
    (egui::Modifiers::SHIFT, egui::Key::ArrowRight, "timeline.forward10Frames"),
    (egui::Modifiers::NONE, egui::Key::ArrowLeft, "timeline.previousFrame"),
    (egui::Modifiers::NONE, egui::Key::ArrowRight, "timeline.nextFrame"),
    (egui::Modifiers::NONE, egui::Key::Home, "timeline.firstFrame"),
    (egui::Modifiers::NONE, egui::Key::End, "timeline.lastFrame"),
    (egui::Modifiers::NONE, egui::Key::Space, PLAY_PAUSE),
];

// Layout, in points.
const HEADER_W: f32 = 190.0;
const RULER_H: f32 = 22.0;
const WORK_H: f32 = 10.0;
const ROW_H: f32 = 34.0;
const FOOTER_H: f32 = 28.0;
/// Width of the floating panel.
const FLOAT_W: f32 = 780.0;
/// Most points a frame gets zoomed in.
const MAX_ZOOM: f32 = 96.0;
/// A video layer's clip.
const CLIP: Color32 = Color32::from_rgb(104, 90, 178);
/// The playhead.
const PLAYHEAD: Color32 = Color32::from_rgb(232, 72, 72);

/// Timeline Settings… while its dialog is open.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDraft {
    pub fps: f32,
    pub start_number: i64,
}

/// A video layer's row in the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub layer: u64,
    pub name: String,
    pub visible: bool,
    pub frames: usize,
    /// Timeline frames painted over: those Restore Frame would change.
    pub altered: Vec<usize>,
}

/// Timeline panel view state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineUi {
    pub open: bool,
    /// Playing back (the playhead follows the clock at the frame rate).
    #[serde(default)]
    pub playing: bool,
    /// Where playback counts from: the egui time (seconds) and the frame shown then. Cleared when
    /// playback starts; set again whenever something else moves the playhead.
    #[serde(default, skip)]
    pub play_anchor: Option<(f64, usize)>,
    /// The frame playback last asked for, to notice when something else moved the playhead.
    #[serde(default, skip)]
    pub play_frame: Option<usize>,
    /// The format Render Video writes, from the panel's Render button and from File › Export ›
    /// Render Video… alike, unless the caller names one.
    #[serde(default)]
    pub render_format: RenderFormat,
    /// The frames Render Video writes, likewise.
    #[serde(default)]
    pub render_range: RenderRange,
    /// Docked full width along the bottom of the window (the default), else a floating panel.
    #[serde(default = "on")]
    pub docked: bool,
    /// Panel menu › Enable Timeline Shortcut Keys: Space, ← / →, ⇧← / ⇧→, Home and End drive the
    /// timeline while the panel is open ([`TIMELINE_KEYS`]). On by default, as in Photoshop.
    #[serde(default = "on")]
    pub shortcut_keys: bool,
    /// Points per frame on the ruler; `None` fits every frame in the panel.
    #[serde(default)]
    pub zoom: Option<f32>,
    /// The frame at the ruler's left edge (fractional while zoomed in).
    #[serde(default)]
    pub scroll: f32,
    /// Timeline Settings… while its dialog is open.
    #[serde(default, skip)]
    pub settings: Option<SettingsDraft>,
    /// Go To › Frame…: the frame field takes the keyboard on the next frame.
    #[serde(default, skip)]
    pub focus_frame: bool,
    /// Each track's painted frames, and the document revision they were found at.
    #[serde(default, skip)]
    altered: Vec<(u64, Vec<usize>)>,
    #[serde(default, skip)]
    altered_at: Option<(DocId, u64)>,
}

fn on() -> bool {
    true
}

impl Default for TimelineUi {
    fn default() -> Self {
        TimelineUi {
            open: false,
            playing: false,
            play_anchor: None,
            play_frame: None,
            render_format: RenderFormat::default(),
            render_range: RenderRange::default(),
            docked: true,
            shortcut_keys: true,
            zoom: None,
            scroll: 0.0,
            settings: None,
            focus_frame: false,
            altered: Vec::new(),
            altered_at: None,
        }
    }
}

/// Menu checkmark for Window › Timeline.
pub fn checked(app: &PhotocraftApp, id: &str) -> Option<bool> {
    (id == "window.panel.timeline").then_some(app.ui.timeline.open)
}

/// Ids this module owns (for parity / enablement).
pub fn handles(id: &str) -> bool {
    id == "window.panel.timeline" || id == PLAY_PAUSE
}

/// Toggle the Timeline panel; Play/Pause.
pub fn menu(app: &mut PhotocraftApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    match id {
        "window.panel.timeline" => {
            app.ui.timeline.open = !app.ui.timeline.open;
            Some(Ok(json!({ "open": app.ui.timeline.open })))
        }
        PLAY_PAUSE => Some(play_pause(app)),
        _ => None,
    }
}

/// The panel's playback state with the engine's timeline: `{playing, open, docked, shortcutKeys,
/// renderFormat, renderRange, timeline}` (`timeline` is null without a document timeline).
pub fn state(app: &PhotocraftApp) -> Value {
    let timeline = app.session.active().map_or(Value::Null, |d| photocraft_engine::timeline_json(&d.doc, false));
    json!({
        "playing": app.ui.timeline.playing,
        "open": app.ui.timeline.open,
        "docked": app.ui.timeline.docked,
        "shortcutKeys": app.ui.timeline.shortcut_keys,
        "renderFormat": app.ui.timeline.render_format,
        "renderRange": app.ui.timeline.render_range,
        "timeline": timeline,
    })
}

/// Adds the panel's `playing` to an engine reply's `timeline` (`timeline.info` and the other
/// `timeline.*` commands over the control channel): the engine has the timeline, the panel plays it.
pub fn with_playing(app: &PhotocraftApp, mut reply: Value) -> Value {
    if let Some(t) = reply.get_mut("timeline").and_then(Value::as_object_mut) {
        t.insert("playing".into(), json!(app.ui.timeline.playing));
    }
    reply
}

/// The control method `timeline.play`: the Timeline panel's Play. Opens the panel, whose frame
/// loop moves the playhead, and starts from the current frame; with Loop Playback off and the
/// playhead on the work area's last frame, from the work area's first.
pub fn play(app: &mut PhotocraftApp) -> Result<Value, String> {
    let document = app.session.active().ok_or("no document")?;
    let Some(t) = document.doc.timeline.clone() else {
        return Err("the document has no timeline (Create Video Timeline, or load footage)".into());
    };
    let work = t.work_frames();
    if !t.loop_playback && t.current + 1 >= work.end {
        app.run("timeline.setFrame", json!({ "frame": work.start }))?;
    }
    app.ui.timeline.open = true;
    app.ui.timeline.playing = true;
    app.ui.timeline.play_anchor = None;
    Ok(state(app))
}

/// Play/Pause: pause while playing, else [`play`].
pub fn play_pause(app: &mut PhotocraftApp) -> Result<Value, String> {
    if app.ui.timeline.playing {
        app.ui.timeline.playing = false;
        return Ok(state(app));
    }
    play(app)
}

/// The control method `timeline.stop {rewind?}`: pauses at the current frame; `rewind: true`
/// also goes back to the first frame.
pub fn stop(app: &mut PhotocraftApp, p: &Value) -> Result<Value, String> {
    let rewind = match p.get("rewind") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("`rewind` must be true or false".into()),
    };
    app.ui.timeline.playing = false;
    if rewind {
        app.run("timeline.setFrame", json!({ "frame": 0 }))?;
    }
    Ok(state(app))
}

/// The frame playback shows at `now`, and whether playback goes on: counted at the timeline's
/// rate from where playback started, or from where the playhead was moved to since, inside the
/// work area. It loops there, or with Loop Playback off stops on its last frame. A frame that
/// takes longer to show than the rate allows (a large float plate) makes playback skip frames,
/// not slow down.
fn playback_frame(ui: &mut TimelineUi, t: &Timeline, now: f64) -> (usize, bool) {
    let work = t.work_frames();
    let (start, from) = match ui.play_anchor {
        Some(anchor) if ui.play_frame == Some(t.current) => anchor,
        // Just started, or Next Frame, a command or the channel moved the playhead. Outside the
        // work area playback starts at its first frame.
        _ => {
            let from = if work.contains(&t.current) { t.current } else { work.start };
            let anchor = (now, from);
            ui.play_anchor = Some(anchor);
            anchor
        }
    };
    let elapsed = now - start;
    let elapsed = if elapsed.is_finite() { elapsed.max(0.0) } else { 0.0 };
    // `as` saturates, so a clock running for days cannot wrap.
    let advanced = (elapsed * f64::from(t.fps.max(1.0))).floor() as usize;
    let offset = from.saturating_sub(work.start).saturating_add(advanced);
    let len = work.len().max(1);
    let (next, more) = if t.loop_playback {
        (work.start + offset % len, true)
    } else if offset >= len - 1 {
        (work.end - 1, false)
    } else {
        (work.start + offset, true)
    };
    ui.play_frame = Some(next);
    (next, more)
}

/// Playback: the playhead follows the clock at the frame rate ([`playback_frame`]).
fn tick(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.timeline.playing {
        return;
    }
    let Some(t) = app.session.active().and_then(|d| d.doc.timeline.clone()) else {
        app.ui.timeline.playing = false;
        return;
    };
    let now = ctx.input(|i| i.time);
    let (next, more) = playback_frame(&mut app.ui.timeline, &t, now);
    if next != t.current
        && let Err(e) = app.run("timeline.setFrame", json!({ "frame": next as u64 }))
    {
        // Say why instead of playing silently on one frame.
        app.ui.timeline.playing = false;
        report(app, e);
        return;
    }
    if more {
        ctx.request_repaint();
    } else {
        app.ui.timeline.playing = false;
    }
}

fn report(app: &mut PhotocraftApp, e: String) {
    app.ui.status = e;
    app.ui.status_error = true;
}

/// Whether Enable Timeline Shortcut Keys has the keys now: on, the panel open, a timeline in the
/// active document, and nobody typing: not in a field, not on the canvas, no Type tool.
pub fn keys_active(app: &PhotocraftApp, ctx: &egui::Context) -> bool {
    let ui = &app.ui.timeline;
    ui.open
        && ui.shortcut_keys
        && !ctx.text_edit_focused()
        && app.ui.text_edit.is_none()
        && !app.ui.tool.is_type()
        && app.session.active().is_some_and(|d| d.doc.timeline.is_some())
}

/// Enable Timeline Shortcut Keys: Space plays and pauses, ← / → step a frame (⇧ ten), Home / End
/// go to the first / last frame, ahead of the Move tool's nudge; Free Transform keeps its arrows.
/// Returns true when a key was used.
pub fn keys(app: &mut PhotocraftApp, ctx: &egui::Context) -> bool {
    if !keys_active(app, ctx) || app.ui.transform.is_some() {
        return false;
    }
    for (mods, key, id) in TIMELINE_KEYS {
        if ctx.input_mut(|i| i.consume_key(mods, key)) {
            let result = if id == PLAY_PAUSE {
                play_pause(app)
            } else {
                app.ui.timeline.playing = false;
                app.run(id, json!({}))
            };
            if let Err(e) = result {
                report(app, e);
            }
            return true;
        }
    }
    false
}

/// The active document's video layers, top first, with their painted frames. Those are compared
/// with the footage again once the document has changed and settled: not while playback or a drag
/// (scrubbing, painting) changes it every frame.
fn tracks(app: &mut PhotocraftApp, ctx: &egui::Context) -> Vec<Track> {
    let Some(st) = app.session.active() else { return Vec::new() };
    let at = (st.doc.id, st.revision);
    let settled = !app.ui.timeline.playing && !ctx.input(|i| i.pointer.any_down());
    let fresh = settled && app.ui.timeline.altered_at != Some(at);
    let current = st.doc.timeline.as_ref().map_or(0, |t| t.current);
    let known = &app.ui.timeline.altered;
    let tracks: Vec<Track> = st
        .doc
        .walk()
        .into_iter()
        .rev()
        .filter_map(|(_, _, l)| {
            let v = l.video.as_ref()?;
            let altered = if fresh {
                let live = match &l.content {
                    LayerContent::Raster(s) => Some(s),
                    _ => None,
                };
                photocraft_engine::altered_frames(v, live, current)
            } else {
                known.iter().find(|(id, _)| *id == l.id.0).map(|(_, a)| a.clone()).unwrap_or_default()
            };
            Some(Track { layer: l.id.0, name: l.name.clone(), visible: l.visible, frames: v.len(), altered })
        })
        .collect();
    if fresh {
        app.ui.timeline.altered = tracks.iter().map(|t| (t.layer, t.altered.clone())).collect();
        app.ui.timeline.altered_at = Some(at);
    }
    tracks
}

/// What the panel shows this frame, gathered first: it can't borrow the app while it draws.
struct View {
    have_doc: bool,
    timeline: Option<Timeline>,
    /// Each track with its thumbnail, and whether its layer is the active one.
    tracks: Vec<(Track, Option<egui::TextureId>, bool)>,
    playing: bool,
    state: TimelineUi,
}

impl View {
    fn of(app: &mut PhotocraftApp, ctx: &egui::Context) -> View {
        let tracks = tracks(app, ctx);
        let doc = app.session.active().map(|d| (d.doc.clone(), d.active_layer));
        let mut rows = Vec::new();
        if let Some((doc, active)) = &doc {
            for track in tracks {
                let thumb = doc.layer(LayerId(track.layer)).map(|l| app.layer_thumb(ctx, doc, l).0);
                let is_active = active.is_some_and(|a| a.0 == track.layer);
                rows.push((track, thumb, is_active));
            }
        }
        View {
            have_doc: doc.is_some(),
            timeline: doc.as_ref().and_then(|(d, _)| d.timeline.clone()),
            tracks: rows,
            playing: app.ui.timeline.playing,
            state: app.ui.timeline.clone(),
        }
    }
}

/// What the panel asked for this frame, applied once it's drawn.
#[derive(Default)]
struct Out {
    /// Commands to run, in order (the first error stops the rest).
    run: Vec<(&'static str, Value)>,
    pause: bool,
    toggle_play: bool,
    /// `Some(None)` fits every frame.
    zoom: Option<Option<f32>>,
    scroll: Option<f32>,
    render: bool,
    render_format: Option<RenderFormat>,
    render_range: Option<RenderRange>,
    close: bool,
    docked: Option<bool>,
    shortcut_keys: Option<bool>,
    open_settings: bool,
    focus_frame: Option<bool>,
}

impl Out {
    /// A transport command: pauses playback, then runs `id`.
    fn step(&mut self, id: &'static str) {
        self.pause = true;
        self.run.push((id, json!({})));
    }
}

fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, out: Out) {
    let ui = &mut app.ui.timeline;
    if let Some(f) = out.render_format {
        ui.render_format = f;
    }
    if let Some(r) = out.render_range {
        ui.render_range = r;
    }
    if let Some(z) = out.zoom {
        ui.zoom = z;
    }
    if let Some(s) = out.scroll {
        ui.scroll = s;
    }
    if let Some(d) = out.docked {
        ui.docked = d;
    }
    if let Some(k) = out.shortcut_keys {
        ui.shortcut_keys = k;
    }
    if let Some(f) = out.focus_frame {
        ui.focus_frame = f;
    }
    if out.close {
        ui.open = false;
        ui.playing = false;
    }
    if out.pause {
        ui.playing = false;
    }
    for (cmd, p) in out.run {
        if let Err(e) = app.run(cmd, p) {
            report(app, e);
            break;
        }
    }
    if out.toggle_play
        && !out.pause
        && let Err(e) = play_pause(app)
    {
        report(app, e);
    }
    if out.open_settings
        && let Some(t) = app.session.active().and_then(|d| d.doc.timeline.as_ref())
    {
        app.ui.timeline.settings = Some(SettingsDraft { fps: t.fps, start_number: t.start_number });
    }
    // The menu command asks for the output folder first; nothing is written if that is cancelled.
    if out.render {
        let params = json!({ "format": app.ui.timeline.render_format.ext(), "range": app.ui.timeline.render_range.param() });
        if let Err(e) = crate::menus::invoke(app, ctx, "file.export.renderVideo", params) {
            report(app, e);
        }
    }
}

/// The floating panel, Timeline Settings…, and playback. Call every frame.
pub fn windows(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.timeline.open {
        return;
    }
    if !app.ui.timeline.docked {
        let view = View::of(app, ctx);
        let mut out = Out::default();
        crate::analysis_ui::panel_window(app, ctx, "timeline", tl!("Timeline"), vec2(0.0, 520.0), FLOAT_W, |ui| body(ui, &view, &mut out, true));
        apply(app, ctx, out);
    }
    settings_window(app, ctx);
    tick(app, ctx);
}

/// The docked panel: full width along the bottom of the window, above the status bar, sized with
/// its top edge. Call before the side panels and the canvas take their room.
pub fn dock(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    if !app.ui.timeline.open || !app.ui.timeline.docked {
        return;
    }
    let ctx = ui.ctx().clone();
    let view = View::of(app, &ctx);
    let mut out = Out::default();
    let t = Tokens::get(&ctx);
    let rows = view.tracks.len().clamp(1, 4) as f32;
    let height = 36.0 + RULER_H + WORK_H + rows * ROW_H + FOOTER_H + 40.0;
    egui::Panel::bottom("timeline-dock")
        .resizable(true)
        .default_size(height)
        .size_range(150.0..=640.0)
        .frame(egui::Frame::NONE.fill(t.dock).inner_margin(egui::Margin::symmetric(8, 4)))
        .show(ui, |ui| body(ui, &view, &mut out, false));
    apply(app, &ctx, out);
}

/// Timeline Settings…: frame rate and start number.
fn settings_window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(mut draft) = app.ui.timeline.settings.clone() else { return };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(tl!("Timeline Settings")).id(egui::Id::unique("timeline-settings")).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("timeline-settings-grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            ui.label(tl!("Frame Rate"));
            ui.add(egui::DragValue::new(&mut draft.fps).range(0.01..=1000.0).speed(0.05).max_decimals(3).suffix(" fps"));
            ui.end_row();
            ui.label(tl!("Start Number"));
            ui.add(egui::DragValue::new(&mut draft.start_number).range(-photocraft_doc::MAX_START_NUMBER..=photocraft_doc::MAX_START_NUMBER).speed(1.0));
            ui.end_row();
        });
        ui.weak(tl!("The number the first frame shows: 1001 for most plates."));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ok = crate::widgets::primary_button(ui, tl!("OK"), 80.0).clicked();
            cancel = crate::widgets::secondary_button(ui, tl!("Cancel"), 80.0).clicked();
        });
    });
    if ok {
        if let Err(e) = app.run("timeline.setProps", json!({ "fps": draft.fps, "startNumber": draft.start_number })) {
            report(app, e);
        }
        app.ui.timeline.settings = None;
    } else if cancel || app.session.active().is_none_or(|d| d.doc.timeline.is_none()) {
        app.ui.timeline.settings = None;
    } else {
        app.ui.timeline.settings = Some(draft);
    }
}

/// The ruler's geometry: frame `f` spans `x(f)..x(f + 1)`.
#[derive(Clone, Copy)]
struct Geo {
    left: f32,
    width: f32,
    /// Points per frame.
    ppf: f32,
    /// The frame at the left edge.
    scroll: f32,
    duration: usize,
}

impl Geo {
    fn new(left: f32, width: f32, tl: &Timeline, state: &TimelineUi) -> Geo {
        let duration = tl.duration.max(1);
        let fit = width / duration as f32;
        let ppf = state.zoom.map_or(fit, |z| z.clamp(fit.min(MAX_ZOOM), MAX_ZOOM.max(fit)));
        let mut geo = Geo { left, width, ppf, scroll: 0.0, duration };
        geo.scroll = if state.scroll.is_finite() { state.scroll.clamp(0.0, geo.max_scroll()) } else { 0.0 };
        geo
    }

    fn right(&self) -> f32 {
        self.left + self.width
    }

    /// Points per frame with every frame shown.
    fn fit(&self) -> f32 {
        self.width / self.duration as f32
    }

    fn max_scroll(&self) -> f32 {
        (self.duration as f32 - self.width / self.ppf).max(0.0)
    }

    /// Screen x of frame position `frame` (`f + 0.5` is the middle of frame `f`).
    fn x(&self, frame: f32) -> f32 {
        self.left + (frame - self.scroll) * self.ppf
    }

    /// The frame under screen x, kept in the timeline.
    fn frame_at(&self, x: f32) -> usize {
        let f = ((x - self.left) / self.ppf + self.scroll).floor();
        if f.is_finite() { (f.max(0.0) as usize).min(self.duration - 1) } else { 0 }
    }

    /// The frame boundary nearest screen x, `0..=duration` (the work area's brackets sit on them).
    fn boundary_at(&self, x: f32) -> usize {
        let b = ((x - self.left) / self.ppf + self.scroll).round();
        if b.is_finite() { (b.max(0.0) as usize).min(self.duration) } else { 0 }
    }

    /// The timeline columns of a row.
    fn area(&self, y: egui::Rangef) -> Rect {
        Rect::from_x_y_ranges(self.left..=self.right(), y)
    }

    /// Zoom to `ppf` keeping frame position `anchor` where it is on screen: the new zoom (`None`
    /// once every frame fits) and scroll.
    fn zoomed(&self, ppf: f32, anchor: f32) -> (Option<f32>, f32) {
        let fit = self.fit();
        let ppf = ppf.clamp(fit.min(MAX_ZOOM), MAX_ZOOM.max(fit));
        if ppf <= fit * 1.001 {
            return (None, 0.0);
        }
        let offset = (anchor - self.scroll) * self.ppf;
        let scroll = (anchor - offset / ppf).clamp(0.0, (self.duration as f32 - self.width / ppf).max(0.0));
        (Some(ppf), scroll)
    }

    /// Where zooming with the buttons and slider keeps still: the playhead when it's in view,
    /// else the middle of the ruler.
    fn zoom_anchor(&self, current: usize) -> f32 {
        let x = self.x(current as f32 + 0.5);
        if (self.left..=self.right()).contains(&x) { current as f32 + 0.5 } else { self.scroll + self.width / self.ppf / 2.0 }
    }
}

/// Frame numbers apart between ruler labels: 1, 2 or 5 times a power of ten, at least `min_px`
/// points apart.
fn label_step(ppf: f32, min_px: f32) -> i64 {
    let mut decade: i64 = 1;
    loop {
        for m in [1, 2, 5] {
            let step = decade * m;
            if step as f32 * ppf >= min_px || decade >= 1_000_000_000 {
                return step;
            }
        }
        decade *= 10;
    }
}

/// The ruler's ticks between labels: the finest of a tenth, fifth or half of `step` that stays
/// 4 points apart, else none between.
fn minor_step(step: i64, ppf: f32) -> i64 {
    for d in [10, 5, 2] {
        if step % d == 0 && (step / d) as f32 * ppf >= 4.0 {
            return step / d;
        }
    }
    step
}

/// A frame rate as people write it: 24, 23.976, 29.97.
fn fps_text(fps: f32) -> String {
    let s = format!("{fps:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn body(ui: &mut egui::Ui, v: &View, out: &mut Out, floating: bool) {
    let t = Tokens::get(ui.ctx());
    if !floating {
        // A docked panel keeps the size its contents take: fill it exactly, so it neither grows
        // nor shrinks from one frame to the next (the tracks take what the rest leaves).
        ui.set_min_height(ui.available_height());
    }
    transport_row(ui, v, out, &t);
    ui.add_space(2.0);
    if !v.have_doc {
        ui.add_space(6.0);
        ui.weak(tl!("Open a document to use the timeline."));
        return;
    }
    let Some(tl) = &v.timeline else {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            if crate::widgets::primary_button(ui, tl!("Create Video Timeline"), 200.0).clicked() {
                out.run.push(("timeline.create", json!({ "duration": 30, "fps": 30.0 })));
            }
            ui.add_space(4.0);
            ui.weak(tl!("30 frames @ 30 fps"));
        });
        return;
    };
    let full = ui.available_width();
    let left = ui.available_rect_before_wrap().left();
    let header_w = HEADER_W.min(full * 0.35);
    let geo = Geo::new(left + header_w, (full - header_w - 4.0).max(40.0), tl, &v.state);
    // Playing past the edge of a zoomed-in ruler pages it along.
    if v.playing && v.state.zoom.is_some() {
        let x = geo.x(tl.current as f32 + 0.5);
        if !(geo.left..=geo.right()).contains(&x) {
            out.scroll = Some((tl.current as f32).min(geo.max_scroll()));
        }
    }
    let ruler = ruler(ui, &geo, tl, out, &t, full);
    work_bar(ui, &geo, tl, out, &t, full);
    let rows = if floating {
        v.tracks.len().clamp(1, 4) as f32 * ROW_H
    } else {
        // The footer and the spacing around it, with room to spare: taking more than the panel
        // has would grow it every frame.
        let footer = FOOTER_H + 3.0 + 2.0 * ui.spacing().item_spacing.y + 12.0;
        (ui.available_height() - footer).max(ROW_H)
    };
    let lanes = track_rows(ui, v, &geo, tl, out, &t, full, rows);
    overlays(ui, &geo, tl, ruler.top(), lanes.bottom());
    wheel(ui, &geo, out, Rect::from_min_max(ruler.min, lanes.max), ruler);
    footer(ui, v, &geo, tl, out);
}

fn transport_row(ui: &mut egui::Ui, v: &View, out: &mut Out, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.label(RichText::new(tl!("Timeline")).strong().color(t.text));
        ui.add_space(6.0);
        let tl = v.timeline.as_ref();
        ui.add_enabled_ui(tl.is_some(), |ui| {
            if crate::icons::button(ui, "chevrons-left", 22.0, false, tl!("First Frame")).clicked() {
                out.step("timeline.firstFrame");
            }
            if crate::icons::button(ui, "chevron-left", 22.0, false, tl!("Previous Frame")).clicked() {
                out.step("timeline.previousFrame");
            }
            let (icon, tip) = if v.playing { ("pause", tl!("Pause")) } else { ("play", tl!("Play")) };
            if crate::icons::button(ui, icon, 24.0, v.playing, tip).clicked() {
                out.toggle_play = true;
            }
            if crate::icons::button(ui, "chevron-right", 22.0, false, tl!("Next Frame")).clicked() {
                out.step("timeline.nextFrame");
            }
            if crate::icons::button(ui, "chevrons-right", 22.0, false, tl!("Last Frame")).clicked() {
                out.step("timeline.lastFrame");
            }
            let looping = tl.is_some_and(|t| t.loop_playback);
            if crate::icons::button(ui, "rotate-cw", 22.0, looping, tl!("Loop Playback")).clicked() {
                out.run.push(("timeline.toggleLoop", json!({})));
            }
        });
        if let Some(tl) = tl {
            ui.separator();
            // The current frame's number: type one and press Enter to go there.
            let last = tl.number(tl.duration.saturating_sub(1));
            let mut number = tl.number(tl.current);
            let field = ui
                .add(egui::DragValue::new(&mut number).range(tl.start_number..=last).speed(0.25).update_while_editing(false))
                .on_hover_text(tl!("Current frame: type a frame number and press Enter"));
            if v.state.focus_frame {
                field.request_focus();
                out.focus_frame = Some(false);
            }
            if field.changed() && number != tl.number(tl.current) {
                out.pause = true;
                out.run.push(("timeline.setFrame", json!({ "number": number })));
            }
            ui.label(RichText::new(format!("/ {last}")).color(t.text_dim).monospace());
            ui.separator();
            ui.label(RichText::new(format!("{:.2}s", tl.time())).color(t.text_dim).monospace());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::icons::button(ui, "x", 20.0, false, tl!("Close")).clicked() {
                out.close = true;
            }
            let menu = crate::widgets::panel_menu_button(ui);
            egui::Popup::menu(&menu).show(|ui| {
                crate::widgets::style_spectrum_popup_menu(ui);
                ui.set_min_width(230.0);
                panel_menu(ui, v, out);
            });
            if let Some(tl) = tl {
                ui.label(RichText::new(format!("{} fps", fps_text(tl.fps))).color(t.text_dim));
            }
        });
    });
}

/// The panel menu (≡).
fn panel_menu(ui: &mut egui::Ui, v: &View, out: &mut Out) {
    let tl = v.timeline.as_ref();
    let has = tl.is_some();
    if ui.add_enabled(has, egui::Button::new(tl!("Timeline Settings…"))).clicked() {
        out.open_settings = true;
        ui.close();
    }
    let mut looping = tl.is_some_and(|t| t.loop_playback);
    if ui.add_enabled(has, egui::Checkbox::new(&mut looping, tl!("Loop Playback"))).clicked() {
        out.run.push(("timeline.toggleLoop", json!({})));
        ui.close();
    }
    let mut keys = v.state.shortcut_keys;
    if ui.checkbox(&mut keys, tl!("Enable Timeline Shortcut Keys")).clicked() {
        out.shortcut_keys = Some(keys);
        ui.close();
    }
    ui.separator();
    ui.add_enabled_ui(has, |ui| {
        ui.menu_button(tl!("Go To"), |ui| {
            if ui.button(tl!("First Frame")).clicked() {
                out.step("timeline.firstFrame");
                ui.close();
            }
            if ui.button(tl!("Last Frame")).clicked() {
                out.step("timeline.lastFrame");
                ui.close();
            }
            if ui.button(tl!("Frame…")).clicked() {
                out.pause = true;
                out.focus_frame = Some(true);
                ui.close();
            }
        });
        ui.menu_button(tl!("Work Area"), |ui| {
            for (id, label) in [
                ("timeline.setWorkAreaStart", tl!("Set Start of Work Area")),
                ("timeline.setWorkAreaEnd", tl!("Set End of Work Area")),
                ("timeline.resetWorkArea", tl!("Reset Work Area")),
            ] {
                if ui.button(label).clicked() {
                    out.run.push((id, json!({})));
                    ui.close();
                }
            }
        });
    });
    ui.separator();
    if ui.add_enabled(has, egui::Button::new(tl!("Render Video…"))).clicked() {
        out.pause = true;
        out.render = true;
        ui.close();
    }
    let dock_label = if v.state.docked { tl!("Float Panel") } else { tl!("Dock at Bottom") };
    if ui.button(dock_label).clicked() {
        out.docked = Some(!v.state.docked);
        ui.close();
    }
    if ui.add_enabled(has, egui::Button::new(tl!("Delete Timeline"))).clicked() {
        out.step("timeline.delete");
        ui.close();
    }
    ui.separator();
    if ui.button(tl!("Close")).clicked() {
        out.close = true;
        ui.close();
    }
}

/// The frame ruler: numbers spaced for the zoom, ticks between. Click or drag to move the playhead.
fn ruler(ui: &mut egui::Ui, geo: &Geo, tl: &Timeline, out: &mut Out, t: &Tokens, full: f32) -> Rect {
    let (row, _) = ui.allocate_exact_size(vec2(full, RULER_H), Sense::hover());
    let area = geo.area(row.y_range());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, t.field);
    let last = tl.duration.saturating_sub(1);
    let digits = tl.number(0).to_string().len().max(tl.number(last).to_string().len());
    let step = label_step(geo.ppf, digits as f32 * 7.0 + 12.0);
    let minor = minor_step(step, geo.ppf);
    let first_shown = geo.scroll.floor().max(0.0) as usize;
    let last_shown = ((geo.scroll + geo.width / geo.ppf).ceil().max(0.0) as usize).min(last);
    let (n0, n1) = (tl.number(first_shown), tl.number(last_shown));
    let mut n = n0.div_euclid(minor) * minor;
    if n < n0 {
        n += minor;
    }
    while n <= n1 {
        let x = geo.x((n - tl.start_number) as f32 + 0.5);
        let major = n.rem_euclid(step) == 0;
        let h = if major { 7.0 } else { 4.0 };
        painter.line_segment([pos2(x, area.bottom() - h), pos2(x, area.bottom())], Stroke::new(1.0, if major { t.text_dim } else { t.text_faint }));
        if major {
            painter.text(pos2(x, area.top() + 2.0), Align2::CENTER_TOP, n, FontId::monospace(10.0), t.text_dim);
        }
        n += minor;
    }
    painter.line_segment([area.left_bottom(), area.right_bottom()], Stroke::new(1.0, t.separator));
    let r = ui.interact(area, ui.scope_id().with("timeline-ruler"), Sense::click_and_drag()).on_hover_text(tl!("Click or drag to move the playhead"));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, tl!("Timeline ruler")));
    if r.clicked() || r.dragged() || r.drag_started() {
        out.pause = true;
        if let Some(p) = r.interact_pointer_pos() {
            let f = geo.frame_at(p.x);
            if f != tl.current {
                out.run.push(("timeline.setFrame", json!({ "frame": f })));
            }
        }
    }
    area
}

/// The work area: a bar between two brackets under the ruler. Drag a bracket to move that end;
/// double-click to reset it to the whole timeline.
fn work_bar(ui: &mut egui::Ui, geo: &Geo, tl: &Timeline, out: &mut Out, t: &Tokens, full: f32) {
    let (row, _) = ui.allocate_exact_size(vec2(full, WORK_H), Sense::hover());
    let area = geo.area(row.y_range());
    let work = tl.work_frames();
    let (x0, x1) = (geo.x(work.start as f32), geo.x(work.end as f32));
    let painter = ui.painter_at(area);
    painter.rect_filled(Rect::from_x_y_ranges(x0..=x1, row.y_range().shrink(2.0)), 2.0, t.accent_soft);
    for x in [x0, x1] {
        painter.rect_filled(Rect::from_center_size(pos2(x, area.center().y), vec2(5.0, WORK_H)), 1.0, t.accent);
    }
    // The work area's numbers in the header column.
    let label = format!("{} {}–{}", tl!("Work"), tl.number(work.start), tl.number(work.end - 1));
    ui.painter_at(Rect::from_min_max(row.min, pos2(geo.left - 4.0, row.max.y))).text(
        pos2(row.left() + 4.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::monospace(9.5),
        t.text_dim,
    );
    let id = ui.scope_id().with("timeline-work-area");
    let r = ui.interact(area, id, Sense::click_and_drag()).on_hover_text(tl!("Work area: drag either end; double-click to reset it"));
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, tl!("Work area")));
    if r.double_clicked() {
        out.run.push(("timeline.resetWorkArea", json!({})));
        return;
    }
    if r.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let end = (p.x - x1).abs() < (p.x - x0).abs();
        ui.data_mut(|d| d.insert_temp(id, end));
    }
    if r.dragged()
        && let Some(p) = r.interact_pointer_pos()
    {
        let end = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let b = geo.boundary_at(p.x);
        if end {
            let we = b.max(work.start + 1);
            if we != work.end {
                out.run.push(("timeline.setProps", json!({ "workEnd": we })));
            }
        } else {
            let ws = b.min(work.end - 1);
            if ws != work.start {
                out.run.push(("timeline.setProps", json!({ "workStart": ws })));
            }
        }
    }
}

/// One row per video layer, scrolling when there are more than fit.
#[allow(clippy::too_many_arguments)]
fn track_rows(ui: &mut egui::Ui, v: &View, geo: &Geo, tl: &Timeline, out: &mut Out, t: &Tokens, full: f32, height: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(full, height), Sense::hover());
    ui.painter().rect_filled(geo.area(rect.y_range()), 0.0, t.canvas);
    if v.tracks.is_empty() {
        ui.painter_at(rect).text(
            pos2(rect.left() + 6.0, rect.top() + ROW_H / 2.0),
            Align2::LEFT_CENTER,
            tl!("No video layers: load footage, or Layer › Video Layers › New Blank Video Layer"),
            FontId::proportional(11.5),
            t.text_faint,
        );
        return rect;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical().id_salt("timeline-tracks").auto_shrink([false, false]).max_height(height).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (track, thumb, active) in &v.tracks {
                track_row(ui, track, *thumb, *active, geo, tl, out, t);
            }
        });
    });
    rect
}

#[allow(clippy::too_many_arguments)]
fn track_row(ui: &mut egui::Ui, track: &Track, thumb: Option<egui::TextureId>, active: bool, geo: &Geo, tl: &Timeline, out: &mut Out, t: &Tokens) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let head = Rect::from_min_max(row.min, pos2(geo.left - 4.0, row.max.y));
    let id = ui.scope_id().with(("timeline-track", track.layer));
    let painter = ui.painter_at(head);
    if active {
        painter.rect_filled(head, 0.0, t.accent_soft);
    }
    ui.painter_at(row).line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, t.separator));
    // Eye.
    let eye = Rect::from_center_size(pos2(head.left() + 13.0, row.center().y), vec2(20.0, 20.0));
    crate::icons::paint(ui, eye, if track.visible { "eye" } else { "eye-off" }, 14.0, if track.visible { t.icon } else { t.text_faint });
    let eye_tip = if track.visible { tl!("Hide Layer") } else { tl!("Show Layer") };
    let eye_r = ui.interact(eye, id.with("eye"), Sense::click()).on_hover_text(eye_tip);
    eye_r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{eye_tip}: {}", track.name)));
    if eye_r.clicked() {
        out.run.push(("layer.setProps", json!({ "layer": track.layer, "visible": !track.visible })));
    }
    // Thumbnail and name; a click selects the layer.
    let th = Rect::from_min_size(pos2(eye.right() + 3.0, row.center().y - 13.0), vec2(26.0, 26.0));
    if let Some(tex) = thumb {
        painter.image(tex, th, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(th, 2.0, Stroke::new(1.0, t.card_border), StrokeKind::Inside);
    painter.text(pos2(th.right() + 6.0, row.center().y), Align2::LEFT_CENTER, &track.name, FontId::proportional(12.0), if active { t.text } else { t.text_dim });
    let name = ui.interact(Rect::from_min_max(pos2(eye.right(), row.top()), head.max), id.with("head"), Sense::click()).on_hover_text(track.name.as_str());
    name.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, track.name.clone()));
    if name.clicked() {
        out.run.push(("layer.select", json!({ "layer": track.layer })));
    }
    // The clip: the layer's frames, with a tick on each painted one.
    let lane = geo.area(row.y_range());
    let clip_painter = ui.painter_at(lane);
    let clip = Rect::from_x_y_ranges(geo.x(0.0)..=geo.x(track.frames as f32), row.y_range().shrink(6.0));
    clip_painter.rect_filled(clip, 3.0, if track.visible { CLIP } else { CLIP.gamma_multiply(0.45) });
    if active {
        clip_painter.rect_stroke(clip, 3.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    for &f in &track.altered {
        let x = geo.x(f as f32 + 0.5);
        if (lane.left() - 2.0..=lane.right() + 2.0).contains(&x) {
            clip_painter.line_segment([pos2(x, clip.top() + 2.0), pos2(x, clip.center().y + 2.0)], Stroke::new(2.0, t.warning));
        }
    }
    // Click selects the layer; right-click a frame for Go to Frame and Restore Frame.
    let lane_r = ui.interact(lane, id.with("clip"), Sense::click());
    lane_r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{}: {}", tl!("Clip"), track.name)));
    if lane_r.clicked() {
        out.run.push(("layer.select", json!({ "layer": track.layer })));
    }
    let menu_frame = id.with("menu-frame");
    if lane_r.secondary_clicked()
        && let Some(p) = lane_r.interact_pointer_pos()
    {
        // The painted frame nearest the pointer within a few points, else the frame under it.
        let reach = geo.ppf.max(8.0) / 2.0;
        let distance = |f: usize| (geo.x(f as f32 + 0.5) - p.x).abs();
        let near = track.altered.iter().copied().filter(|&f| distance(f) <= reach).min_by(|&a, &b| distance(a).total_cmp(&distance(b)));
        let frame = near.unwrap_or_else(|| geo.frame_at(p.x));
        ui.data_mut(|d| d.insert_temp(menu_frame, frame));
    }
    lane_r.context_menu(|ui| {
        let f = ui.data(|d| d.get_temp::<usize>(menu_frame)).unwrap_or(tl.current).min(tl.duration.saturating_sub(1));
        let n = tl.number(f);
        if ui.button(format!("{} {n}", tl!("Go to Frame"))).clicked() {
            out.pause = true;
            out.run.push(("timeline.setFrame", json!({ "frame": f })));
            ui.close();
        }
        let painted = track.altered.contains(&f);
        let restore = ui.add_enabled(painted, egui::Button::new(format!("{} {n}", tl!("Restore Frame")))).on_disabled_hover_text(tl!("Nothing is painted on this frame"));
        if restore.clicked() {
            out.pause = true;
            out.run.push(("layer.select", json!({ "layer": track.layer })));
            out.run.push(("timeline.setFrame", json!({ "frame": f })));
            out.run.push(("layer.videoLayers.restoreFrame", json!({})));
            ui.close();
        }
    });
}

/// Shading outside the work area, and the playhead through the ruler and the tracks.
fn overlays(ui: &egui::Ui, geo: &Geo, tl: &Timeline, top: f32, bottom: f32) {
    let area = Rect::from_x_y_ranges(geo.left..=geo.right(), top..=bottom);
    let painter = ui.painter().with_clip_rect(area);
    let work = tl.work_frames();
    let shade = Color32::from_black_alpha(90);
    let (x0, x1) = (geo.x(work.start as f32), geo.x(work.end as f32));
    let below = (top + RULER_H)..=bottom;
    if x0 > area.left() {
        painter.rect_filled(Rect::from_x_y_ranges(area.left()..=x0, below.clone()), 0.0, shade);
    }
    if x1 < area.right() {
        painter.rect_filled(Rect::from_x_y_ranges(x1..=area.right(), below), 0.0, shade);
    }
    let x = geo.x(tl.current as f32 + 0.5);
    painter.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.5, PLAYHEAD));
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(x - 5.0, top), pos2(x + 5.0, top), pos2(x + 5.0, top + 7.0), pos2(x, top + 12.0), pos2(x - 5.0, top + 7.0)],
        PLAYHEAD,
        Stroke::NONE,
    ));
}

/// ⌘ / Ctrl + wheel over the ruler or the tracks zooms around the pointer; the wheel over the
/// ruler, or a sideways wheel (⇧ + wheel) over the tracks, scrolls the frames.
fn wheel(ui: &egui::Ui, geo: &Geo, out: &mut Out, rect: Rect, ruler: Rect) {
    let lanes = Rect::from_x_y_ranges(geo.left..=geo.right(), rect.y_range());
    let Some(p) = ui.ctx().pointer_hover_pos() else { return };
    if !lanes.contains(p) || !ui.rect_contains_pointer(lanes) {
        return;
    }
    let (zoom, delta) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
    if (zoom - 1.0).abs() > 1e-4 {
        let (z, scroll) = geo.zoomed(geo.ppf * zoom, (p.x - geo.left) / geo.ppf + geo.scroll);
        out.zoom = Some(z);
        out.scroll = Some(scroll);
        return;
    }
    let dx = delta.x + if ruler.contains(p) { delta.y } else { 0.0 };
    if dx != 0.0 && geo.max_scroll() > 0.0 {
        out.zoom = Some(Some(geo.ppf));
        out.scroll = Some((geo.scroll - dx / geo.ppf).clamp(0.0, geo.max_scroll()));
    }
}

fn footer(ui: &mut egui::Ui, v: &View, geo: &Geo, tl: &Timeline, out: &mut Out) {
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.set_min_height(FOOTER_H - 4.0);
        let anchor = geo.zoom_anchor(tl.current);
        if crate::icons::button(ui, "minus", 20.0, false, tl!("Zoom Out")).clicked() {
            let (z, s) = geo.zoomed(geo.ppf / 1.5, anchor);
            (out.zoom, out.scroll) = (Some(z), Some(s));
        }
        let (lo, hi) = (geo.fit().clamp(1e-3, MAX_ZOOM).ln(), MAX_ZOOM.max(geo.fit()).ln());
        let mut level = geo.ppf.max(1e-3).ln();
        if ui.add(egui::Slider::new(&mut level, lo..=hi).show_value(false)).on_hover_text(tl!("Zoom")).changed() {
            let (z, s) = geo.zoomed(level.exp(), anchor);
            (out.zoom, out.scroll) = (Some(z), Some(s));
        }
        if crate::icons::button(ui, "plus", 20.0, false, tl!("Zoom In")).clicked() {
            let (z, s) = geo.zoomed(geo.ppf * 1.5, anchor);
            (out.zoom, out.scroll) = (Some(z), Some(s));
        }
        if crate::widgets::secondary_button(ui, tl!("Fit"), 0.0).on_hover_text(tl!("Show every frame")).clicked() {
            out.zoom = Some(None);
            out.scroll = Some(0.0);
        }
        let max = geo.max_scroll();
        if max > 0.0 {
            let mut s = geo.scroll;
            if ui.add(egui::Slider::new(&mut s, 0.0..=max).show_value(false)).on_hover_text(tl!("Scroll the timeline")).changed() {
                out.zoom = Some(Some(geo.ppf));
                out.scroll = Some(s);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::widgets::primary_button(ui, tl!("Render Video…"), 120.0).clicked() {
                out.pause = true;
                out.render = true;
            }
            let mut format = v.state.render_format;
            crate::widgets::dropdown(ui, "timeline-render-format", tl!("Format"), &mut format, &RenderFormat::OPTIONS, 96.0);
            if format != v.state.render_format {
                out.render_format = Some(format);
            }
            let mut range = v.state.render_range;
            crate::widgets::dropdown(ui, "timeline-render-range", tl!("Range"), &mut range, &RenderRange::OPTIONS, 110.0);
            if range != v.state.render_range {
                out.render_range = Some(range);
            }
        });
    });
}

#[cfg(test)]
mod tests;
