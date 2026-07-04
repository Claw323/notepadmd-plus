@echo off
rem Build the Windows release natively on Windows. Output: dist\NotepadMD+.exe
rem remap-path-prefix keeps local filesystem paths out of the shipped binary
set RUSTFLAGS=%RUSTFLAGS% --remap-path-prefix=%USERPROFILE%=/build --remap-path-prefix=%CD%=/src
cargo build --release || exit /b 1
if not exist dist mkdir dist
copy /y target\release\notepadmd_plus.exe "dist\NotepadMD+.exe" >nul
echo Built: dist\NotepadMD+.exe
