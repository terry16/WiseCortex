@echo off
rem ============================================================
rem  WiseCortex - Windows build script
rem  Usage:
rem    scripts\build-windows.bat            web + backend   (the everyday build)
rem    scripts\build-windows.bat backend    backend only (server + cli, release)
rem    scripts\build-windows.bat web        frontend only (web\dist)
rem    scripts\build-windows.bat desktop    desktop client (web + Tauri installers)
rem    scripts\build-windows.bat all        everything, installers included
rem
rem  The default deliberately skips the desktop step: packaging the Tauri installer
rem  takes minutes and almost no run needs it. Ask for it explicitly.
rem
rem  Prereqs: Rust (rustup), Node.js >= 20.
rem  Desktop also needs VS C++ build tools + WebView2 runtime.
rem ============================================================
rem
rem  KEEP THIS FILE PURE ASCII - comments included. Two reasons, both cost us a
rem  broken build already:
rem
rem  1. cmd.exe decodes the .bat with the *console* codepage, so any non-ASCII
rem     text prints as mojibake on a machine whose codepage differs from the
rem     file encoding. A diagnostic nobody can read is a diagnostic nobody has.
rem  2. Worse: cmd.exe seeks through the batch file by BYTE offset and re-reads
rem     it after each command. Multi-byte characters make the offset arithmetic
rem     disagree with the decoder, and the parser resumes in the MIDDLE of a
rem     line - executing the tail of a comment as if it were a command. Running
rem     `chcp` mid-script to "fix" the mojibake triggers this immediately: the
rem     codepage changes under the already-stored offset. We shipped exactly
rem     that and the build started spraying
rem     "'...' is not recognized as an internal or external command" from inside
rem     its own comments. ASCII-only sidesteps the whole class of bug, and then
rem     no chcp is needed either.
rem
rem  Also keep ampersand and pipe characters out of rem lines: rem does not
rem  protect them, cmd still treats them as command separators and will try to
rem  run the rest of the comment. This very warning used to contain the two
rem  characters it warns about, and cmd duly tried to execute it.
rem ============================================================
setlocal enableextensions

rem cd to repo root (this script lives in scripts\)
pushd "%~dp0.."

rem Rust may be installed under %USERPROFILE%\.cargo\bin but not on PATH
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

set "TARGET=%~1"
if "%TARGET%"=="" set "TARGET=all"

echo ============================================================
echo   WiseCortex build   target=%TARGET%   dir=%CD%
echo ============================================================

where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] cargo not found. Install Rust via rustup, or ensure %USERPROFILE%\.cargo\bin exists.
  goto :fail
)

if /i "%TARGET%"=="web" (
  call :web || goto :fail
) else if /i "%TARGET%"=="backend" (
  call :backend || goto :fail
) else if /i "%TARGET%"=="desktop" (
  call :web || goto :fail
  call :desktop || goto :fail
) else if /i "%TARGET%"=="fast" (
  rem Runnable artifacts only, no installers: packaging Tauri takes minutes and
  rem is dead time when you are iterating on the backend.
  call :web || goto :fail
  call :backend || goto :fail
) else if /i "%TARGET%"=="all" (
  call :web || goto :fail
  call :backend || goto :fail
  call :desktop || goto :fail
) else (
  echo [ERROR] unknown target "%TARGET%". use: all ^| fast ^| backend ^| web ^| desktop
  goto :fail
)

echo.
echo ============================================================
echo   BUILD OK
echo ============================================================
popd
endlocal
exit /b 0

:fail
echo.
echo ============================================================
echo   BUILD FAILED  (see errors above)
echo ============================================================
popd
endlocal
exit /b 1

rem ---------------- subroutines ----------------

:web
echo.
echo [web] building web\dist ...
where npm >nul 2>&1
if errorlevel 1 (
  echo [ERROR] npm not found. Install Node.js ^(^>=20^).
  exit /b 1
)
pushd web
rem Always npm install, never npm ci.
rem
rem npm ci deletes node_modules wholesale before reinstalling. On Windows, any
rem process holding a single file in that tree (a running vite dev server, a
rem vitest watcher, an antivirus scan) makes the delete fail with EPERM - by
rem which point half the tree is already gone. tsc and vite vanish, and what the
rem script reports is "'tsc' is not recognized as an internal or external
rem command", which points nowhere near the real cause, and the tree now needs
rem repairing by hand. We hit this twice in one day. install reconciles
rem incrementally, never nukes the tree, and renames locked files out of the way.
call npm install
if errorlevel 1 ( popd & exit /b 1 )

rem Verify the executables the build actually invokes are present.
rem
rem npm run build runs tsc --noEmit first and then vite build, so BOTH must be
rem checked. The previous version only checked tsc - so a half-installed tree
rem with tsc present but vite missing walked straight past this guard and the
rem user still got the same unreadable "'vite' is not recognized" message this
rem guard exists to prevent. That hole was real and someone hit it.
rem
rem npm ci is not the only way to end up half-installed: a second copy of this
rem script or another npm install running concurrently (two npms fighting over
rem one tree), an npm proxy configured but not running so package fetches die
rem midway, or antivirus holding files - all leave the same wreckage. Re-running
rem install fixes it in practice (that is how we repaired it by hand, twice), so
rem do that once automatically before giving up rather than dumping a cryptic
rem error on the user.
call :webtools
if not errorlevel 1 goto :webbuild
echo [web] node_modules is incomplete, reinstalling once to repair ...
call npm install
if errorlevel 1 ( popd & exit /b 1 )
call :webtools
if not errorlevel 1 goto :webbuild
echo [ERROR] node_modules still incomplete: .bin is missing tsc or vite, and one
echo         automatic reinstall did not repair it. Usual causes:
echo           - another copy of this script, or another npm install, running now
echo           - npm proxy configured but not reachable, so fetches die midway
echo           - dev server / vitest / antivirus holding files in web\node_modules
echo         Clear that and re-run. If it persists, delete web\node_modules and retry.
popd
exit /b 1

:webbuild
call npm run build
if errorlevel 1 ( popd & exit /b 1 )
popd
echo [web] done -^> web\dist
exit /b 0

rem Are the build's executables all present? 0 = yes, 1 = something is missing.
rem Must be called with the web directory as the current directory.
:webtools
if not exist "node_modules\.bin\tsc.cmd" exit /b 1
if not exist "node_modules\.bin\vite.cmd" exit /b 1
exit /b 0

:backend
echo.
echo [backend] building server + cli (release) ...
cargo build --release -p wisecortex-server -p wisecortex-cli
if errorlevel 1 exit /b 1
echo [backend] done:
echo     target\release\wisecortex-server.exe   ^(backend, listens 127.0.0.1:7070^)
echo     target\release\wisecortex.exe          ^(CLI^)
exit /b 0

:desktop
echo.
echo [desktop] packaging Windows client with Tauri ...
cargo tauri --version >nul 2>&1
if errorlevel 1 (
  echo [desktop] tauri-cli not found, installing ...
  cargo install tauri-cli --version "^2.0" --locked
  if errorlevel 1 exit /b 1
)
cargo tauri build
if errorlevel 1 exit /b 1
echo [desktop] done -^> installers in target\release\bundle\
echo     msi\WiseCortex_*_x64_en-US.msi
echo     nsis\WiseCortex_*_x64-setup.exe
exit /b 0
