//! NotepadMD+ application shell: state, menus, toolbar, editor/preview,
//! file operations, find/replace, preferences and modal dialogs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use egui::text::{CCursor, CCursorRange};
use egui::{Align, Key, KeyboardShortcut, Modifiers};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

use crate::highlight;

const MAX_RECENT: usize = 8;
const DISK_POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Plain,
    Pretty,
    Split,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Plain => "Plain Text",
            Mode::Pretty => "Pretty",
            Mode::Split => "Split",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ThemePref {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum StartupMode {
    Plain,
    Pretty,
    LastUsed,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Prefs {
    theme: ThemePref,
    word_wrap: bool,
    line_numbers: bool,
    startup_mode: StartupMode,
    remember_recent: bool,
    last_mode: Mode,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            word_wrap: true,
            line_numbers: false,
            startup_mode: StartupMode::LastUsed,
            remember_recent: true,
            last_mode: Mode::Plain,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuSide {
    Editor,
    Preview,
}

#[derive(Clone, Copy)]
enum MenuAction {
    Undo,
    Redo,
    Cut,
    EditorCopy,
    Paste,
    Delete,
    SelectAll,
    PreviewCopy,
    CopyAll,
    EditPlain,
}

/// An action deferred behind the "unsaved changes" confirmation.
#[derive(Clone)]
enum Pending {
    New,
    OpenDialog,
    OpenPath(PathBuf),
    Revert,
    Exit,
}

pub struct App {
    text: String,
    path: Option<PathBuf>,
    dirty: bool,
    crlf: bool, // file used \r\n on disk; restore on save
    mode: Mode,
    prefs: Prefs,
    recent: Vec<PathBuf>,
    md_cache: CommonMarkCache,

    // find / replace
    find_open: bool,
    replace_open: bool,
    find_query: String,
    replace_with: String,
    match_case: bool,
    find_status: String,
    focus_find: bool,
    pending_scroll: Option<usize>, // char index to scroll into view

    // dialogs
    confirm: Option<Pending>,
    error: Option<String>,
    show_about: bool,
    show_prefs: bool,
    reload_prompt: bool,
    lossy_offer: Option<(PathBuf, Vec<u8>)>, // invalid-UTF-8 file, offer lossy open
    allow_close: bool,

    // right-click context menus (editor + preview)
    ctx_menu: Option<(egui::Pos2, MenuSide)>,
    ctx_menu_opened: bool, // opened this frame; skip the dismiss check once
    ctx_menu_can_paste: bool,
    pending_context_click: Option<egui::Pos2>, // secondary click captured in raw_input_hook
    // menu rows are hit-tested by hand: clicks inside the menu are swallowed in
    // raw_input_hook so egui's selection plugins never see them (keeps text
    // highlights alive through a menu click, like native Windows menus)
    menu_rect: egui::Rect,
    menu_rows: Vec<(egui::Rect, MenuAction, bool)>,
    pending_menu_click: Option<egui::Pos2>,
    preview_rect: egui::Rect, // last frame's preview area, for hit-testing clicks
    editor_rect: egui::Rect,  // last frame's editor area
    // editor events (undo/cut/paste…) queued by menus, replayed at frame start
    // so the editor — which renders before the menus — actually receives them
    pending_editor_events: Vec<egui::Event>,

    // disk change watching
    disk_mtime: Option<SystemTime>,
    last_disk_check: Instant,

    last_title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_system_fonts(&cc.egui_ctx);

        let (prefs, recent) = match cc.storage {
            Some(s) => (
                eframe::get_value(s, "prefs").unwrap_or_default(),
                eframe::get_value(s, "recent").unwrap_or_default(),
            ),
            None => (Prefs::default(), Vec::new()),
        };
        let prefs: Prefs = prefs;
        let mode = match prefs.startup_mode {
            StartupMode::Plain => Mode::Plain,
            StartupMode::Pretty => Mode::Pretty,
            StartupMode::LastUsed => prefs.last_mode,
        };
        apply_theme(&cc.egui_ctx, prefs.theme);

        let mut app = Self {
            text: String::new(),
            path: None,
            dirty: false,
            crlf: false,
            mode,
            prefs,
            recent,
            md_cache: CommonMarkCache::default(),
            find_open: false,
            replace_open: false,
            find_query: String::new(),
            replace_with: String::new(),
            match_case: false,
            find_status: String::new(),
            focus_find: false,
            pending_scroll: None,
            confirm: None,
            error: None,
            show_about: false,
            show_prefs: false,
            reload_prompt: false,
            lossy_offer: None,
            allow_close: false,
            ctx_menu: None,
            ctx_menu_opened: false,
            ctx_menu_can_paste: false,
            pending_context_click: None,
            menu_rect: egui::Rect::NOTHING,
            menu_rows: Vec::new(),
            pending_menu_click: None,
            preview_rect: egui::Rect::NOTHING,
            editor_rect: egui::Rect::NOTHING,
            pending_editor_events: Vec::new(),
            disk_mtime: None,
            last_disk_check: Instant::now(),
            last_title: String::new(),
        };

        // Opened via "Open with" / command line
        if let Some(arg) = std::env::args().nth(1) {
            let p = PathBuf::from(arg);
            if p.is_file() {
                app.load_path(&p);
            }
        }
        app
    }

    // ---------- file operations ----------

    fn load_path(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => self.set_document(path.to_path_buf(), s),
                Err(e) => self.lossy_offer = Some((path.to_path_buf(), e.into_bytes())),
            },
            Err(e) => self.error = Some(friendly_io_error("Could not open the file", path, &e)),
        }
    }

    fn set_document(&mut self, path: PathBuf, s: String) {
        self.crlf = s.contains("\r\n");
        self.text = if self.crlf { s.replace("\r\n", "\n") } else { s };
        self.disk_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.push_recent(path.clone());
        self.path = Some(path);
        self.dirty = false;
        self.reload_prompt = false;
    }

    fn push_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }

    /// Save to current path (or Save As). Returns true on success.
    fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.write_to(&p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("Text", &["txt"])
            .add_filter("All files", &["*"]);
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                dlg = dlg.set_directory(dir);
            }
            if let Some(name) = p.file_name() {
                dlg = dlg.set_file_name(name.to_string_lossy());
            }
        } else {
            dlg = dlg.set_file_name("untitled.md");
        }
        match dlg.save_file() {
            Some(p) => {
                if self.write_to(&p) {
                    self.push_recent(p.clone());
                    self.path = Some(p);
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    /// Atomic-ish write: temp file in the same directory, then rename over.
    /// The editor buffer is never touched, so a failed save loses nothing.
    fn write_to(&mut self, path: &Path) -> bool {
        let data = if self.crlf { self.text.replace('\n', "\r\n") } else { self.text.clone() };
        let tmp = path.with_extension("nmdp-tmp~");
        let result = std::fs::write(&tmp, data.as_bytes()).and_then(|_| std::fs::rename(&tmp, path));
        match result {
            Ok(()) => {
                self.dirty = false;
                self.disk_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                true
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                self.error = Some(friendly_io_error(
                    "Could not save the file (your text is still in the editor)",
                    path,
                    &e,
                ));
                false
            }
        }
    }

    /// Run `action`, or park it behind the unsaved-changes prompt.
    fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if self.dirty {
            self.confirm = Some(action);
        } else {
            self.proceed(action, ctx);
        }
    }

    fn proceed(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::New => {
                self.text.clear();
                self.path = None;
                self.dirty = false;
                self.crlf = false;
                self.disk_mtime = None;
            }
            Pending::OpenDialog => {
                let picked = rfd::FileDialog::new()
                    .add_filter("Markdown / Text", &["md", "markdown", "txt"])
                    .add_filter("All files", &["*"])
                    .pick_file();
                if let Some(p) = picked {
                    self.load_path(&p);
                }
            }
            Pending::OpenPath(p) => self.load_path(&p),
            Pending::Revert => {
                if let Some(p) = self.path.clone() {
                    self.load_path(&p);
                }
            }
            Pending::Exit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    // ---------- find / replace ----------

    /// Case-aware substring search over chars; returns (char_start, char_end).
    /// ASCII-case-insensitive when match_case is off (byte-offset safe).
    fn find_in(text: &str, query: &str, from_char: usize, forward: bool, match_case: bool) -> Option<(usize, usize)> {
        if query.is_empty() {
            return None;
        }
        let hay: Vec<char> = text.chars().collect();
        let needle: Vec<char> = query.chars().collect();
        if needle.len() > hay.len() {
            return None;
        }
        let eq = |a: char, b: char| {
            if match_case { a == b } else { a.eq_ignore_ascii_case(&b) }
        };
        let matches_at = |i: usize| hay[i..].len() >= needle.len() && needle.iter().enumerate().all(|(j, &c)| eq(hay[i + j], c));
        let last = hay.len() - needle.len();
        if forward {
            (from_char..=last).chain(0..from_char.min(last + 1)).find(|&i| matches_at(i))
        } else {
            let start = from_char.min(last + 1);
            (0..start).rev().chain((start..=last).rev()).find(|&i| matches_at(i))
        }
        .map(|i| (i, i + needle.len()))
    }

    fn editor_id(&self) -> egui::Id {
        egui::Id::new("nmdp-editor")
    }

    fn cursor_char_range(&self, ctx: &egui::Context) -> Option<CCursorRange> {
        egui::text_edit::TextEditState::load(ctx, self.editor_id()).and_then(|s| s.cursor.char_range())
    }

    fn select_range(&mut self, ctx: &egui::Context, start: usize, end: usize) {
        let mut state = egui::text_edit::TextEditState::load(ctx, self.editor_id()).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(start), CCursor::new(end))));
        state.store(ctx, self.editor_id());
        ctx.memory_mut(|m| m.request_focus(self.editor_id()));
        self.pending_scroll = Some(start);
        if self.mode == Mode::Pretty {
            self.mode = Mode::Plain;
        }
    }

    fn do_find(&mut self, ctx: &egui::Context, forward: bool) {
        let from = self
            .cursor_char_range(ctx)
            .map(|r| {
                let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
                if forward { b } else { a }
            })
            .unwrap_or(0);
        match Self::find_in(&self.text, &self.find_query, from, forward, self.match_case) {
            Some((s, e)) => {
                self.select_range(ctx, s, e);
                self.find_status.clear();
            }
            None => self.find_status = "No matches".into(),
        }
    }

    fn do_replace(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.cursor_char_range(ctx) {
            let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
            if b > a {
                let byte_a = char_to_byte(&self.text, a);
                let byte_b = char_to_byte(&self.text, b);
                let selected = &self.text[byte_a..byte_b];
                let is_match = if self.match_case {
                    selected == self.find_query
                } else {
                    selected.eq_ignore_ascii_case(&self.find_query)
                };
                if is_match {
                    self.text.replace_range(byte_a..byte_b, &self.replace_with);
                    self.dirty = true;
                    let new_end = a + self.replace_with.chars().count();
                    self.select_range(ctx, a, new_end);
                }
            }
        }
        self.do_find(ctx, true);
    }

    fn do_replace_all(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let mut out = String::with_capacity(self.text.len());
        let mut count = 0usize;
        let mut from = 0usize;
        loop {
            match Self::find_in(&self.text[..], &self.find_query, from, true, self.match_case) {
                Some((s, e)) if s >= from => {
                    let bs = char_to_byte(&self.text, s);
                    let be = char_to_byte(&self.text, e);
                    out.push_str(&self.text[char_to_byte(&self.text, from)..bs]);
                    out.push_str(&self.replace_with);
                    let _ = be;
                    from = e;
                    count += 1;
                }
                _ => break, // wrapped around or no match
            }
        }
        out.push_str(&self.text[char_to_byte(&self.text, from)..]);
        if count > 0 {
            self.text = out;
            self.dirty = true;
        }
        self.find_status = format!("Replaced {count} occurrence(s)");
    }

    // ---------- UI sections ----------

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New\tCtrl+N").clicked() {
                    self.request(Pending::New, ctx);
                }
                if ui.button("Open…\tCtrl+O").clicked() {
                    self.request(Pending::OpenDialog, ctx);
                }
                ui.menu_button("Open Recent", |ui| {
                    if self.recent.is_empty() {
                        ui.weak("(empty)");
                    }
                    let recents = self.recent.clone();
                    for p in recents {
                        if ui.button(p.display().to_string()).clicked() {
                            self.request(Pending::OpenPath(p), ctx);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Clear list").clicked() {
                        self.recent.clear();
                    }
                });
                ui.separator();
                if ui.button("Save\tCtrl+S").clicked() {
                    self.save();
                }
                if ui.button("Save As…\tCtrl+Shift+S").clicked() {
                    self.save_as();
                }
                ui.add_enabled_ui(self.path.is_some(), |ui| {
                    if ui.button("Revert to Saved\tF5").clicked() {
                        self.request(Pending::Revert, ctx);
                    }
                });
                ui.separator();
                if ui.button("Exit").clicked() {
                    self.request(Pending::Exit, ctx);
                }
            });
            ui.menu_button("Edit", |ui| {
                let editing = self.mode != Mode::Pretty;
                ui.add_enabled_ui(editing, |ui| {
                    if ui.button("Undo\tCtrl+Z").clicked() {
                        self.send_editor_key(Key::Z, Modifiers::COMMAND);
                    }
                    if ui.button("Redo\tCtrl+Y").clicked() {
                        self.send_editor_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
                    }
                    ui.separator();
                    if ui.button("Cut\tCtrl+X").clicked() {
                        self.send_editor_event(egui::Event::Cut);
                    }
                    if ui.button("Copy\tCtrl+C").clicked() {
                        self.send_editor_event(egui::Event::Copy);
                    }
                    if ui.button("Paste\tCtrl+V").clicked() {
                        if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                            self.send_editor_event(egui::Event::Paste(t));
                        }
                    }
                    ui.separator();
                    if ui.button("Select All\tCtrl+A").clicked() {
                        let n = self.text.chars().count();
                        self.select_range(ctx, 0, n);
                    }
                });
                ui.separator();
                if ui.button("Find…\tCtrl+F").clicked() {
                    self.open_find(false);
                }
                if ui.button("Replace…\tCtrl+H").clicked() {
                    self.open_find(true);
                }
                if ui.button("Find Next\tF3").clicked() {
                    self.do_find(ctx, true);
                }
                if ui.button("Find Previous\tShift+F3").clicked() {
                    self.do_find(ctx, false);
                }
            });
            ui.menu_button("View", |ui| {
                for (m, label) in [
                    (Mode::Plain, "Plain Text\tCtrl+1"),
                    (Mode::Pretty, "Pretty\tCtrl+2"),
                    (Mode::Split, "Split\tCtrl+3"),
                ] {
                    if ui.radio(self.mode == m, label).clicked() {
                        self.mode = m;
                        ui.close();
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.prefs.word_wrap, "Word Wrap\tAlt+Z");
                ui.checkbox(&mut self.prefs.line_numbers, "Line Numbers");
                ui.separator();
                if ui.button("Zoom In\tCtrl+Plus").clicked() {
                    ctx.set_zoom_factor(ctx.zoom_factor() * 1.1);
                }
                if ui.button("Zoom Out\tCtrl+Minus").clicked() {
                    ctx.set_zoom_factor(ctx.zoom_factor() / 1.1);
                }
                if ui.button("Reset Zoom\tCtrl+0").clicked() {
                    ctx.set_zoom_factor(1.0);
                }
                ui.separator();
                if ui.button("Preferences…").clicked() {
                    self.show_prefs = true;
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About NotepadMD+").clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
        });
    }

    fn open_find(&mut self, with_replace: bool) {
        self.find_open = true;
        self.replace_open = with_replace;
        self.focus_find = true;
        self.find_status.clear();
        if self.mode == Mode::Pretty {
            self.mode = Mode::Plain;
        }
    }

    fn send_editor_key(&mut self, key: Key, modifiers: Modifiers) {
        self.pending_editor_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
    }

    fn send_editor_event(&mut self, ev: egui::Event) {
        self.pending_editor_events.push(ev);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.request(Pending::New, ctx);
            }
            if ui.button("Open").clicked() {
                self.request(Pending::OpenDialog, ctx);
            }
            if ui.button("Save").clicked() {
                self.save();
            }
            ui.separator();
            for m in [Mode::Plain, Mode::Pretty, Mode::Split] {
                if ui.selectable_label(self.mode == m, m.label()).clicked() {
                    self.mode = m;
                }
            }
            ui.separator();
            if ui.button("Find").clicked() {
                self.open_find(false);
            }
        });
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label("Find:");
            let resp = ui.add(egui::TextEdit::singleline(&mut self.find_query).desired_width(200.0));
            if self.focus_find {
                resp.request_focus();
                self.focus_find = false;
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                self.do_find(ctx, true);
                self.focus_find = true; // keep typing in the box
            }
            if ui.button("Next").clicked() {
                self.do_find(ctx, true);
            }
            if ui.button("Prev").clicked() {
                self.do_find(ctx, false);
            }
            ui.checkbox(&mut self.match_case, "Match case");
            ui.label(egui::RichText::new(&self.find_status).weak());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button("✕").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.find_open = false;
                    self.replace_open = false;
                }
            });
        });
        if self.replace_open {
            ui.horizontal(|ui| {
                ui.label("Replace:");
                ui.add(egui::TextEdit::singleline(&mut self.replace_with).desired_width(200.0));
                if ui.button("Replace").clicked() {
                    self.do_replace(ctx);
                }
                if ui.button("Replace All").clicked() {
                    self.do_replace_all();
                }
            });
        }
    }

    fn editor_ui(&mut self, ui: &mut egui::Ui) {
        self.editor_rect = ui.max_rect();
        let dark = ui.visuals().dark_mode;
        let wrap = self.prefs.word_wrap;
        let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = highlight::layout_job(buf.as_str(), dark);
            job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };

        let scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        scroll.auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let gutter = if self.prefs.line_numbers {
                    let digits = self.text.lines().count().max(1).ilog10() as usize + 1;
                    let char_w = ui.ctx().fonts_mut(|f| {
                        f.glyph_width(&egui::FontId::monospace(highlight::EDITOR_FONT_SIZE), '0')
                    });
                    let w = char_w * (digits.max(2) as f32) + 8.0;
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(w, ui.available_height()),
                        egui::Sense::hover(),
                    );
                    Some(rect)
                } else {
                    None
                };

                let editor_id = self.editor_id();
                let out = egui::TextEdit::multiline(&mut self.text)
                    .id(editor_id)
                    .font(egui::FontId::monospace(highlight::EDITOR_FONT_SIZE))
                    .code_editor()
                    .lock_focus(true) // Tab inserts a tab character
                    .desired_width(if wrap { ui.available_width() } else { f32::INFINITY })
                    .desired_rows(30)
                    .layouter(&mut layouter)
                    .show(ui);

                if out.response.changed() {
                    self.dirty = true;
                }

                // line numbers, aligned to laid-out rows (correct under word wrap)
                if let Some(gutter_rect) = gutter {
                    let painter = ui.painter_at(gutter_rect.intersect(ui.clip_rect()));
                    let color = ui.visuals().weak_text_color();
                    let font = egui::FontId::monospace(highlight::EDITOR_FONT_SIZE - 2.0);
                    let clip = ui.clip_rect();
                    let mut line_no = 1usize;
                    let mut number_next = true;
                    for row in &out.galley.rows {
                        let y = out.galley_pos.y + row.pos.y;
                        if number_next && y + row.row.height() >= clip.min.y && y <= clip.max.y {
                            painter.text(
                                egui::pos2(gutter_rect.right() - 4.0, y),
                                egui::Align2::RIGHT_TOP,
                                line_no.to_string(),
                                font.clone(),
                                color,
                            );
                        }
                        if number_next {
                            line_no += 1;
                        }
                        number_next = row.ends_with_newline;
                    }
                }

                // scroll a fresh find-match into view
                if let Some(ci) = self.pending_scroll.take() {
                    let rect = out.galley.pos_from_cursor(CCursor::new(ci)).translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect.expand(60.0), Some(Align::Center));
                }
            });
        });
        if self.text.is_empty() && self.path.is_none() {
            draw_empty_state(ui, self.editor_rect);
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        self.preview_rect = ui.max_rect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            highlight::reading_style(ui);
            // comfortable reading column
            let max_w = 860.0_f32.min(ui.available_width());
            let pad = ((ui.available_width() - max_w) / 2.0).max(12.0);
            let shown = lift_nested_fences(&self.text);
            egui::Frame::new()
                .inner_margin(egui::Margin { left: pad as i8, right: pad as i8, top: 16, bottom: 32 })
                .show(ui, |ui| {
                    ui.set_max_width(max_w);
                    CommonMarkViewer::new().show(ui, &mut self.md_cache, &shown);
                });
        });
        if self.text.is_empty() && self.path.is_none() {
            draw_empty_state(ui, self.preview_rect);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let name = self
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Untitled".into());
            ui.label(egui::RichText::new(name).small());
            if self.dirty {
                ui.label(egui::RichText::new("● modified").small().color(ui.visuals().warn_fg_color));
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(egui::RichText::new(self.mode.label()).small());
                ui.separator();
                ui.label(egui::RichText::new("UTF-8").small());
                ui.separator();
                if let Some(r) = self.cursor_char_range(ctx) {
                    let (ln, col) = line_col(&self.text, r.primary.index.0);
                    ui.label(egui::RichText::new(format!("Ln {ln}, Col {col}")).small());
                    ui.separator();
                }
                if self.prefs.word_wrap {
                    ui.label(egui::RichText::new("Wrap").small());
                    ui.separator();
                }
            });
        });
    }

    fn run_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::Undo => self.send_editor_key(Key::Z, Modifiers::COMMAND),
            MenuAction::Redo => self.send_editor_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
            MenuAction::Cut => self.send_editor_event(egui::Event::Cut),
            MenuAction::EditorCopy => self.send_editor_event(egui::Event::Copy),
            MenuAction::Paste => {
                if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    self.send_editor_event(egui::Event::Paste(t));
                }
            }
            MenuAction::Delete => self.send_editor_key(Key::Delete, Modifiers::NONE),
            MenuAction::SelectAll => {
                let n = self.text.chars().count();
                self.select_range(ctx, 0, n);
            }
            // injected now (frame start): the labels render later this frame
            // and egui's selection plugin performs the actual copy
            MenuAction::PreviewCopy => ctx.input_mut(|i| i.events.push(egui::Event::Copy)),
            MenuAction::CopyAll => ctx.copy_text(self.text.clone()),
            MenuAction::EditPlain => self.mode = Mode::Plain,
        }
    }

    fn context_menu(&mut self, ctx: &egui::Context) {
        let Some((pos, side)) = self.ctx_menu else {
            self.menu_rect = egui::Rect::NOTHING;
            self.menu_rows.clear();
            return;
        };
        // None = separator. Rows are painted by hand and hit-tested against
        // clicks swallowed in raw_input_hook, so selecting a menu item never
        // produces a pointer press that egui could react to.
        let rows: Vec<Option<(&str, bool, MenuAction)>> = match side {
            MenuSide::Editor => {
                // standard Windows edit-control menu
                let has_sel = self
                    .cursor_char_range(ctx)
                    .is_some_and(|r| r.primary.index.0 != r.secondary.index.0);
                vec![
                    Some(("Undo", true, MenuAction::Undo)),
                    Some(("Redo", true, MenuAction::Redo)),
                    None,
                    Some(("Cut", has_sel, MenuAction::Cut)),
                    Some(("Copy", has_sel, MenuAction::EditorCopy)),
                    Some(("Paste", self.ctx_menu_can_paste, MenuAction::Paste)),
                    Some(("Delete", has_sel, MenuAction::Delete)),
                    None,
                    Some(("Select All", true, MenuAction::SelectAll)),
                ]
            }
            MenuSide::Preview => {
                let has_sel = ctx
                    .plugin::<egui::text_selection::LabelSelectionState>()
                    .lock()
                    .has_selection();
                vec![
                    Some(("Copy", has_sel, MenuAction::PreviewCopy)),
                    Some(("Copy All (Markdown)", true, MenuAction::CopyAll)),
                    None,
                    Some(("Edit in Plain Text", true, MenuAction::EditPlain)),
                ]
            }
        };

        let hover = ctx.input(|i| i.pointer.hover_pos());
        self.menu_rows.clear();
        let area = egui::Area::new(egui::Id::new("ctx-menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(170.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for row in rows {
                        let Some((label, enabled, action)) = row else {
                            ui.separator();
                            continue;
                        };
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width().max(170.0), 24.0),
                            egui::Sense::hover(),
                        );
                        if enabled && hover.is_some_and(|p| rect.contains(p)) {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                ui.visuals().widgets.hovered.weak_bg_fill,
                            );
                        }
                        let color = if enabled {
                            ui.visuals().text_color()
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        ui.painter().text(
                            rect.left_center() + egui::vec2(8.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(14.0),
                            color,
                        );
                        self.menu_rows.push((rect, action, enabled));
                    }
                });
            });
        self.menu_rect = area.response.rect;

        if self.ctx_menu_opened {
            self.ctx_menu_opened = false;
        } else {
            // clicks inside the menu never reach egui, so any press seen here
            // is outside the menu → dismiss (as does Escape)
            let dismiss = ctx.input(|i| i.key_pressed(Key::Escape) || i.pointer.any_pressed());
            if dismiss {
                self.ctx_menu = None;
            }
        }
    }

    // ---------- modals ----------

    fn modals(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.confirm.clone() {
            let name = self
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into());
            egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
                ui.heading("Unsaved changes");
                ui.add_space(6.0);
                ui.label(format!("Save changes to “{name}”?"));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.confirm = None;
                        if self.save() {
                            self.proceed(action.clone(), ctx);
                        }
                    } else if ui.button("Don't Save").clicked() {
                        self.confirm = None;
                        self.dirty = false;
                        self.proceed(action.clone(), ctx);
                    } else if ui.button("Cancel").clicked() {
                        self.confirm = None;
                    }
                });
            });
        }

        if let Some(msg) = self.error.clone() {
            egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.heading("Something went wrong");
                ui.add_space(6.0);
                ui.label(&msg);
                ui.add_space(12.0);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }

        if let Some((path, bytes)) = self.lossy_offer.clone() {
            egui::Modal::new(egui::Id::new("lossy")).show(ctx, |ui| {
                ui.heading("Not valid UTF-8");
                ui.add_space(6.0);
                ui.label(format!(
                    "“{}” is not valid UTF-8 text.\nOpen it anyway? Unreadable characters will be replaced with �.",
                    path.display()
                ));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Open anyway").clicked() {
                        let s = String::from_utf8_lossy(&bytes).into_owned();
                        self.set_document(path.clone(), s);
                        self.dirty = true; // saving will rewrite as UTF-8; treat as changed
                        self.lossy_offer = None;
                    } else if ui.button("Cancel").clicked() {
                        self.lossy_offer = None;
                    }
                });
            });
        }

        if self.reload_prompt {
            egui::Modal::new(egui::Id::new("reload")).show(ctx, |ui| {
                ui.heading("File changed on disk");
                ui.add_space(6.0);
                ui.label("Another program modified this file.\nReload it? Unsaved edits here would be lost.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.reload_prompt = false;
                        if let Some(p) = self.path.clone() {
                            self.load_path(&p);
                        }
                    } else if ui.button("Keep my version").clicked() {
                        self.reload_prompt = false;
                        self.dirty = true; // buffer no longer matches disk
                    }
                });
            });
        }

        if self.show_about {
            egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("NotepadMD+");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(6.0);
                    ui.label("A lightweight Markdown notepad for Windows.");
                    ui.weak("Built with Rust and egui. Fully offline.");
                });
                ui.add_space(10.0);
                ui.separator();
                ui.monospace(
                    "Ctrl+N New        Ctrl+O Open       Ctrl+S Save\n\
                     Ctrl+Shift+S Save As               F5 Revert\n\
                     Ctrl+F Find       Ctrl+H Replace    F3 Find Next\n\
                     Ctrl+1 Plain      Ctrl+2 Pretty     Ctrl+3 Split\n\
                     Alt+Z Word Wrap   Ctrl+± Zoom",
                );
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    if ui.button("Close").clicked() {
                        self.show_about = false;
                    }
                });
            });
        }

        if self.show_prefs {
            let mut theme_changed = false;
            egui::Modal::new(egui::Id::new("prefs")).show(ctx, |ui| {
                ui.heading("Preferences");
                ui.add_space(8.0);
                egui::Grid::new("prefs-grid").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        for (t, l) in [(ThemePref::System, "System"), (ThemePref::Light, "Light"), (ThemePref::Dark, "Dark")] {
                            if ui.radio(self.prefs.theme == t, l).clicked() && self.prefs.theme != t {
                                self.prefs.theme = t;
                                theme_changed = true;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Word wrap");
                    ui.checkbox(&mut self.prefs.word_wrap, "");
                    ui.end_row();
                    ui.label("Line numbers");
                    ui.checkbox(&mut self.prefs.line_numbers, "");
                    ui.end_row();
                    ui.label("Startup mode");
                    ui.horizontal(|ui| {
                        for (m, l) in [
                            (StartupMode::Plain, "Plain Text"),
                            (StartupMode::Pretty, "Pretty"),
                            (StartupMode::LastUsed, "Last used"),
                        ] {
                            if ui.radio(self.prefs.startup_mode == m, l).clicked() {
                                self.prefs.startup_mode = m;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Remember recent files");
                    ui.checkbox(&mut self.prefs.remember_recent, "");
                    ui.end_row();
                });
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    if ui.button("Close").clicked() {
                        self.show_prefs = false;
                    }
                });
            });
            if theme_changed {
                apply_theme(ctx, self.prefs.theme);
            }
        }
    }

    fn any_modal_open(&self) -> bool {
        self.confirm.is_some()
            || self.error.is_some()
            || self.lossy_offer.is_some()
            || self.reload_prompt
            || self.show_about
            || self.show_prefs
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.any_modal_open() {
            return;
        }
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let hits: Vec<Pending> = ctx.input_mut(|i| {
            let mut v = Vec::new();
            if i.consume_shortcut(&sc(Modifiers::COMMAND, Key::N)) {
                v.push(Pending::New);
            }
            if i.consume_shortcut(&sc(Modifiers::COMMAND, Key::O)) {
                v.push(Pending::OpenDialog);
            }
            v
        });
        for h in hits {
            self.request(h, ctx);
        }

        let (save, save_as, find, replace, next, prev, m1, m2, m3, wrap, revert) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S)),
                i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::F)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::H)),
                i.consume_shortcut(&sc(Modifiers::NONE, Key::F3)),
                i.consume_shortcut(&sc(Modifiers::SHIFT, Key::F3)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Num1)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Num2)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Num3)),
                i.consume_shortcut(&sc(Modifiers::ALT, Key::Z)),
                i.consume_shortcut(&sc(Modifiers::NONE, Key::F5)),
            )
        });
        if save_as {
            self.save_as();
        } else if save {
            self.save();
        }
        if find {
            self.open_find(false);
        }
        if replace {
            self.open_find(true);
        }
        if next && !self.find_query.is_empty() {
            self.do_find(ctx, true);
        }
        if prev && !self.find_query.is_empty() {
            self.do_find(ctx, false);
        }
        if m1 {
            self.mode = Mode::Plain;
        }
        if m2 {
            self.mode = Mode::Pretty;
        }
        if m3 {
            self.mode = Mode::Split;
        }
        if wrap {
            self.prefs.word_wrap = !self.prefs.word_wrap;
        }
        if revert && self.path.is_some() {
            self.request(Pending::Revert, ctx);
        }
        // Windows-standard Redo shortcut; egui's TextEdit only knows Ctrl+Shift+Z
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Y))) {
            self.send_editor_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        }
    }

    fn poll_disk(&mut self) {
        if self.last_disk_check.elapsed() < DISK_POLL {
            return;
        }
        self.last_disk_check = Instant::now();
        if let Some(p) = &self.path {
            if let Ok(mtime) = std::fs::metadata(p).and_then(|m| m.modified()) {
                if let Some(known) = self.disk_mtime {
                    if mtime != known && !self.reload_prompt {
                        self.reload_prompt = true;
                        self.disk_mtime = Some(mtime); // don't re-prompt for the same change
                    }
                } else {
                    self.disk_mtime = Some(mtime);
                }
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let title = format!("{}{} — NotepadMD+", if self.dirty { "● " } else { "" }, name);
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        self.poll_disk();
        ctx.request_repaint_after(DISK_POLL); // keeps disk polling alive while idle

        // drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            self.request(Pending::OpenPath(p), ctx);
        }

        // intercept window close while dirty
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Exit);
        }

        // Replay editor events queued by menu clicks last frame (the editor
        // renders before the menus, so same-frame events would be missed).
        if !self.pending_editor_events.is_empty() {
            ctx.memory_mut(|m| m.request_focus(self.editor_id()));
            let events = std::mem::take(&mut self.pending_editor_events);
            ctx.input_mut(|i| i.events.extend(events));
        }

        // Menu row clicked last frame (the click itself was swallowed in
        // raw_input_hook, so no selection anywhere was disturbed). Run the
        // action now, before panels render, so injected events land this frame.
        if let Some(p) = self.pending_menu_click.take() {
            let action = self
                .menu_rows
                .iter()
                .find(|(r, _, enabled)| *enabled && r.contains(p))
                .map(|(_, a, _)| *a);
            if let Some(a) = action {
                self.ctx_menu = None;
                self.run_menu_action(a, ctx);
            } // clicking a disabled row or padding keeps the menu open, like Windows
        }

        // Right-click (captured and swallowed in raw_input_hook so selections
        // survive it, as in native Windows apps): open the matching context menu.
        if let Some(pos) = self.pending_context_click.take() {
            if !self.any_modal_open() {
                if self.mode != Mode::Plain && self.preview_rect.contains(pos) {
                    self.ctx_menu = Some((pos, MenuSide::Preview));
                    self.ctx_menu_opened = true;
                } else if self.mode != Mode::Pretty && self.editor_rect.contains(pos) {
                    self.ctx_menu_can_paste = arboard::Clipboard::new()
                        .and_then(|mut c| c.get_text())
                        .is_ok_and(|t| !t.is_empty());
                    self.ctx_menu = Some((pos, MenuSide::Editor));
                    self.ctx_menu_opened = true;
                }
            }
        }

        self.handle_shortcuts(ctx);
        self.update_title(ctx);
        self.prefs.last_mode = self.mode;

        egui::Panel::top("menu").show(root, |ui| self.menu_bar(ui, ctx));
        egui::Panel::top("toolbar").show(root, |ui| self.toolbar(ui, ctx));
        if self.find_open {
            egui::Panel::top("find").show(root, |ui| self.find_bar(ui, ctx));
        }
        egui::Panel::bottom("status").show(root, |ui| self.status_bar(ui, ctx));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(0))
            .show(root, |ui| match self.mode {
                Mode::Plain => self.editor_ui(ui),
                Mode::Pretty => self.preview_ui(ui),
                Mode::Split => {
                    ui.columns(2, |cols| {
                        self.editor_ui(&mut cols[0]);
                        self.preview_ui(&mut cols[1]);
                    });
                }
            });

        self.context_menu(ctx);
        self.modals(ctx);
    }

    /// Swallow right-clicks before egui sees them: both the editor's and the
    /// preview's text selections are cleared by egui on *any* pointer press,
    /// but native Windows apps keep the selection on right-click (that's how
    /// "right-click the highlight → Copy" works). We capture the position and
    /// open our own context menu in update() instead.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let menu_rect = if self.ctx_menu.is_some() { self.menu_rect } else { egui::Rect::NOTHING };
        raw_input.events.retain(|e| match e {
            egui::Event::PointerButton {
                button: egui::PointerButton::Secondary,
                pressed,
                pos,
                ..
            } => {
                if !pressed {
                    self.pending_context_click = Some(*pos);
                }
                false
            }
            // Clicks on the open context menu are swallowed too and hit-tested
            // by hand in update(): if egui saw the press, its selection plugin
            // would clear the preview highlight before the action runs.
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary,
                pressed,
                pos,
                ..
            } if menu_rect.contains(*pos) => {
                if !pressed {
                    self.pending_menu_click = Some(*pos);
                }
                false
            }
            _ => true,
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "prefs", &self.prefs);
        let recent: &[PathBuf] = if self.prefs.remember_recent { &self.recent } else { &[] };
        eframe::set_value(storage, "recent", &recent.to_vec());
    }
}

