# DesktopCountdown

A countdown that lives on your wallpaper — under the desktop icons, above whatever else is
drawing your background. Windows and macOS.

![The countdown painted into the wallpaper layer](docs/screenshot.png)

It does not steal focus, does not sit in a window, and does not cover your background: it is
painted into the wallpaper layer itself, so it survives Explorer restarts and coexists with
animated-wallpaper apps like Wallpaper Engine.

## Features

- **Lines you compose yourself.** The display is a list of lines, each a template string. Put a
  clock, a day count, a caption, or all three — in any order.
- **Tokens.** `{daysTotal} days left`, `D-{daysTotal}`, `{hh}:{mm}:{ss}`, `{months}m {weeks}w
  {days}d` — 21 tokens covering every part of the remaining time.
- **Presets.** A preset is a named snapshot of the whole look. Pick one, tweak it, save your own.
- **Live editing.** Drag a slider and the wallpaper follows it in real time.
- **Daily countdown.** A second, recurring target: a clock time (say 18:00) that every day
  counts down to, counts up past (`+00:12:34`), and resets at midnight.
- **Multi-monitor.** Every monitor gets the countdown, and any monitor can override the style,
  the position, the line list, or opt out entirely.
- **Font picker that shows the fonts.** Each family name is drawn in its own font, so Korean,
  Chinese and Japanese family names are legible instead of tofu.

## Install

Both builds are on the [latest release](https://github.com/HaJH/DesktopCountdown/releases).

### Windows

Grab `desktop-countdown.exe` and run it. It is a single self-contained binary — the C runtime is
linked statically and everything else it uses (Direct2D, DirectWrite, DirectComposition) ships
with Windows. There is nothing to install and no console window; look for the tray icon.

### macOS 11+

The app is a universal binary (Apple silicon and Intel) and lives in the menu bar. It is
ad-hoc signed but **not notarized** (notarization needs a paid Apple Developer account), so
macOS blocks the first launch — on macOS 15 and later as **"…is damaged and can't be opened"**
with only a *Move to Trash* button. It is **not damaged**; that is just how Gatekeeper reports
an app it can't tie to a registered developer. Clearing the quarantine flag once gets past it.

**Paste this into Terminal** — it downloads the latest release, unpacks it into `Applications`,
clears the flag, and launches the app:

```sh
curl -L https://github.com/HaJH/DesktopCountdown/releases/latest/download/DesktopCountdown-macos-universal.zip -o /tmp/dc.zip
ditto -x -k /tmp/dc.zip /Applications
xattr -dr com.apple.quarantine /Applications/DesktopCountdown.app
open /Applications/DesktopCountdown.app
```

**Prefer to do it by hand:**

1. Download `DesktopCountdown-macos-universal.zip` from the [latest release](https://github.com/HaJH/DesktopCountdown/releases/latest).
2. Double-click the zip in Finder to unpack `DesktopCountdown.app`.
3. Drag `DesktopCountdown.app` into your `Applications` folder.
4. The first launch is blocked — clear it either way, then open the app normally:
   - **Terminal** — run:
     ```sh
     xattr -dr com.apple.quarantine /Applications/DesktopCountdown.app
     ```
   - **System Settings → Privacy & Security** — scroll down and click **Open Anyway**. For an
     ad-hoc-signed app this button often doesn't appear; if it is missing, use the Terminal command.

### From source

You need [Rust](https://rustup.rs) (1.92+).

```
cargo build --release
```

The result is `target/release/desktop-countdown` (`.exe` on Windows; `build.bat` does the same
and can be double-clicked). On macOS a plain `cargo run` behaves like the bundled app, so there
is no need to build a bundle to try it.

## Using it

The app has no window of its own. Everything hangs off the tray / menu bar icon:

| Menu item | What it does |
|---|---|
| **Open settings** | The settings window. Closing the app closes it too. |
| **Reload config** | Re-reads `config.toml`, for when you edited it by hand. |
| **Quit** | Stops the countdown. |

Launching the app again while it is running also opens the settings window — useful when the
tray / menu bar icon is hard to reach (a crowded macOS menu bar hides overflowed icons
entirely).

In the settings window, set the target time, then pick a **preset** — a named snapshot of the
whole look, lines and style together. Editing on top of one does not change it; `Reset` puts it
back, and `Save as…` keeps your version under a name of your own. The tabs across the top switch
between the global settings and each monitor.

Every change is saved as you make it and shows on the wallpaper right away. There is no Apply
button and no undo — which is why saving a look you like under a name is worth the two seconds.

You can also edit the config by hand; it is re-read about 100 ms after you save it.

| | Config | Log |
|---|---|---|
| Windows | `%APPDATA%\DesktopCountdown\config.toml` | `%LOCALAPPDATA%\DesktopCountdown\log.txt` |
| macOS | `~/Library/Application Support/DesktopCountdown/config.toml` | `~/Library/Logs/DesktopCountdown/log.txt` |

**[Configuration reference →](docs/CONFIGURATION.md)** — every key, the full token list, presets,
and per-monitor overrides.

## Known limitations

- The countdown is part of the wallpaper, so you cannot click it. Everything is driven from the
  tray / menu bar icon and the settings window.
- At the target time the countdown stops at `00:00:00`. It does not count up (the daily
  tokens are the exception — they do) and does not notify.
- **Windows:** attaching to the wallpaper layer relies on undocumented behaviour. It re-attaches
  by itself when Explorer restarts, but a future Windows update could change the window structure
  and break it.
- **macOS:** the app is not notarized, so the first launch needs the workaround above.
- **Sharing a `config.toml` between the two systems** works, but not pixel-for-pixel. Sizes are
  points on macOS and physical pixels on Windows, and per-monitor overrides are keyed on the
  display's UUID there and on its device name here — so the global settings apply on both, and
  only the per-monitor ones go unmatched on the other system.
- **Upgrading a `config.toml` from before lines were configurable:** if it has no `[[line]]`
  section, one is filled in for you — but always with Clock only, even if the old file had
  `show_summary_line = true`. That flag no longer decides anything, so the summary line does not
  carry over automatically; pick **Summary + Clock** in the settings window afterwards if you want
  it back.

## Documentation

- **[Configuration](docs/CONFIGURATION.md)** — `config.toml` and `presets.toml`, key by key.
- **[Internals](docs/INTERNALS.md)** — how the wallpaper layer is reached on each system, and why
  the two routes have nothing in common.
- `docs/superpowers/` — design documents and implementation plans, in Korean. The record of the
  non-obvious decisions and the reasoning behind them.

## Licence

MIT. See [LICENSE](LICENSE).
