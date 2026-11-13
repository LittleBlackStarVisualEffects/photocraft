//! Window › Timeline: a floating panel that creates and drives the document timeline. All actions
//! run the engine `timeline.*` commands, so the control channel drives it the same way. Its
//! Render Video… button runs File › Export › Render Video… (`file.export.renderVideo`) in the
//! format chosen next to it.

use egui::{RichText, vec2};
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

/// Timeline panel view state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
}

/// Menu checkmark for Window › Timeline.
pub fn checked(app: &PhotocraftApp, id: &str) -> Option<bool> {
    (id == "window.panel.timeline").then_some(app.ui.timeline.open)
}

/// Ids this module owns (for parity / enablement).
pub fn handles(id: &str) -> bool {
    id == "window.panel.timeline"
}

/// Toggle the Timeline panel.
pub fn menu(app: &mut PhotocraftApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    if id == "window.panel.timeline" {
        app.ui.timeline.open = !app.ui.timeline.open;
        return Some(Ok(json!({ "open": app.ui.timeline.open })));
    }
    None
}

/// The panel's playback state with the engine's `timeline.info`: `{playing, open, renderFormat,
/// timeline}` (`timeline` is null without a document timeline).
pub fn state(app: &PhotocraftApp) -> Value {
    let timeline = app.session.active().and_then(|d| d.doc.timeline.as_ref()).map(|t| {
        json!({"fps": t.fps, "duration": t.duration, "current": t.current, "workStart": t.work_start, "workEnd": t.work_end, "time": t.time()})
    });
    json!({
        "playing": app.ui.timeline.playing,
        "open": app.ui.timeline.open,
        "renderFormat": app.ui.timeline.render_format,
        "timeline": timeline,
    })
}

/// The control method `timeline.play`: the Timeline panel's Play. Opens the panel, whose frame
/// loop moves the playhead, and starts from the current frame.
pub fn play(app: &mut PhotocraftApp) -> Result<Value, String> {
    let document = app.session.active().ok_or("no document")?;
    if document.doc.timeline.is_none() {
        return Err("the document has no timeline (Create Video Timeline, or load footage)".into());
    }
    app.ui.timeline.open = true;
    app.ui.timeline.playing = true;
    app.ui.timeline.play_anchor = None;
    Ok(state(app))
}

/// The control method `timeline.stop {rewind?}`: the Timeline panel's Stop. Pauses at the current
/// frame; `rewind: true` also goes back to the first frame, as a second Stop does.
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

/// The frame playback shows at `now`: counted at the timeline's rate from where playback started,
/// or from where the playhead was moved to since, wrapping at the end. A frame that takes longer to
/// show than the rate allows (a large float plate) makes playback skip frames, not slow down.
fn playback_frame(ui: &mut TimelineUi, t: &photocraft_doc::Timeline, now: f64) -> usize {
    let (start, from) = match ui.play_anchor {
        Some(anchor) if ui.play_frame == Some(t.current) => anchor,
        // Just started, or Next frame, a command or the channel moved the playhead.
        _ => {
            let anchor = (now, t.current);
            ui.play_anchor = Some(anchor);
            anchor
        }
    };
    let elapsed = now - start;
    let elapsed = if elapsed.is_finite() { elapsed.max(0.0) } else { 0.0 };
    // `as` saturates, so a clock running for days cannot wrap.
    let advanced = (elapsed * f64::from(t.fps.max(1.0))).floor() as usize;
    let next = from.saturating_add(advanced) % t.duration.max(1);
    ui.play_frame = Some(next);
    next
}

