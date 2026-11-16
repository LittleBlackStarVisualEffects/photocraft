use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::{Harness, kittest::Queryable};

fn app_with_timeline(duration: u64) -> PhotocraftApp {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"width": 100, "height": 80})).unwrap();
    app.run("timeline.create", json!({"duration": duration, "fps": 24.0})).unwrap();
    app.ui.timeline.open = true;
    app.last_canvas_rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));
    app
}

/// The panel as the app draws it: the timeline keys, the docked panel (or the floating one) and
/// playback.
fn harness(app: PhotocraftApp) -> Harness<'static, PhotocraftApp> {
    let mut h = Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.001).build_ui_state(
        |ui, app: &mut PhotocraftApp| {
            let ctx = ui.ctx().clone();
            // The first frame comes before `setup_context` installs the fonts.
            if !ctx.fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                return;
            }
            crate::shortcuts::handle(app, &ctx);
            dock(app, ui);
            windows(app, &ctx);
        },
        app,
    );
    // The app's fonts (the panel's labels use its medium face).
    PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
    h.run_steps(3);
    h
}

fn timeline(h: &Harness<'_, PhotocraftApp>) -> Timeline {
    h.state().session.active().unwrap().doc.timeline.clone().unwrap()
}

fn press(h: &mut Harness<'_, PhotocraftApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

fn click_at(h: &mut Harness<'_, PhotocraftApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    press(h, p, true);
    h.step();
    press(h, p, false);
    h.run_steps(2);
}

fn drag(h: &mut Harness<'_, PhotocraftApp>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    press(h, from, true);
    h.step();
    for k in 1..=6 {
        h.event(Event::PointerMoved(from.lerp(to, k as f32 / 6.0)));
        h.step();
    }
    press(h, to, false);
    h.run_steps(2);
}

/// The docked panel keeps its height (it once grew a little every frame until it filled the
/// window), and with one track it leaves most of the window to the canvas.
#[test]
fn the_docked_panel_keeps_its_height() {
    let mut h = harness(app_with_timeline(28));
    let top = h.get_by_label("Timeline ruler").rect().top();
    h.run_steps(40);
    assert_eq!(h.get_by_label("Timeline ruler").rect().top(), top, "the panel's height is stable");
    assert!(top > 450.0, "a one-track panel is short: its ruler starts at {top} of 700");
}

#[test]
fn transport_buttons_step_play_pause_and_loop() {
    let mut h = harness(app_with_timeline(10));
    h.state_mut().run("timeline.setFrame", json!({"frame": 4})).unwrap();
    h.run_steps(1);
    for (button, frame) in [("Next Frame", 5), ("Previous Frame", 4), ("Last Frame", 9), ("First Frame", 0)] {
        h.state_mut().ui.timeline.playing = true;
        h.get_by_label(button).click();
        h.run_steps(2);
        assert!(!h.state().ui.timeline.playing, "{button} pauses playback");
        assert_eq!(timeline(&h).current, frame, "{button}");
    }
    h.get_by_label("Play").click();
    h.run_steps(2);
    assert!(h.state().ui.timeline.playing);
    h.get_by_label("Pause").click();
    h.run_steps(2);
    assert!(!h.state().ui.timeline.playing, "one button plays and pauses");
    assert!(timeline(&h).loop_playback);
    h.get_by_label("Loop Playback").click();
    h.run_steps(2);
    assert!(!timeline(&h).loop_playback);
}

/// Acceptance 1: plate numbers on the ruler and in the frame field, and a click on the ruler
/// moves the playhead to the frame under it.
#[test]
fn the_ruler_counts_from_the_start_number_and_a_click_moves_the_playhead() {
    let mut app = app_with_timeline(28);
    app.run("timeline.setProps", json!({"startNumber": 1001})).unwrap();
    let mut h = harness(app);
    assert!(h.query_by_label("/ 1028").is_some(), "the last frame's number");
    let ruler = h.get_by_label("Timeline ruler").rect();
    // Every frame fits: frame 12 is the 13th of 28 equal cells.
    click_at(&mut h, egui::pos2(ruler.left() + ruler.width() * 12.5 / 28.0, ruler.center().y));
    assert_eq!(timeline(&h).current, 12);
    assert_eq!(timeline(&h).number(12), 1013);
    // Dragging scrubs.
    drag(&mut h, egui::pos2(ruler.left() + ruler.width() * 2.5 / 28.0, ruler.center().y), egui::pos2(ruler.left() + ruler.width() * 20.5 / 28.0, ruler.center().y));
    assert_eq!(timeline(&h).current, 20);
}

/// Acceptance 2: drag the work area's ends; a double-click resets it.
#[test]
fn the_work_area_brackets_drag_and_a_double_click_resets() {
    let mut h = harness(app_with_timeline(28));
    let bar = h.get_by_label("Work area").rect();
    let at = |frame: f32| egui::pos2(bar.left() + bar.width() * frame / 28.0, bar.center().y);
    drag(&mut h, at(0.1), at(8.0));
    drag(&mut h, at(27.9), at(25.0));
    let t = timeline(&h);
    assert_eq!((t.work_start, t.work_end), (8, 25), "1009-1025 on a plate from 1001");
    for _ in 0..2 {
        h.event(Event::PointerMoved(at(14.0)));
        press(&mut h, at(14.0), true);
        h.step();
        press(&mut h, at(14.0), false);
        h.step();
    }
    h.run_steps(2);
    let t = timeline(&h);
    assert_eq!((t.work_start, t.work_end), (0, 28));
}

/// Acceptance 2: playback loops inside the work area, or stops at its end with Loop off.
#[test]
fn playback_keeps_time_by_skipping_frames_and_loops_in_the_work_area() {
    let mut ui = TimelineUi::default();
    let mut t = Timeline::new(10, 10.0);
    let mut at = |t: &mut Timeline, now: f64| {
        let (f, more) = playback_frame(&mut ui, t, now);
        t.current = f;
        (f, more)
    };
    assert_eq!(at(&mut t, 100.0), (0, true), "playback starts on the current frame");
    assert_eq!(at(&mut t, 100.05).0, 0);
    // A slow frame (a large plate) is followed by the frame due now, not the next one.
    assert_eq!(at(&mut t, 100.35).0, 3);
    assert_eq!(at(&mut t, 100.95).0, 9);
    assert_eq!(at(&mut t, 101.05).0, 0, "playback loops");
    // Something else moved the playhead: playback carries on from there.
    t.current = 6;
    assert_eq!(at(&mut t, 101.06).0, 6);
    assert_eq!(at(&mut t, 101.27).0, 8);
    // A clock that jumps back or a broken rate never panics.
    assert!(at(&mut t, f64::NAN).0 < 10);
    assert!(at(&mut t, 0.0).0 < 10);

    // A work area of frames 3-5: playback starts there and loops there.
    t.work_start = 3;
    t.work_end = 6;
    t.current = 0;
    assert_eq!(at(&mut t, 200.0).0, 3, "outside the work area playback starts at its first frame");
    assert_eq!(at(&mut t, 200.25).0, 5);
    assert_eq!(at(&mut t, 200.35).0, 3);
    // Loop off: it stops on the work area's last frame.
    t.loop_playback = false;
    t.current = 4;
    assert_eq!(at(&mut t, 300.0), (4, true));
    assert_eq!(at(&mut t, 300.15), (5, false));
}

/// Acceptance 4: with Enable Timeline Shortcut Keys on, Space plays, the arrows step (⇧ ten),
/// Home and End go to the ends; off, the arrows are the Move tool's again; a Type tool keeps Space.
#[test]
fn timeline_shortcut_keys() {
    let mut app = app_with_timeline(28);
    app.run("timeline.setProps", json!({"startNumber": 1001})).unwrap();
    app.ui.tool = crate::state::Tool::Move;
    let mut h = harness(app);
    let key = |h: &mut Harness<'_, PhotocraftApp>, m: Modifiers, k: egui::Key| {
        h.key_press_modifiers(m, k);
        h.run_steps(2);
        timeline(h).current
    };
    assert_eq!(key(&mut h, Modifiers::NONE, egui::Key::ArrowRight), 1);
    assert_eq!(key(&mut h, Modifiers::SHIFT, egui::Key::ArrowRight), 11);
    assert_eq!(key(&mut h, Modifiers::NONE, egui::Key::ArrowLeft), 10);
    assert_eq!(key(&mut h, Modifiers::SHIFT, egui::Key::ArrowLeft), 0);
    assert_eq!(key(&mut h, Modifiers::NONE, egui::Key::End), 27);
    assert_eq!(key(&mut h, Modifiers::NONE, egui::Key::Home), 0, "Home goes to 1001");
    key(&mut h, Modifiers::NONE, egui::Key::Space);
    assert!(h.state().ui.timeline.playing, "Space plays");
    assert_eq!(crate::hold_keys::held_tool(h.state(), &h.ctx), None, "Space is not the Hand tool then");
    key(&mut h, Modifiers::NONE, egui::Key::Space);
    assert!(!h.state().ui.timeline.playing, "and pauses");

    // Off: the arrows don't move the playhead (the Move tool nudges with them).
    h.state_mut().ui.timeline.shortcut_keys = false;
    assert_eq!(key(&mut h, Modifiers::NONE, egui::Key::ArrowRight), 0);
    key(&mut h, Modifiers::NONE, egui::Key::Space);
    assert!(!h.state().ui.timeline.playing);

    // A Type tool keeps Space for typing.
    h.state_mut().ui.timeline.shortcut_keys = true;
    h.state_mut().ui.tool = crate::state::Tool::Type;
    key(&mut h, Modifiers::NONE, egui::Key::Space);
    assert!(!h.state().ui.timeline.playing);
}