// ---------- helpers ----------

/// Friendly watermark shown when nothing is open yet (new empty document).
fn draw_empty_state(ui: &egui::Ui, rect: egui::Rect) {
    let p = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    let center = rect.center() - egui::vec2(0.0, 30.0);

    let tile = if dark {
        egui::Color32::from_white_alpha(8)
    } else {
        egui::Color32::from_black_alpha(10)
    };
    let accent = egui::Color32::from_rgb(86, 156, 214)
        .gamma_multiply(if dark { 0.55 } else { 0.8 });
    let faint = ui.visuals().weak_text_color().gamma_multiply(0.75);

    let icon = egui::Rect::from_center_size(center - egui::vec2(0.0, 40.0), egui::vec2(112.0, 112.0));
    p.rect_filled(icon, 26.0, tile);
    p.text(
        icon.center(),
        egui::Align2::CENTER_CENTER,
        "MD+",
        egui::FontId::monospace(38.0),
        accent,
    );
    p.text(
        center + egui::vec2(0.0, 48.0),
        egui::Align2::CENTER_CENTER,
        "NotepadMD+",
        egui::FontId::proportional(22.0),
        faint,
    );
    p.text(
        center + egui::vec2(0.0, 78.0),
        egui::Align2::CENTER_CENTER,
        "Start typing  ·  Ctrl+O to open  ·  or drop a file here",
        egui::FontId::proportional(14.0),
        faint.gamma_multiply(0.8),
    );
}