/// Render the Timeline panel.
pub fn windows(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.timeline.open {
        return;
    }
    let tl = app.session.active().and_then(|d| d.doc.timeline.clone());
    let have_doc = app.session.active().is_some();
    let mut act: Option<(&str, Value)> = None;
    let mut close = false;
    let mut toggle_play = false;
    let mut stop_play = false;
    let mut render = false;
    let mut render_format = app.ui.timeline.render_format;
    let playing = app.ui.timeline.playing;
    let t = Tokens::get(ctx);

    crate::analysis_ui::panel_window(app, ctx, "timeline", tl!("Timeline"), vec2(0.0, 520.0), 660.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("Timeline")).strong().color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::icons::button(ui, "x", 20.0, false, tl!("Close")).clicked() {
                    close = true;
                }
            });
        });
        ui.separator();

        if !have_doc {
            ui.weak(tl!("Open a document to use the timeline."));
            return;
        }
        match &tl {
            None => {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if crate::widgets::primary_button(ui, tl!("Create Video Timeline"), 200.0).clicked() {
                        act = Some(("timeline.create", json!({ "duration": 30, "fps": 30.0 })));
                    }
                    ui.add_space(4.0);
                    ui.weak(tl!("30 frames @ 30 fps"));
                });
            }
            Some(tl) => {
                ui.horizontal(|ui| {
                    let play_icon = if playing { "pause" } else { "play" };
                    if crate::icons::button(ui, play_icon, 22.0, playing, if playing { tl!("Pause") } else { tl!("Play") }).clicked() {
                        toggle_play = true;
                    }
                    if crate::icons::button(ui, "square", 22.0, false, tl!("Stop")).clicked() {
                        if !playing {
                            act = Some(("timeline.setFrame", json!({ "frame": 0 })));
                        }
                        stop_play = true;
                    }
                    if crate::icons::button(ui, "chevron-left", 22.0, false, tl!("Previous frame")).clicked() {
                        stop_play = true;
                        act = Some(("timeline.previousFrame", json!({})));
                    }
                    ui.label(
                        RichText::new(crate::i18n::fmt(
                            crate::i18n::t("Frame {current} / {total}"),
                            &[("current", &(tl.current + 1).to_string()), ("total", &tl.duration.to_string())],
                        ))
                        .color(t.text)
                        .monospace(),
                    );
                    if crate::icons::button(ui, "chevron-right", 22.0, false, tl!("Next frame")).clicked() {
                        stop_play = true;
                        act = Some(("timeline.nextFrame", json!({})));
                    }
                    ui.separator();
                    ui.label(RichText::new(format!("{:.2}s", tl.time())).color(t.text_dim).monospace());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::icons::button(ui, "trash", 20.0, false, tl!("Delete Timeline")).clicked() {
                            stop_play = true;
                            act = Some(("timeline.delete", json!({})));
                        }
                        ui.label(RichText::new(format!("{:.0} fps", tl.fps)).color(t.text_dim));
                    });
                });
                ui.add_space(6.0);
                // Scrubber: frame playhead across the duration.
                if tl.duration > 1 {
                    let mut frame = tl.current as f64;
                    let resp = ui.add(egui::Slider::new(&mut frame, 0.0..=(tl.duration - 1) as f64).integer().show_value(false).text("playhead"));
                    if resp.changed() {
                        stop_play = true;
                        act = Some(("timeline.setFrame", json!({ "frame": frame as u64 })));
                    }
                }
                ui.add_space(6.0);
                // Render: every frame of the timeline, as File › Export › Render Video… does.
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tl!("Format")).color(t.text_dim));
                    crate::widgets::dropdown(ui, "timeline-render-format", tl!("Format"), &mut render_format, &RenderFormat::OPTIONS, 100.0);
                    if crate::widgets::primary_button(ui, tl!("Render Video…"), 120.0).clicked() {
                        stop_play = true;
                        render = true;
                    }
                });
            }
        }
    });

    app.ui.timeline.render_format = render_format;
    if close {
        app.ui.timeline.open = false;
        app.ui.timeline.playing = false;
    }
    if stop_play {
        app.ui.timeline.playing = false;
    } else if toggle_play {
        app.ui.timeline.playing = !app.ui.timeline.playing;
        app.ui.timeline.play_anchor = None;
    }
    if let Some((cmd, p)) = act {
        let _ = app.run(cmd, p);
    }
    // The menu command asks for the output folder first; nothing is written if that is cancelled.
    if render && let Err(e) = crate::menus::invoke(app, ctx, "file.export.renderVideo", json!({ "format": render_format.ext() })) {
        app.ui.status = e;
        app.ui.status_error = true;
    }
    // Playback: the playhead follows the clock at the frame rate and loops (`playback_frame`).
    if app.ui.timeline.playing {
        if let Some(t) = app.session.active().and_then(|d| d.doc.timeline.clone()) {
            let now = ctx.input(|i| i.time);
            let next = playback_frame(&mut app.ui.timeline, &t, now);
            if next != t.current
                && let Err(e) = app.run("timeline.setFrame", json!({ "frame": next as u64 }))
            {
                // Say why instead of playing silently on one frame.
                app.ui.timeline.playing = false;
                app.ui.status = e;
                app.ui.status_error = true;
            }
            ctx.request_repaint();
        } else {
            app.ui.timeline.playing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn stop_step_and_scrub_pause_playback_and_second_stop_rewinds() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 100, "height": 80})).unwrap();
        app.run("timeline.create", json!({"duration": 10, "fps": 30.0})).unwrap();
        app.ui.timeline.open = true;
        app.last_canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0));

        let mut h = Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.001).build_ui_state(|ui, app| windows(app, ui.ctx()), app);
        h.run_steps(2);

        // Start playback via Play button.
        h.get_by_label("Play").click();
        h.run_steps(2);
        assert!(h.state().ui.timeline.playing);

        // Move to frame 5 while playing; first Stop pauses at the current frame without rewinding.
        h.state_mut().run("timeline.setFrame", json!({"frame": 5})).unwrap();
        h.get_by_label("Stop").click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "first Stop pauses playback");
        assert_eq!(h.state().session.active().unwrap().doc.timeline.as_ref().unwrap().current, 5);

        // Second Stop while already paused rewinds to frame 0.
        h.get_by_label("Stop").click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing);
        assert_eq!(h.state().session.active().unwrap().doc.timeline.as_ref().unwrap().current, 0);

        // Next frame and Previous frame pause playback.
        h.state_mut().ui.timeline.playing = true;
        h.run_steps(1);
        h.get_by_label("Next frame").click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "Next frame pauses playback");
        assert_eq!(h.state().session.active().unwrap().doc.timeline.as_ref().unwrap().current, 1);

        h.state_mut().ui.timeline.playing = true;
        h.run_steps(1);
        h.get_by_label("Previous frame").click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "Previous frame pauses playback");
        assert_eq!(h.state().session.active().unwrap().doc.timeline.as_ref().unwrap().current, 0);

        // Scrubbing the playhead slider pauses playback and updates the current frame.
        h.state_mut().ui.timeline.playing = true;
        h.run_steps(1);
        let slider = h.get_by_role(egui::accesskit::Role::Slider).rect();
        let target = egui::pos2(slider.right() - 8.0, slider.center().y);
        h.event(egui::Event::PointerMoved(target));
        h.event(egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.step();
        h.event(egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "scrubbing pauses playback");
        assert!(h.state().session.active().unwrap().doc.timeline.as_ref().unwrap().current > 0);
    }

    #[test]
    fn playback_keeps_time_by_skipping_frames_and_follows_a_moved_playhead() {
        let mut ui = TimelineUi::default();
        let mut t = photocraft_doc::Timeline::new(10, 10.0);
        let mut at = |t: &mut photocraft_doc::Timeline, now: f64| {
            let f = playback_frame(&mut ui, t, now);
            t.current = f;
            f
        };
        assert_eq!(at(&mut t, 100.0), 0, "playback starts on the current frame");
        assert_eq!(at(&mut t, 100.05), 0);
        // A slow frame (a large plate) is followed by the frame due now, not the next one.
        assert_eq!(at(&mut t, 100.35), 3);
        assert_eq!(at(&mut t, 100.95), 9);
        assert_eq!(at(&mut t, 101.05), 0, "playback loops");
        // Something else moved the playhead: playback carries on from there.
        t.current = 6;
        assert_eq!(at(&mut t, 101.06), 6);
        assert_eq!(at(&mut t, 101.27), 8);
        // A clock that jumps back or a broken rate never panics.
        assert!(at(&mut t, f64::NAN) < 10);
        assert!(at(&mut t, 0.0) < 10);
    }

    #[test]
    fn play_and_stop_control_methods_drive_the_panel() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        assert!(play(&mut app).is_err(), "no document");
        app.run("file.new", json!({"width": 10, "height": 10})).unwrap();
        assert!(play(&mut app).unwrap_err().contains("no timeline"));
        app.run("timeline.create", json!({"duration": 8, "fps": 24.0})).unwrap();
        app.run("timeline.setFrame", json!({"frame": 4})).unwrap();

        let r = play(&mut app).unwrap();
        assert_eq!((r["playing"].as_bool(), r["open"].as_bool()), (Some(true), Some(true)), "{r}");
        assert_eq!(r["timeline"]["duration"], 8);
        assert_eq!(state(&app)["playing"], true);

        let r = stop(&mut app, &json!({})).unwrap();
        assert_eq!(r["playing"], false);
        assert_eq!(r["timeline"]["current"], 4, "Stop pauses where playback is");
        assert!(stop(&mut app, &json!({"rewind": "yes"})).is_err());
        let r = stop(&mut app, &json!({"rewind": true})).unwrap();
        assert_eq!(r["timeline"]["current"], 0, "rewind goes back to the first frame");
        assert_eq!(state(&app)["renderFormat"], "png");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn render_button_asks_for_a_folder_then_writes_every_frame_in_the_chosen_format() {
        use crate::file_dialog::{FileDialogAnswer, FileDialogReply, FileDialogRequest};
        use std::cell::RefCell;
        use std::rc::Rc;

        let open: Rc<RefCell<Vec<(FileDialogRequest, FileDialogReply)>>> = Rc::default();
        let held = open.clone();
        let services = crate::Services { file_dialog: Some(Box::new(move |request, _parent, reply| held.borrow_mut().push((request, reply)))), ..Default::default() };
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
        app.run("file.new", json!({"width": 8, "height": 6})).unwrap();
        app.run("timeline.create", json!({"duration": 3, "fps": 24.0})).unwrap();
        app.ui.timeline.open = true;
        app.ui.timeline.playing = true;
        app.ui.timeline.render_format = RenderFormat::Jpg;
        app.last_canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 700.0));

        let mut h = Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.001).build_ui_state(|ui, app| windows(app, ui.ctx()), app);
        h.run_steps(2);
        h.get_by_label("Render Video…").click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "rendering stops playback");
        assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
        assert!(h.state().file_dialog_open(), "the output folder is asked for first");

        let dir = std::env::temp_dir().join(format!("pc-timeline-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = egui::Context::default();
        h.state_mut().poll_file_dialog(&ctx, None);
        let (request, reply) = open.borrow_mut().pop().expect("a folder dialog is open");
        assert!(matches!(request, FileDialogRequest::Folder { .. }), "{request:?}");
        let chosen = dir.to_string_lossy().into_owned();
        std::thread::spawn(move || reply.send(Some(FileDialogAnswer::FolderTo(chosen)))).join().unwrap();
        h.state_mut().poll_file_dialog(&ctx, None);

        let mut frames: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        frames.sort();
        assert_eq!(frames.len(), 3, "one file per timeline frame: {frames:?}");
        assert!(frames.iter().all(|f| f.ends_with(".jpg")), "{frames:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
