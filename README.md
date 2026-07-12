# DesktopCountdown

A countdown that lives on your Windows wallpaper — under the desktop icons, above whatever
else is drawing your background.

![The countdown painted into the wallpaper layer](docs/screenshot.png)

It does not steal focus, does not sit in a window, and does not cover your background: it is
painted into the wallpaper layer itself, so it survives Explorer restarts and coexists with
animated-wallpaper apps like Wallpaper Engine.

## Features

- **Lines you compose yourself.** The display is a list of lines, each a template string. Put a
  clock, a day count, a caption, or all three — in any order.
- **Tokens.** `{daysTotal} days left`, `D-{daysTotal}`, `{hh}:{mm}:{ss}`, `{months}m {weeks}w
  {days}d` — 13 tokens covering every part of the remaining time.
- **Per-line size, alignment and colour.** Everything else (font, weight, outline, shadow,
  letter spacing, opacity) is shared, so the whole thing stays coherent.
- **Live editing.** A settings window writes `config.toml`; the renderer watches the file and
  repaints within ~0.2 s. Drag a slider and the wallpaper follows it in real time.
- **Multi-monitor.** Every monitor gets the countdown, and any monitor can override the style,
  the position, the line list, or opt out entirely.
- **Font picker that shows the fonts.** Each family name is drawn in its own font, so Korean,
  Chinese and Japanese family names are legible instead of tofu.

## Install

Grab `desktop-countdown.exe` from the [latest
release](https://github.com/HaJH/DesktopCountdown/releases) and run it. It is a single
self-contained binary — the C runtime is linked statically and everything else it uses
(Direct2D, DirectWrite, DirectComposition) ships with Windows. There is nothing to install and
no console window; look for the tray icon.

To build it yourself you need [Rust](https://rustup.rs) (1.92+) and Windows 10/11:

```
cargo build --release
```

The result is at `target\release\desktop-countdown.exe`. (`build.bat` does the same and can be
double-clicked.)

| Command | What it does |
|---|---|
| `desktop-countdown.exe` | Draws the countdown. Right-click the tray icon for the menu. |
| `desktop-countdown.exe --settings` | Opens the settings window. |

## Configuration

Right-click the tray icon → **Open settings** for the GUI. Every change is saved to
`%APPDATA%\DesktopCountdown\config.toml` automatically and shows up on the wallpaper right
away. You can also edit that file by hand; it is re-read about 100 ms after you save it.

### Lines and tokens

The display is a stack of lines, top to bottom. Each line's `text` is a template. `size_ratio`
multiplies the base `[style].size_px`, and a line without a `color` inherits `[style].color`.

```toml
target = "2026-12-31T23:59:59"

[style]
font_family = "Consolas"
size_px = 64.0            # the base size that every size_ratio multiplies
color = "#FFFFFF"

[[line]]
text = "PROJECT DEADLINE"
size_ratio = 0.17
color = "#9FB4C7"

[[line]]
text = "{daysTotal} days left"
size_ratio = 0.3

[[line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
align = "center"          # left | center | right
```

Available tokens:

| Token | Meaning |
|---|---|
| `{months}` `{weeks}` `{days}` | Calendar months, then whole weeks, then whole days |
| `{daysTotal}` `{hoursTotal}` `{minutesTotal}` `{secondsTotal}` | Total time remaining, in that unit |
| `{hours}` `{minutes}` `{seconds}` | Hours within the day (0–23), minutes, seconds |
| `{hh}` `{mm}` `{ss}` | Zero-padded to two digits (`{hh}` is the *total* hours, so it grows past two) |

An unknown token is printed as-is, so a typo shows on the wallpaper instead of vanishing
silently.

The settings window ships presets — Classic, Clock only, D-Day, Days left, Caption + Clock —
that fill the whole list in one click. The default is Classic: `{months}m {weeks}w {days}d`
above `{hh}:{mm}:{ss}`.

### Per-monitor overrides

The settings window has a tab per monitor. Turning on the override copies the current global
settings so you tweak from what you see. By hand, it is a `[[display]]` block:

```toml
[[display]]
id = "\\\\?\\DISPLAY#DEL41A8#1"
name = "DISPLAY1"
enabled = true
anchor = "top-center"
size_px = 48.0

[[display.line]]
text = "D-{daysTotal}"
```

A monitor's line list replaces the global one wholesale. `enabled = false` hides the countdown
on that monitor.

A bad config is not applied: the previous one stays, the tray icon shows a warning, and the
reason is written to `%LOCALAPPDATA%\DesktopCountdown\log.txt`.

## How it works

Windows draws the wallpaper in a `WorkerW` window that sits below the desktop icons. The app
finds that window (sending the undocumented `0x052C` message to `Progman` when it has to),
creates a child of it, and composes a per-pixel-alpha visual onto it with DirectComposition.
`UpdateLayeredWindow` is the obvious approach and does not work here — it returns `Ok` from a
child window and draws nothing, which a spike confirmed
(`docs/superpowers/plans/spike-result.md`).

The text itself is DirectWrite: glyph outlines, measured by their *ink* rather than their line
boxes, so the gap between lines is the gap you asked for in any font — including CJK fonts,
whose ascent is sized for Hangul while a countdown only ever draws digits.

## Known limitations

- The countdown is part of the wallpaper, so you cannot click it. Everything is driven from the
  tray icon and the settings window.
- Attaching to the wallpaper layer relies on undocumented Windows behaviour. It re-attaches by
  itself when Explorer restarts, but a future Windows update could change the window structure
  and break it.
- At the target time the countdown stops at `00:00:00`. It does not count up and does not
  notify.

## Documentation

Design documents and implementation plans live in `docs/` (in Korean). They record the
non-obvious decisions — why DirectComposition, how the wallpaper window is found, how the line
list replaced the old fixed two-line layout.

## Licence

MIT. See [LICENSE](LICENSE).