/// Acceptance 3: each video layer has a track; a painted frame gets a tick, and the track's menu
/// restores it.
#[test]
fn tracks_list_video_layers_with_their_painted_frames() {
    let mut app = app_with_timeline(6);
    app.run("layer.videoLayers.newBlankVideoLayer", json!({})).unwrap();
    app.run("timeline.setFrame", json!({"frame": 2})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    let mut h = harness(app);
    let ctx = h.ctx.clone();
    let tracks = tracks(h.state_mut(), &ctx);
    assert_eq!(tracks.len(), 1);
    assert_eq!((tracks[0].frames, tracks[0].altered.clone()), (6, vec![2]));
    assert!(h.query_by_label("Clip: Video Layer 1").is_some());
    // The track's eye hides the layer.
    h.get_by_label("Hide Layer: Video Layer 1").click();
    h.run_steps(2);
    let id = LayerId(tracks[0].layer);
    assert!(!h.state().session.active().unwrap().doc.layer(id).unwrap().visible);
}

#[test]
fn panel_menu_turns_the_shortcut_keys_off_and_floats_the_panel() {
    let mut h = harness(app_with_timeline(10));
    h.get_by_label("Panel menu").click();
    h.run_steps(2);
    h.get_by_label("Enable Timeline Shortcut Keys").click();
    h.run_steps(2);
    assert!(!h.state().ui.timeline.shortcut_keys);
    h.get_by_label("Panel menu").click();
    h.run_steps(2);
    h.get_by_label("Float Panel").click();
    h.run_steps(3);
    assert!(!h.state().ui.timeline.docked);
    // Floating, it still has its controls.
    assert!(h.query_by_label("Timeline ruler").is_some());
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
    assert_eq!(with_playing(&app, json!({"timeline": {"current": 4}}))["timeline"]["playing"], true);

    let r = stop(&mut app, &json!({})).unwrap();
    assert_eq!(r["playing"], false);
    assert_eq!(r["timeline"]["current"], 4, "Stop pauses where playback is");
    assert!(stop(&mut app, &json!({"rewind": "yes"})).is_err());
    let r = stop(&mut app, &json!({"rewind": true})).unwrap();
    assert_eq!(r["timeline"]["current"], 0, "rewind goes back to the first frame");
    assert_eq!((state(&app)["renderFormat"].as_str(), state(&app)["renderRange"].as_str()), (Some("png"), Some("workArea")));

    // Play/Pause is a command too (Keyboard Shortcuts › Timeline).
    let ctx = egui::Context::default();
    crate::menus::invoke(&mut app, &ctx, PLAY_PAUSE, json!({})).unwrap();
    assert!(app.ui.timeline.playing);
    crate::menus::invoke(&mut app, &ctx, PLAY_PAUSE, json!({})).unwrap();
    assert!(!app.ui.timeline.playing);
    // Loop off on the work area's last frame: Play starts it over.
    app.run("timeline.setProps", json!({"loop": false, "workStart": 2, "workEnd": 6})).unwrap();
    app.run("timeline.setFrame", json!({"frame": 5})).unwrap();
    play(&mut app).unwrap();
    assert_eq!(app.session.active().unwrap().doc.timeline.as_ref().unwrap().current, 2);
}

#[test]
fn keyboard_shortcuts_list_the_timeline_commands() {
    let app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
    let items = crate::prefs_ui::shortcut_items(&app);
    for (id, label) in SHORTCUT_COMMANDS {
        let item = items.iter().find(|i| i.0 == id).unwrap_or_else(|| panic!("{id} is not listed"));
        assert_eq!((item.1.as_str(), item.2.as_slice()), (label, ["Timeline".to_string()].as_slice()), "{id}");
        assert_eq!(item.3, None, "{id} has no key of its own");
        assert!(crate::menus::is_live(id), "{id}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn render_button_asks_for_a_folder_then_writes_the_work_area_in_the_chosen_format() {
    use crate::file_dialog::{FileDialogAnswer, FileDialogReply, FileDialogRequest};
    use std::cell::RefCell;
    use std::rc::Rc;

    let open: Rc<RefCell<Vec<(FileDialogRequest, FileDialogReply)>>> = Rc::default();
    let held = open.clone();
    let services = crate::Services { file_dialog: Some(Box::new(move |request, _parent, reply| held.borrow_mut().push((request, reply)))), ..Default::default() };
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
    app.run("file.new", json!({"width": 8, "height": 6})).unwrap();
    app.run("timeline.create", json!({"duration": 5, "fps": 24.0})).unwrap();
    app.run("timeline.setProps", json!({"startNumber": 1001, "workStart": 1, "workEnd": 4})).unwrap();
    app.ui.timeline.open = true;
    app.ui.timeline.playing = true;
    app.ui.timeline.render_format = RenderFormat::Jpg;
    app.last_canvas_rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0));

    let mut h = harness(app);
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
    assert_eq!(frames.len(), 3, "the work area's frames: {frames:?}");
    assert!(frames[0].ends_with("_1002.jpg") && frames[2].ends_with("_1004.jpg"), "{frames:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ruler_labels_stay_readable_at_any_zoom() {
    assert_eq!(label_step(40.0, 40.0), 1);
    assert_eq!(label_step(10.0, 40.0), 5);
    assert_eq!(label_step(1.0, 40.0), 50);
    assert_eq!(label_step(0.001, 40.0), 50_000);
    assert_eq!(minor_step(10, 4.0), 1);
    assert_eq!(minor_step(10, 1.0), 5);
    assert_eq!(minor_step(1, 1.0), 1);
    assert_eq!(fps_text(24.0), "24");
    assert_eq!(fps_text(23.976), "23.976");
    assert_eq!(fps_text(29.97), "29.97");
}
