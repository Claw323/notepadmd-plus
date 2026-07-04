//! Visual repro for split-view selection mirroring: select text in the editor
//! and check the matching rendered text glows in the preview.
//!
//! Run: cargo test --test mirror_repro -- --ignored --nocapture

use egui::text::{CCursor, CCursorRange};

const DOC: &str = "## FIX THIS WEEK\n\n2. **DB indexes** not guaranteed on live schema (HIGH)\n\nThe right indexes exist only in `scripts/db-indexes.mjs` and only creates the ticket query.\n\n```bash\nmkdir -p ~/mc-backups/$TS\n```\n\n3. N+1 query in `listDbTickets()` (HIGH, perf)\n";

#[test]
#[ignore = "visual repro; run explicitly"]
fn editor_selection_mirrors_to_preview() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 640.0))
        .build_eframe(|cc| {
            cc.egui_ctx
                .options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
            let mut app = notepadmd_plus::app::App::new(cc);
            app.debug_setup(DOC, true);
            app
        });

    harness.run();

    // select "indexes exist only in scripts" in the editor (char positions in DOC)
    let needle = "indexes exist only in `scripts/db-indexes.mjs`";
    let start_b = DOC.find(needle).unwrap();
    let start = DOC[..start_b].chars().count();
    let end = start + needle.chars().count();

    let editor_id = harness.state().debug_editor_id();
    let ctx = harness.ctx.clone();
    let mut st = egui::text_edit::TextEditState::load(&ctx, editor_id).unwrap_or_default();
    st.cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(start), CCursor::new(end))));
    st.store(&ctx, editor_id);
    ctx.memory_mut(|m| m.request_focus(editor_id));

    for _ in 0..4 {
        harness.run();
    }

    let img = harness.render().expect("render");
    let out = std::env::var("REPRO_OUT").unwrap_or_else(|_| "/tmp/mirror_repro.png".into());
    img.save(&out).expect("save png");
    println!("wrote {out}");
}

#[test]
#[ignore = "visual repro; run explicitly"]
fn preview_selection_mirrors_to_editor() {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1000.0, 640.0))
        .build_eframe(|cc| {
            cc.egui_ctx
                .options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
            let mut app = notepadmd_plus::app::App::new(cc);
            app.debug_setup(DOC, true);
            app
        });
    harness.run();

    // real-world precondition: the user has been working in the editor, so it
    // holds keyboard focus before they drag on the preview
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::Pos2::new(200.0, 120.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
    let editor_id = harness.state().debug_editor_id();
    assert!(
        harness.ctx.memory(|m| m.has_focus(editor_id)),
        "precondition: editor must be focused like in real use"
    );

    // drag-select on the preview (right pane) across the paragraph
    let start = egui::Pos2::new(530.0, 160.0);
    let end = egui::Pos2::new(950.0, 175.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(start));
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    for t in 1..=6 {
        let f = t as f32 / 6.0;
        let p = start + (end - start) * f;
        harness.input_mut().events.push(egui::Event::PointerMoved(p));
        harness.run();
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..4 {
        harness.run();
    }

    // show the F12 diagnostics overlay in the saved screenshot
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::F12,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    harness.run();

    let mirrored = harness.state().debug_editor_mirror();
    println!("editor mirror range: {mirrored:?}");
    if let Some((a, b)) = mirrored {
        let text: String = DOC.chars().skip(a).take(b - a).collect();
        println!("mirrored source: {text:?}");
        assert!(text.contains("indexes"), "mirror should cover the selected paragraph: {text:?}");
    }

    let img = harness.render().expect("render");
    let out = std::env::var("REPRO_OUT2").unwrap_or_else(|_| "/tmp/mirror_repro2.png".into());
    img.save(&out).expect("save png");
    println!("wrote {out}");
    assert!(mirrored.is_some(), "preview selection should mirror into the editor");
}

#[test]
#[ignore = "visual repro; run explicitly"]
fn line_numbers_continue_past_first_screen() {
    let doc: String = (1..=200).map(|i| format!("line number {i}\n")).collect();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(700.0, 500.0))
        .build_eframe(|cc| {
            let mut app = notepadmd_plus::app::App::new(cc);
            app.debug_setup(&doc, false);
            app
        });
    harness.run();

    // wheel-scroll deep into the document with the pointer over the editor
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::Pos2::new(300.0, 250.0)));
    for _ in 0..30 {
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -8.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
    }
    let img = harness.render().expect("render");
    let out = std::env::var("REPRO_OUT3").unwrap_or_else(|_| "/tmp/line_numbers.png".into());
    img.save(&out).expect("save png");
    println!("wrote {out}");
}
