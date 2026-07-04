fn main() {
    // Embed icon + version metadata into the Windows executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "NotepadMD+");
        res.set("FileDescription", "NotepadMD+ — Markdown Notepad");
        res.set("OriginalFilename", "NotepadMD+.exe");
        res.set("LegalCopyright", "Copyright © 2026 Claw323 — MIT License");
        res.compile().expect("failed to embed Windows resources");
    }
}
