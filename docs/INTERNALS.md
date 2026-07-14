# Internals

How the countdown gets onto the wallpaper, and why it took two completely different routes to
get there.

## Two backends, one app

Windows and macOS reach the wallpaper layer by unrelated means, so the app has a backend for
each under `src/platform/`. Everything above them — the config, the tokens, the layout, the
settings window — is shared, and the backends meet it through the traits sketched at the top of
`src/platform/mod.rs`.

**Windows** draws the wallpaper in a `WorkerW` window that sits below the desktop icons. The app
finds that window (sending the undocumented `0x052C` message to `Progman` when it has to),
creates a child of it, and composes a per-pixel-alpha visual onto it with DirectComposition.

`UpdateLayeredWindow` is the obvious approach and does not work here — it returns `Ok` from a
child window and draws nothing. A spike confirmed it before the code was written
(`superpowers/plans/spike-result.md`), which is why the app carries a DirectComposition
dependency it would otherwise not need.

Explorer restarts destroy the `WorkerW`. The app notices, re-acquires it with a backoff, and
rebuilds its panels — so the countdown comes back on its own instead of vanishing until the next
launch.

**macOS** has no wallpaper window to find. A borderless `NSWindow` one level below
`kCGDesktopIconWindowLevel` *is* the wallpaper layer: it sits under the icons, ignores the mouse,
follows you across Spaces, and stays out of Mission Control, the Dock and Cmd-Tab. The whole
re-attach-with-backoff dance the Windows backend needs simply does not exist here
(`superpowers/plans/macos-spike-result.md`).

## Text

DirectWrite on one side and CoreText on the other, but the same idea on both: glyph outlines,
measured by their *ink* rather than their line boxes, so the gap between lines is the gap you
asked for in any font — including CJK fonts, whose ascent is sized for Hangul while a countdown
only ever draws digits.

The font picker in the settings window renders each family name *in that family*, which egui
does not do on its own: it rasterizes its own text and has no OS font fallback. The names are
resolved to font files through DirectWrite / CoreText and registered with egui lazily, only for
the rows actually on screen — registering all ~300 families at once costs about a gigabyte.

## Two processes

The renderer and the settings window are the same executable. `desktop-countdown --settings`
skips the DPI setup, the renderer's single-instance lock and the wallpaper surface, and opens an
egui window instead.

They talk through `config.toml` and nothing else. The settings window writes it (atomically —
temp file plus rename, so a reader never catches it half-written), and the renderer watches the
file and repaints. That is the whole protocol: there is no IPC, no shared memory, no socket.

It also means there is no draft state. Every widget in the settings window saves as you touch it,
throttled to one write per 100 ms, and the wallpaper follows a slider drag live. What looks like
a preview *is* the thing.

Presets are the one thing that does not go through `config.toml`. They live in `presets.toml`,
which the renderer does not read at all — the preset library has no business in the config the
countdown is drawn from. `config.toml` carries only the *name* of the active preset, and the
renderer ignores that too.

## Where the decisions are written down

`docs/superpowers/` holds the design documents and implementation plans, in Korean. They are the
record of the non-obvious calls and the reasoning behind them — why DirectComposition, how the
wallpaper window is found, how the line list replaced the old fixed two-line layout, why a preset
is a snapshot of the whole look rather than just a layout.