fn apply_theme(ctx: &egui::Context, theme: ThemePref) {
    ctx.options_mut(|o| {
        o.theme_preference = match theme {
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::Dark => egui::ThemePreference::Dark,
        };
    });
}

/// Use native Windows fonts when available for a more native look.
/// Falls back silently to egui's embedded fonts elsewhere.
fn install_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: [(&str, &[&str]); 2] = [
        ("ui", &[r"C:\Windows\Fonts\segoeui.ttf"]),
        ("mono", &[r"C:\Windows\Fonts\consola.ttf"]),
    ];
    for (name, paths) in candidates {
        for p in paths {
            if let Ok(bytes) = std::fs::read(p) {
                fonts
                    .font_data
                    .insert(name.to_string(), egui::FontData::from_owned(bytes).into());
                let family = if name == "mono" {
                    egui::FontFamily::Monospace
                } else {
                    egui::FontFamily::Proportional
                };
                fonts.families.entry(family).or_default().insert(0, name.to_string());
                break;
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn friendly_io_error(what: &str, path: &Path, e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    let why = match e.kind() {
        NotFound => "The file could not be found.".into(),
        PermissionDenied => "Permission denied — the file may be read-only, locked by another program, or in a protected folder.".into(),
        _ => e.to_string(),
    };
    format!("{what}:\n{}\n\n{why}", path.display())
}

/// egui_commonmark renders each list item in a `horizontal_wrapped` row, so a
/// fenced code block nested in a list item floats to the right of the text and
/// overlaps the content below it. Work around it for the preview only: lift
/// indented fences to column 0 as standalone blocks (ordered lists resume with
/// the right number afterwards since the renderer honors the list start).
/// ponytail: preview loses the code block's list indentation; drop this when
/// upstream fixes block widgets inside list items.
fn lift_nested_fences(text: &str) -> std::borrow::Cow<'_, str> {
    let is_opener = |line: &str| {
        let ind = line.len() - line.trim_start().len();
        if ind == 0 {
            return false;
        }
        let t = &line[ind..];
        // backtick fences may not contain ` in the info string
        (t.starts_with("```") && !t.trim_start_matches('`').contains('`')) || t.starts_with("~~~")
    };
    if !text.lines().any(is_opener) {
        return std::borrow::Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len() + 16);
    // (indent length to strip, fence char, fence length) while inside a fence
    let mut fence: Option<(usize, char, usize)> = None;
    let mut prev_blank = true;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        match fence {
            None => {
                if is_opener(body) {
                    let ind = body.len() - body.trim_start().len();
                    let c = body[ind..].chars().next().unwrap();
                    let flen = body[ind..].chars().take_while(|&x| x == c).count();
                    if !prev_blank {
                        out.push('\n');
                    }
                    out.push_str(&line[ind..]);
                    fence = Some((ind, c, flen));
                } else {
                    out.push_str(line);
                }
                prev_blank = body.trim().is_empty();
            }
            Some((ind, c, flen)) => {
                // strip up to the opener's indent (content may be less indented)
                let ws = body.len() - body.trim_start().len();
                let strip = ws.min(ind);
                out.push_str(&line[strip..]);
                let t = body[strip..].trim_end();
                let closes = t.chars().take_while(|&x| x == c).count() >= flen
                    && t.chars().all(|x| x == c);
                if closes {
                    fence = None;
                    prev_blank = false;
                }
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

fn line_col(s: &str, char_idx: usize) -> (usize, usize) {
    let mut ln = 1;
    let mut col = 1;
    for (i, c) in s.chars().enumerate() {
        if i == char_idx {
            break;
        }
        if c == '\n' {
            ln += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (ln, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_wraps_and_ignores_case() {
        let t = "Hello world, hello Rust";
        assert_eq!(App::find_in(t, "hello", 0, true, false), Some((0, 5)));
        assert_eq!(App::find_in(t, "hello", 1, true, false), Some((13, 18)));
        assert_eq!(App::find_in(t, "hello", 20, true, false), Some((0, 5))); // wrap
        assert_eq!(App::find_in(t, "hello", 0, true, true), Some((13, 18))); // case-sensitive
        assert_eq!(App::find_in(t, "xyz", 0, true, false), None);
        // backwards
        assert_eq!(App::find_in(t, "hello", 20, false, false), Some((13, 18)));
        assert_eq!(App::find_in(t, "hello", 5, false, false), Some((0, 5)));
    }

    #[test]
    fn find_multibyte_safe() {
        let t = "日本語 abc 日本語";
        assert_eq!(App::find_in(t, "abc", 0, true, false), Some((4, 7)));
        assert_eq!(char_to_byte(t, 4), 10); // 3 CJK chars * 3 bytes + space
    }

    #[test]
    fn lifts_fences_out_of_list_items() {
        let src = "2. **Back up** the DB:\n   ```bash\n   TS=$(date)\n     indented more\n   ```\n3. Next item\n";
        let out = lift_nested_fences(src);
        assert_eq!(
            out,
            "2. **Back up** the DB:\n\n```bash\nTS=$(date)\n  indented more\n```\n3. Next item\n"
        );
        // top-level fences and fence-free text stay untouched (borrowed)
        let plain = "# t\n\n```rs\nlet x = 1;\n```\n";
        assert!(matches!(lift_nested_fences(plain), std::borrow::Cow::Borrowed(_)));
        // ``` inside a nested fence body must not close or re-open anything
        let tricky = "- item\n  ~~~md\n  ```\n  not a fence\n  ```\n  ~~~\n";
        let out = lift_nested_fences(tricky);
        assert_eq!(out, "- item\n\n~~~md\n```\nnot a fence\n```\n~~~\n");
        // unclosed fence runs to EOF without panicking
        let unclosed = "1. a\n   ```\n   code";
        assert_eq!(lift_nested_fences(unclosed), "1. a\n\n```\ncode");
    }

    #[test]
    fn line_col_basic() {
        let t = "ab\ncd";
        assert_eq!(line_col(t, 0), (1, 1));
        assert_eq!(line_col(t, 4), (2, 2));
    }
}
