# macOS Runbook

## Purpose

Use this runbook to launch or recover the known-good Apple Silicon installations. It records the compatibility work that was required so it does not have to be rediscovered.

## Launching

Open the desired app directly from `~/Applications/`:

- `Lords of Magic 3.02.app` for the near-vanilla bug-fix release.
- `Lords of Magic GS5R3.app` for the balance overhaul.
- `Steambuild 32 64bit DXVK.app` only as the preserved working baseline.

Do not launch these profiles through native macOS Steam. Each app contains its own Windows Steam installation and Wine prefix, but normal gameplay starts `lomse.exe` directly.

## Verified rendering configuration

The game uses `cnc-ddraw` 7.1.0.0 with Wine's native DLL override:

```ini
[ddraw]
windowed=true
maintas=true
maxfps=60
renderer=opengl
shader=Shaders\interpolation\lanczos2-sharp.glsl
```

The window dimensions can change when the window is resized; `cnc-ddraw` automatically persists window position and size. `Option/Alt + Enter` toggles windowed and fullscreen modes.

### Why this is necessary

Without the DirectDraw wrapper, the original game attempted an unsupported fullscreen display-mode change on the ultrawide Retina display. The Wine log showed:

- `NtUserChangeDisplaySettings ... returned -2`
- `GL_INVALID_FRAMEBUFFER_OPERATION`

The windowed OpenGL renderer avoids that legacy display transition.

## Verified CD-path configuration

The Steam install script expects this registry value:

```text
HKLM\Software\Sierra Online\Setup\LOMSE\CDPath = C:\
```

Because `lomse.exe` is a 32-bit process inside a 64-bit Wine prefix, the effective entry must also exist in the redirected view:

```text
HKLM\Software\Wow6432Node\Sierra Online\Setup\LOMSE\CDPath = C:\
```

The missing `Wow6432Node` value was the actual cause of the CD-ROM prompt in the baseline Steam release. Adding only the 64-bit registry value did not work.

Each prefix also exposes its own game directory as Wine drive `D:`. This did not fix the CD prompt by itself, but it remains a harmless compatibility fallback.

Community 3.02 and GS5R3 remove the legacy CD check in their scripts, so those profiles should not depend on this workaround. It is retained because it is already verified and causes no observed regression.

## Profile layout

Within each app, the game directory is:

```text
Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Steam/
steamapps/common/Lords of Magic Special Edition/English
```

Each modded profile contains `_vanilla_backup/` with copies of:

- `gs.mpq`
- `pic.mpq`
- `lom.cfg`
- `settings.cfg`
- `ddraw.ini`

Save games live under that profile's `English/savegame/` directory. Do not assume saves are portable between vanilla, 3.02, and GS5R3.

## Recovery checklist

If the game stops opening:

1. Confirm no stale `lomse.exe`, `wineserver`, or `wineskinlauncher` process is running.
2. Confirm the app's `Info.plist` points `Program Name and Path` at the installed `English/lomse.exe`.
3. Confirm `ddraw.dll` and `ddraw.ini` are next to `lomse.exe`.

**`ddraw.ini` is rewritten by the game on exit.** *Observed 2026-09-18*: after an attended run the
file came back one byte longer, with line 32's `shader=` terminator changed from `\n` to `\r\n`,
and each profile accumulates its own window geometry besides. Two consequences worth knowing before
an attended session:

- a "did anything change in this profile?" check will flag `ddraw.ini` every time the game has run,
  and that is the game working normally, not the experiment leaking;
- a profile's `ddraw.ini` is **local state, not a clone of the baseline's**. The development profile's
  differed from the baseline's before any of today's work. Restoring it *from the baseline* therefore
  discards whatever that profile had, which is easy to do by accident and cannot be undone from a
  hash-only snapshot. `.lom-pipeline/pristine` holds `gs.mpq` and `pic.mpq` only, by design -- it is
  not a profile backup.

The general form of that second point: a snapshot that records digests is a **change detector**, not
a backup. Copy the bytes of anything you might want to put back.
4. Confirm the Wine DLL override contains `*ddraw = native,builtin`.
5. Confirm the profile's `dosdevices/d:` symlink points inside the same app, not another clone.
6. For the baseline build, confirm the 32-bit `Wow6432Node` CD-path entry exists.
7. Compare changed gameplay files with `_vanilla_backup/` before replacing anything.

## Performance notes

- Rendering is capped at 60 FPS.
- `ddraw.dll`, Wine OpenGL, Apple's OpenGL-to-Metal driver, and the AGX Metal driver were all observed loaded in the running process.
- The 1998 game loop consumes approximately one CPU core even at the 60 FPS render cap. This is expected busy-loop behavior, not evidence of a GPU bottleneck.
- Raising the frame cap is not currently recommended; it offers little benefit and may expose timing problems.
