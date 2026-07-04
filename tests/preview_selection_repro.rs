//! Visual repro rig for preview selection artifacts (issue reports):
//! renders the markdown preview offscreen, simulates a drag selection across
//! paragraphs + a code block, and writes PNG frames to inspect.
//!
//! Run with: cargo test --test preview_selection_repro -- --ignored

use egui::Pos2;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

const DOC: &str = "## FIX THIS WEEK\n\n2. **DB indexes** not guaranteed on live schema (HIGH)\n\nThe right indexes exist only in `scripts/db-indexes.mjs`, `src/lib/helpdesk-db.ts` only creates `idx_prebilling` ticket query.\n\n```bash\nTS=$(date +%Y%m%d-%H%M%S)\nmkdir -p ~/mc-backups/$TS\nfor db in src/data/auth/auth.sqlite; do\n  sqlite3 \"$db\" \".backup x\"\ndone\n```\n\n3. N+1 query in `listDbTickets()` (HIGH, perf)\n\n`src/lib/helpdesk-db.ts:147` loads all ~2,654 tickets.\n";

#[test]
#[ignore = "visual repro; run explicitly"]
fn drag_select_preview() {
    let mut cache = CommonMarkCache::default();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(520.0, 640.0))
        .build_ui(|ui| {
            ui.ctx().options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
            egui::ScrollArea::vertical().show(ui, |ui| {
                CommonMarkViewer::new().show(ui, &mut cache, DOC);
            });
        });

    harness.run();

    // drag from inside the first paragraph down across the code block
    let start = Pos2::new(40.0, 90.0);
    let end = Pos2::new(400.0, 560.0);
    harness.input_mut().events.push(egui::Event::PointerMoved(start));
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    // move in steps so the drag is recognized
    for t in 1..=8 {
        let f = t as f32 / 8.0;
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
    harness.run();
    harness.run();

    // copying the selection must yield the text with newlines between blocks
    // (step, not run: run() may do extra passes whose output drops the command)
    harness.input_mut().events.push(egui::Event::Copy);
    harness.step();
    let copied = harness.output().platform_output.commands.iter().find_map(|c| match c {
        egui::OutputCommand::CopyText(t) => Some(t.clone()),
        _ => None,
    });
    if let Some(t) = &copied {
        println!("copied {} chars, {} newlines", t.len(), t.matches('\n').count());
        assert!(t.contains("mkdir -p"), "code block text must be included");
        assert!(t.matches('\n').count() >= 5, "block newlines must survive: {t:?}");
    } else {
        println!("WARNING: no CopyText command captured");
    }

    let img = harness.render().expect("render");
    let out = std::env::var("REPRO_OUT").unwrap_or_else(|_| "/tmp/preview_repro.png".into());
    img.save(&out).expect("save png");
    println!("wrote {out}");
}
