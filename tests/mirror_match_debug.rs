//! Debug harness for the mirror matcher using the user's real document shape:
//! prints which stripped segments fail to match the preview's flattened text.
//! Run: cargo test --test mirror_match_debug -- --ignored --nocapture

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

const DOC: &str = r#"Microsoft Entra ID, while tab visibility and tab access should be controlled inside the application database.

## Phase plan

### Phase 1 — Authentication foundation
Deliver secure Microsoft Entra ID sign-in for 323mc and local user provisioning.

In scope:
- Entra ID OpenID Connect login flow
- Local `users` table
- Match users by stable Entra object ID (`entra_oid`)
- Create user on first login
- Update user profile fields on later login
- Block inactive local users
- Protected `GET /api/me` endpoint
- Code structure ready for later permission dependencies

Out of scope:
- Admin console
- Roles
- Entra group sync
- Tab permissions
- UI redesign

Acceptance criteria:
- A user can sign in with Microsoft credentials [web:25][web:36]

**After each fix, before deploying:**
4. `npm run build` must pass green.
5. Use `/verify` (or manually drive the affected flow) to confirm.

**Rollback if anything goes wrong:**
- Code: `git checkout main && mcrestart`.
"#;

#[test]
fn segment_matching_against_rendered_stream() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut cache = CommonMarkCache::default();
    let flat_cell: Rc<RefCell<Vec<char>>> = Rc::new(RefCell::new(Vec::new()));
    let flat_in = flat_cell.clone();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(900.0, 2000.0))
        .build_ui(move |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                CommonMarkViewer::new().show(ui, &mut cache, DOC);
            });
            // flatten rendered text shapes in paint order, like the app does
            // (must happen inside the pass; graphics drain at frame end)
            let layer = ui.layer_id();
            let mut flat = flat_in.borrow_mut();
            flat.clear();
            ui.ctx().graphics(|g| {
                if let Some(list) = g.get(layer) {
                    for cs in list.all_entries() {
                        if let egui::epaint::Shape::Text(ts) = &cs.shape {
                            for c in ts.galley.text().chars() {
                                flat.push(c);
                            }
                            flat.push(' ');
                        }
                    }
                }
            });
        });
    harness.run();

    let flat: Vec<char> = flat_cell.borrow().clone();
    let stream: String = flat.iter().collect();
    println!("--- flattened stream ---\n{stream}\n---");

    // the selection: everything from "## Phase plan" to "Acceptance criteria:"
    let sel_start = DOC.find("## Phase plan").unwrap();
    let needle = notepadmd_plus::app::debug_strip_md(&DOC[sel_start..]);
    let mut failures = Vec::new();
    for seg in needle.split('\n') {
        let seg_chars: Vec<char> = seg.trim().chars().collect();
        if seg_chars.len() < 3 {
            continue;
        }
        let found = notepadmd_plus::app::debug_find_tolerant(&flat, &seg_chars, 0);
        println!(
            "{} [{}]",
            if found.is_some() { "MATCH" } else { "FAIL " },
            seg.chars().take(70).collect::<String>()
        );
        if found.is_none() {
            failures.push(seg.to_owned());
        }
    }
    assert!(
        failures.is_empty(),
        "every selected segment must match the rendered stream: {failures:#?}"
    );
}
