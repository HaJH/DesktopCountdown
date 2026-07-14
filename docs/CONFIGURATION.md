# Configuration

Everything the settings window writes, it writes here. You can edit these files by hand too —
`config.toml` is re-read about 100 ms after you save it.

| | `config.toml` | `presets.toml` | Log |
|---|---|---|---|
| Windows | `%APPDATA%\DesktopCountdown\` | same folder | `%LOCALAPPDATA%\DesktopCountdown\log.txt` |
| macOS | `~/Library/Application Support/DesktopCountdown/` | same folder | `~/Library/Logs/DesktopCountdown/log.txt` |

A bad config is not applied: the previous one stays, the tray icon shows a warning, and the
reason is written to the log.

## Lines and tokens

The display is a stack of lines, top to bottom. Each line's `text` is a template. `size_ratio`
multiplies the base `[style].size_px`, and a line without a `color` inherits `[style].color`.

```toml
target = "2026-12-31T23:59:59"
daily_target = "18:00:00"   # a clock time the daily tokens count to, every day

[style]
font_family = "Consolas"
size_px = 128.0           # the base size that every size_ratio multiplies
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
| `{dailyHh}` `{dailyMm}` `{dailySs}` | To the daily target, zero-padded: hours, minutes, seconds. Past it they count up until midnight resets the cycle |
| `{dailyHours}` `{dailyMinutes}` `{dailySeconds}` | The same, without the padding |
| `{dailyMinutesTotal}` | Total minutes to (or past) the daily target |
| `{dailySign}` | Empty while counting down, `+` once the daily target has passed |

The `daily*` tokens count to `daily_target` — a clock time, not a date. Every day counts
down to it anew, counts up past it (put `{dailySign}` in front to show the `+`), and resets
at midnight. `target` and `daily_target` are independent; one line can use either, or both.
A `daily_target` of `00:00:00` never counts down: every day is entirely overtime, counting up from midnight.

An unknown token is printed as-is, so a typo shows on the wallpaper instead of vanishing
silently.

## Style

`[style]` is shared by every line. Only size, alignment and colour can differ per line, which
is what keeps a three-line countdown looking like one thing rather than three.

| Key | Default | Range |
|---|---|---|
| `font_family` | `Consolas` on Windows, `Menlo` on macOS | any installed family; an unknown one falls back |
| `font_weight` | `400` | `100`–`900` |
| `size_px` | `128.0` | above `0` — the base that every `size_ratio` multiplies |
| `mode` | `fill` | `fill` \| `outline` \| `both` |
| `color` | `#FFFFFF` | `#RRGGBB` |
| `outline_color` | `#000000` | `#RRGGBB` |
| `outline_width_px` | `1.5` | `0` or above |
| `opacity` | `0.85` | `0.0`–`1.0` |
| `letter_spacing_em` | `0.02` | any finite number; negative tightens |
| `shadow` | `true` | |
| `tabular_figures` | `true` | keeps digits from shuffling sideways as they tick |

`font_family` is the one default that differs per system: a `config.toml` written on one and
opened on the other keeps whatever font it names, and only a *new* file gets the local default.

A `[[line]]`'s own keys:

| Key | Default | Range |
|---|---|---|
| `text` | `""` | up to 200 characters; empty draws nothing |
| `size_ratio` | `1.0` | above `0` — multiplies `[style].size_px` |
| `align` | `center` | `left` \| `center` \| `right` |
| `color` | inherits `[style].color` | `#RRGGBB` |

## Presets

A preset is a **named snapshot of the whole look** — the line list *and* the style. Picking one
replaces both.

Six ship with the app: Clock only, Summary + Clock, D-Day, Days left, Caption + Clock,
Daily countdown. A fresh config starts on Clock only: `{hh}:{mm}:{ss}` on its own.

Picking a preset applies it straight away. Editing on top of it does not touch the preset — the
picker just marks the look as changed (`Clock only *`), and `Reset` puts it back. `Save as…`
stores the current lines and style under a name of your own, so switching presets never costs
you a look you cared to keep; a preset you saved can be deleted again, which drops the name and
leaves the wallpaper as it is.

`config.toml` carries a `preset` key naming which one is active:

```toml
target = "2026-12-31T23:59:59"
preset = "Clock only"
```

That is bookkeeping for the settings window only — the renderer ignores it and draws from
`[style]` and `[[line]]` alone — so there is no need to set it by hand. If it is missing, or
names a preset that no longer exists, the settings window recovers the label by matching your
lines and style against the presets it knows, rather than showing `Custom` for no reason.

### `presets.toml`

Your own presets live in `presets.toml`, next to `config.toml`. The renderer never reads that
file: the preset library stays out of the config the countdown is drawn from, however long it
grows.

```toml
[[preset]]
name = "Nextfest"

[preset.style]
font_family = "Consolas"
size_px = 128.0
color = "#FFFFFF"

[[preset.line]]
text = "NEXTFEST"
size_ratio = 0.25
color = "#A200FF"

[[preset.line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
```

You can write presets into it by hand. One that names a value the renderer would refuse, or
that takes a name already spoken for, is left out of the picker — but it is left in the file,
not deleted, so a preset you wrote is never lost to a preset you save.

## Per-monitor overrides

The settings window has a tab per monitor. Turning on the override copies the current global
settings, so you tweak from what you see rather than from blank defaults — which also means the
monitor stops following later changes to the global settings. Turn the override off to hand it
back.

By hand, it is a `[[display]]` block:

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

A monitor's line list replaces the global one wholesale — there is no per-line merge.
`enabled = false` hides the countdown on that monitor.

Presets are global only. A preset is a whole-look snapshot and an override is a partial change
on top of one, so the two do not mix; the monitor tabs have no preset picker.

## Layout

```toml
[layout]
anchor = "center"         # top-left | top-center | top-right | middle-left | center |
                          # middle-right | bottom-left | bottom-center | bottom-right
offset_px = [0, 0]        # nudged from the anchor; may be negative
```

## General

```toml
[general]
autostart = false         # start with the system
```
