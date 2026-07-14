# 프리셋 모델 구현 계획 (계획 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 프리셋을 "라인 + 스타일 스냅샷"으로 일반화하고, 그 위에 비파괴적·휘발성 오버라이드를 얹을 수 있게 하며, 사용자가 직접 프리셋을 저장·삭제할 수 있게 한다.

**Architecture:** `config.toml`에는 지금처럼 완전히 해석된 `[[line]]`·`[style]`이 들어가고, 최상위에 `preset` 이름표 한 줄만 더한다. 렌더러는 그 필드를 읽지 않으므로 **렌더러 코드는 변경되지 않는다.** 활성 프리셋과 수정 여부는 저장하지 않고 매 프레임 계산한다. 사용자 프리셋 정의는 `config.toml` 옆 `presets.toml`에 따로 두어, 프리셋을 저장·삭제해도 렌더러가 재로드하지 않는다.

**Tech Stack:** Rust, serde + toml, egui/eframe. 설계 문서: `docs/superpowers/specs/2026-07-14-preset-model-design.md`

## Global Constraints

- 코드와 코드 주석은 영어. 문서·커밋 메시지는 한국어.
- 커밋 메시지에 자동 생성 문구(Co-Authored-By 등) 금지.
- 렌더러(`src/platform/**`, `src/app.rs`, `src/countdown.rs`, `src/layout.rs`) 는 이 계획에서 **한 줄도 바뀌지 않는다.** 바뀐다면 설계를 잘못 이해한 것이다.
- TOML 직렬화 제약: **스칼라 필드는 테이블/테이블배열 필드보다 앞에 와야 한다.** `toml::to_string_pretty`는 구조체 필드 순서대로 쓰므로, 스칼라를 테이블 뒤에 두면 파싱 불가능한 파일이 나온다. `DisplayOverride::lines`에 이미 같은 취지의 주석이 달려 있다.
- 레거시 필드 `Style::show_summary_line` / `DisplayOverride::show_summary_line`은 **지우지 않는다.** 두 구조체 모두 `#[serde(deny_unknown_fields)]`라, 지우면 그 필드를 가진 기존 `config.toml`이 파싱 단계에서 거부된다.
- 검증 명령: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

---

## 파일 구조

| 파일 | 책임 | 상태 |
| --- | --- | --- |
| `src/config/schema.rs` | `Config::preset` 이름표 필드, 기본 라인 목록, `migrate` | 수정 |
| `src/config/io.rs` | 테스트만 수정 | 수정 |
| `src/paths.rs` | `presets_path()` 추가 | 수정 |
| `src/settings/presets.rs` | 프리셋 모델·내장 목록·`Library`·활성 판정. 순수 로직, egui 없음 | **신규** |
| `src/settings/presets_io.rs` | `presets.toml` 읽기/쓰기 | **신규** |
| `src/settings/lines.rs` | 줄 재정렬·추가·삭제만 남긴다 (프리셋은 이사) | 수정 |
| `src/settings/mod.rs` | 새 모듈 두 개 선언 | 수정 |
| `src/settings/app.rs` | 프리셋 바 UI, `lines_editor`에서 프리셋 제거 | 수정 |
| `README.md` | 프리셋 문단 | 수정 |

`presets.rs`(순수 모델)와 `presets_io.rs`(파일 I/O)를 나누는 것은 `config/schema.rs` + `config/io.rs`가 이미 쓰는 분리를 그대로 따르는 것이다.

---

### Task 1: config 스키마 — 기본값을 시계 한 줄로, `preset` 이름표 추가

**Files:**
- Modify: `src/config/schema.rs`
- Modify: `src/config/io.rs` (테스트만)
- Modify: `src/settings/overrides.rs` (테스트만)
- Modify: `src/settings/lines.rs` (테스트만 — 프리셋 이사는 Task 2)

**Interfaces:**
- Consumes: 없음 (첫 태스크)
- Produces:
  - `config::default_lines() -> Vec<Line>` — **인자 없음** (기존 `default_lines(summary: bool)` 대체)
  - `config::Config::preset: Option<String>`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`src/config/schema.rs`의 `mod tests` 안에서, 기존 테스트 세 개를 아래로 **교체**한다.

교체 대상: `default_config_has_the_classic_two_lines`, `migrate_synthesizes_the_classic_list_for_a_config_without_lines`, `migrate_honours_the_legacy_summary_flag`.

```rust
    #[test]
    fn the_default_config_is_a_single_clock_line() {
        let cfg = Config::default();
        assert_eq!(cfg.lines, vec![Line {
            text: "{hh}:{mm}:{ss}".to_string(),
            size_ratio: 1.0,
            align: Align::Center,
            color: None,
        }]);
        assert_eq!(cfg.preset, Some("Clock only".to_string()));
    }

    #[test]
    fn migrate_fills_a_config_without_lines_with_the_default_list() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert!(cfg.lines.is_empty());
        migrate(&mut cfg);
        assert_eq!(cfg.lines, default_lines());
    }

    /// The legacy flag no longer decides anything: a config written before `[[line]]`
    /// existed gets the current default, whatever it said about the summary line.
    #[test]
    fn migrate_ignores_the_legacy_summary_flag() {
        let mut cfg: Config = toml::from_str(
            "target = \"2026-10-24T09:00:00\"\n[style]\nshow_summary_line = true\n",
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.lines, default_lines());
        assert_eq!(cfg.lines.len(), 1);
        assert_eq!(cfg.lines[0].text, "{hh}:{mm}:{ss}");
    }

    /// The label is not serialized when absent, so an old file that never had one does
    /// not gain an empty key on the next save.
    #[test]
    fn a_config_without_a_preset_label_round_trips_without_one() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(cfg.preset, None);
        migrate(&mut cfg);
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(!text.contains("preset"), "unexpected preset key in:\n{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.preset, None);
    }

    #[test]
    fn a_preset_label_round_trips() {
        let mut cfg = Config::default();
        cfg.preset = Some("My look".to_string());
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.preset, Some("My look".to_string()));
    }
```

`migrate_converts_a_legacy_per_monitor_summary_flag_into_a_line_list`도 성립하지 않으므로 아래로 교체한다.

```rust
    /// A monitor's legacy flag no longer synthesizes a line list either. The monitor keeps
    /// no list of its own, which means it follows the global one -- the same thing the flag
    /// used to mean when it matched the global setting, and a harmless default when it did not.
    #[test]
    fn migrate_leaves_a_monitor_with_only_a_legacy_summary_flag_following_the_globals() {
        let mut cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "MON-A"
show_summary_line = false
"#,
        )
        .unwrap();
        migrate(&mut cfg);
        assert_eq!(cfg.displays[0].lines, None);
    }
```

- [ ] **Step 2: 실패를 확인한다**

Run: `cargo test --lib config::schema`
Expected: FAIL — `default_lines` takes 1 argument, `Config` has no field `preset`

- [ ] **Step 3: 스키마를 고친다**

`src/config/schema.rs`에서 `default_lines`를 아래로 교체한다 (기존 86~90행의 `SUMMARY_*` 상수는 프리셋이 계속 쓰므로 **남긴다**; 주석만 고친다).

```rust
/// The summary line's em size as a fraction of `size_px`. Hard-coded in the renderer as
/// `SUMMARY_RATIO` before lines became configurable; now the `Summary + Clock` preset's value.
pub const SUMMARY_SIZE_RATIO: f32 = 0.28;
pub const SUMMARY_TEMPLATE: &str = "{months}m {weeks}w {days}d";
pub const MAIN_TEMPLATE: &str = "{hh}:{mm}:{ss}";
```

```rust
/// The line list a fresh config starts with, and what `migrate` fills an empty one with:
/// the clock, on its own. Everything richer is a preset the user picks.
pub fn default_lines() -> Vec<Line> {
    vec![Line {
        text: MAIN_TEMPLATE.to_string(),
        ..Line::default()
    }]
}

/// The preset `default_lines` corresponds to. `Config::default` labels itself with it so a
/// fresh install opens the settings window already sitting on a named preset rather than on
/// `Custom`. Must stay in step with `settings::presets::builtin`, which the test
/// `the_default_config_matches_its_own_label` in that module checks.
pub const DEFAULT_PRESET: &str = "Clock only";
```

`Config`에 이름표 필드를 더한다. **`target` 바로 뒤**여야 한다 — 스칼라는 테이블보다 앞에 와야 TOML이 다시 파싱된다.

```rust
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "d_target")]
    pub target: DateTime,
    /// Which preset the current lines and style came from. Written and read only by the
    /// settings window -- the renderer draws from the fully-resolved `[[line]]` and
    /// `[style]` below and never looks at this. A name that no longer exists (the preset
    /// was deleted) or a missing label is not an error: the settings window recovers a
    /// name by matching the current lines and style against the presets it knows, and
    /// falls back to `Custom`.
    ///
    /// Must stay above `[style]`: TOML rejects a scalar written after a table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub general: General,
    /// The lines drawn, top to bottom. An empty list means "not configured" — `migrate` fills
    /// it in; it never means "draw nothing".
    #[serde(default, rename = "line", skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<Line>,
    #[serde(default, rename = "display", skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<DisplayOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: d_target(),
            preset: Some(DEFAULT_PRESET.to_string()),
            style: Style::default(),
            layout: Layout::default(),
            general: General::default(),
            lines: default_lines(),
            displays: Vec::new(),
        }
    }
}
```

`migrate`를 단순화한다.

```rust
/// Fills in the `[[line]]` list for a config written before lines existed (or hand-edited to
/// have none). The list is the current default; the legacy `show_summary_line` flag no longer
/// decides anything.
pub fn migrate(cfg: &mut Config) {
    if cfg.lines.is_empty() {
        cfg.lines = default_lines();
    }
}
```

레거시 필드 두 개의 주석을 고친다. `Style::show_summary_line` (기존 173~178행):

```rust
    /// Legacy. Older config files carried this flag instead of a `[[line]]` list. Nothing
    /// reads it any more -- not even `migrate`, which now fills an empty list with the
    /// current default regardless. The field stays only because `deny_unknown_fields` would
    /// otherwise reject an existing file that still names it. Never written back
    /// (`skip_serializing`), so it cannot leak into a file that does not already have it.
    #[serde(default, skip_serializing)]
    pub show_summary_line: Option<bool>,
```

`DisplayOverride::show_summary_line` (기존 261~264행):

```rust
    /// Legacy, as in `Style`. Accepted so an existing config.toml still parses; never read,
    /// never written back.
    #[serde(default, skip_serializing)]
    pub show_summary_line: Option<bool>,
```

- [ ] **Step 4: 호출자를 고친다**

`src/config/io.rs`의 테스트 `a_legacy_file_without_lines_gains_the_classic_list_on_load`를 교체한다.

```rust
    #[test]
    fn a_legacy_file_without_lines_gains_the_default_list_on_load() {
        let p = tmp("legacy");
        fs::write(
            &p,
            "target = \"2030-01-01T00:00:00\"\n[style]\nshow_summary_line = false\n",
        )
        .unwrap();
        let cfg = load_or_create(&p).unwrap();
        assert_eq!(cfg.lines, crate::config::default_lines());
    }
```

`src/settings/overrides.rs`의 테스트에서 `default_lines(true)` → `default_lines()` (197행 근처).

```rust
            |o| o.lines = Some(crate::config::default_lines()),
```

`src/settings/lines.rs`의 테스트 `the_classic_preset_is_the_default_list`를 **삭제**한다. Task 2에서 프리셋이 이 모듈을 떠나므로 여기서 되살리지 않는다.

- [ ] **Step 5: 테스트를 통과시킨다**

Run: `cargo test`
Expected: PASS (전부)

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 경고 없음

- [ ] **Step 6: 커밋**

```bash
git add src/config/schema.rs src/config/io.rs src/settings/overrides.rs src/settings/lines.rs
git commit -m "feat(config): 기본 라인 목록을 시계 한 줄로, preset 이름표 필드 추가

- default_lines()에서 summary bool 인자 제거, migrate가 레거시 플래그를 더 이상 읽지 않음
- Config::preset — 설정 창 전용 이름표. 렌더러는 읽지 않음"
```

---

### Task 2: 프리셋 모델과 내장 목록을 새 모듈로

**Files:**
- Create: `src/settings/presets.rs`
- Modify: `src/settings/lines.rs` (프리셋 제거)
- Modify: `src/settings/mod.rs`

**Interfaces:**
- Consumes: `config::default_lines()`, `config::DEFAULT_PRESET`, `config::{Line, Style, Align, MAIN_TEMPLATE, SUMMARY_TEMPLATE, SUMMARY_SIZE_RATIO}`
- Produces:
  - `settings::presets::Preset { name: String, style: Style, lines: Vec<Line> }` (필드 순서 그대로 — TOML 제약)
  - `settings::presets::builtin() -> Vec<Preset>`
  - `settings::presets::BUILTIN_COUNT: usize`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`src/settings/presets.rs`를 새로 만들고 아래를 넣는다 (본문은 아직 비어 있다).

```rust
//! The preset model: a named snapshot of the whole look (lines + style), the built-in list,
//! and the library the settings window picks from. Pure logic, no egui, no I/O.

use crate::config::{
    Line, Style, DEFAULT_PRESET, MAIN_TEMPLATE, SUMMARY_SIZE_RATIO, SUMMARY_TEMPLATE,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_only_comes_first() {
        assert_eq!(builtin()[0].name, "Clock only");
    }

    /// `Config::default` labels itself `DEFAULT_PRESET`, and the settings window resolves
    /// that label against this list. If the two drift apart, a fresh install opens on
    /// `Custom` -- which is exactly the confusing state the label exists to prevent.
    #[test]
    fn the_default_config_matches_its_own_label() {
        let cfg = crate::config::Config::default();
        let labelled = builtin()
            .into_iter()
            .find(|p| p.name == DEFAULT_PRESET)
            .expect("DEFAULT_PRESET names a built-in preset");
        assert_eq!(labelled.lines, cfg.lines);
        assert_eq!(labelled.style, cfg.style);
    }

    #[test]
    fn there_is_no_preset_called_classic() {
        assert!(
            !builtin().iter().any(|p| p.name == "Classic"),
            "Classic was renamed to Summary + Clock"
        );
    }

    #[test]
    fn summary_plus_clock_is_the_old_classic_pair() {
        let p = builtin()
            .into_iter()
            .find(|p| p.name == "Summary + Clock")
            .expect("Summary + Clock preset");
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.lines[0].text, SUMMARY_TEMPLATE);
        assert_eq!(p.lines[0].size_ratio, SUMMARY_SIZE_RATIO);
        assert_eq!(p.lines[1].text, MAIN_TEMPLATE);
        assert_eq!(p.lines[1].size_ratio, 1.0);
    }

    #[test]
    fn every_builtin_carries_the_default_style_and_at_least_one_line() {
        for p in builtin() {
            assert!(!p.lines.is_empty(), "{} built nothing", p.name);
            assert_eq!(p.style, Style::default(), "{} is not on the default style", p.name);
            for line in &p.lines {
                assert!(line.size_ratio > 0.0);
            }
        }
    }

    #[test]
    fn builtin_names_are_unique() {
        let mut names: Vec<String> = builtin().into_iter().map(|p| p.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "duplicate built-in preset name");
    }

    #[test]
    fn builtin_count_matches_the_list() {
        assert_eq!(builtin().len(), BUILTIN_COUNT);
    }
}
```

- [ ] **Step 2: 실패를 확인한다**

`src/settings/mod.rs`에 모듈을 먼저 등록해야 컴파일된다.

```rust
pub mod app;
pub mod lines;
pub mod overrides;
pub mod presets;
pub mod widgets;
```

Run: `cargo test --lib settings::presets`
Expected: FAIL — cannot find function `builtin`

- [ ] **Step 3: 모델과 내장 목록을 쓴다**

`src/settings/presets.rs`의 `use` 아래, `mod tests` 위에 넣는다.

```rust
/// A named snapshot of the whole look: the line list *and* the shared style. Picking a preset
/// replaces both -- so a preset is a look, not just a layout, and switching one away discards
/// any unsaved tweaks on top of it.
///
/// Field order is load-bearing: `name` is a scalar and must be serialized before `style` (a
/// table) and `lines` (an array of tables), or the `presets.toml` this writes will not parse
/// back. Same constraint as `config::DisplayOverride::lines`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub style: Style,
    #[serde(default, rename = "line")]
    pub lines: Vec<Line>,
}

pub const BUILTIN_COUNT: usize = 5;

/// The presets that ship with the app. `Clock only` is first: it is what a fresh config holds
/// (`config::DEFAULT_PRESET`), and the picker shows the list in this order.
///
/// All five carry `Style::default()`. That makes picking one a way back to the stock look as
/// well as to a layout -- a recovery point, not a stray side effect.
pub fn builtin() -> Vec<Preset> {
    fn preset(name: &str, lines: &[(&str, f32)]) -> Preset {
        Preset {
            name: name.to_string(),
            style: Style::default(),
            lines: lines
                .iter()
                .map(|(text, size_ratio)| Line {
                    text: (*text).to_string(),
                    size_ratio: *size_ratio,
                    ..Line::default()
                })
                .collect(),
        }
    }

    vec![
        preset(DEFAULT_PRESET, &[(MAIN_TEMPLATE, 1.0)]),
        preset(
            "Summary + Clock",
            &[(SUMMARY_TEMPLATE, SUMMARY_SIZE_RATIO), (MAIN_TEMPLATE, 1.0)],
        ),
        preset("D-Day", &[("D-{daysTotal}", 1.0), (MAIN_TEMPLATE, 0.3)]),
        preset(
            "Days left",
            &[("{daysTotal} days left", 0.35), (MAIN_TEMPLATE, 1.0)],
        ),
        preset(
            "Caption + Clock",
            &[
                ("Deadline", 0.25),
                (MAIN_TEMPLATE, 1.0),
                ("{daysTotal} days left", 0.25),
            ],
        ),
    ]
}
```

- [ ] **Step 4: 옛 프리셋을 `lines.rs`에서 걷어낸다**

`src/settings/lines.rs`에서 `Preset` 구조체, `impl Preset`, `PRESETS` 상수, 그리고 테스트 `every_preset_builds_at_least_one_line_and_leaves_the_rest_at_the_defaults`를 **삭제**한다. 파일 머리와 `use`, `remove`의 주석을 고친다.

```rust
//! Pure line-list editing for the settings window: reordering, adding, removing. No egui.
//! Presets live in `settings::presets`.

use crate::config::Line;
```

```rust
/// Drops line `i`, unless it is the only one: an empty list reads as "not configured", and
/// `config::migrate` would refill it with the default list on the next load. A monitor is
/// silenced with `enabled = false`, not by emptying its line list.
pub fn remove(lines: &mut Vec<Line>, i: usize) {
```

`lines.rs`의 `mod tests`는 `use super::*;`만으로 `Align`을 더 이상 쓰지 않으므로, `add_appends_a_blank_line_at_the_base_size`를 포함한 나머지 테스트는 그대로 통과해야 한다. 컴파일 오류가 나면 쓰지 않는 `use`를 지운다.

- [ ] **Step 5: 테스트를 통과시킨다**

Run: `cargo test`
Expected: FAIL — `src/settings/app.rs`가 아직 `lines::PRESETS`를 참조한다

이 컴파일 오류를 막기 위해, `app.rs`의 `lines_editor`에서 프리셋 콤보 블록(현재 837~861행, `let preset_id = ...`부터 `ui.ctx().data_mut(|d| d.insert_temp(preset_id, chosen));`까지)과 그 위의 주석을 통째로 **삭제**한다. 프리셋 바는 Task 5에서 다시 붙인다. 함수 머리 주석도 고친다.

```rust
/// The line-list editor: the token reference and one row per line. Shared by the global list
/// and a monitor override's list (`salt` keeps their widget ids apart). The preset bar is not
/// here -- it is global-only, and `SettingsApp::ui_preset_bar` draws it.
/// Returns whether anything changed this frame.
fn lines_editor(ui: &mut egui::Ui, list: &mut Vec<Line>, salt: &str) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new("Available tokens")
```

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS, 경고 없음

이 시점의 설정 창에는 프리셋 UI가 아예 없다 — 의도된 중간 상태다.

- [ ] **Step 6: 커밋**

```bash
git add src/settings/presets.rs src/settings/lines.rs src/settings/mod.rs src/settings/app.rs
git commit -m "refactor(settings): 프리셋을 라인+스타일 스냅샷으로 재정의

- settings::presets 신설. Preset이 style을 함께 담고 내장 5개는 Style::default()
- Classic을 Summary + Clock으로 개명, Clock only를 맨 앞으로
- lines_editor에서 프리셋 콤보/Apply 제거 (프리셋 바는 후속 태스크)"
```

---

### Task 3: 활성 프리셋 판정 (`Library` + `Active`)

**Files:**
- Modify: `src/settings/presets.rs`

**Interfaces:**
- Consumes: `presets::{Preset, builtin, BUILTIN_COUNT}`
- Produces:
  - `presets::Active { Clean(usize), Modified(usize), Custom }`
  - `presets::Library::new(user: Vec<Preset>) -> Library`
  - `Library::all(&self) -> &[Preset]`
  - `Library::user(&self) -> &[Preset]`
  - `Library::is_builtin(&self, i: usize) -> bool`
  - `Library::find(&self, name: &str) -> Option<usize>`
  - `Library::resolve(&self, label: Option<&str>, lines: &[Line], style: &Style) -> Active`
  - `Library::apply(&self, i: usize, cfg: &mut Config)`
  - `presets::style_eq(a: &Style, b: &Style) -> bool`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`src/settings/presets.rs`의 `mod tests`에 덧붙인다.

```rust
    fn lib() -> Library {
        Library::new(vec![Preset {
            name: "Mine".to_string(),
            style: Style {
                size_px: 99.0,
                ..Style::default()
            },
            lines: vec![Line {
                text: "hi".to_string(),
                ..Line::default()
            }],
        }])
    }

    #[test]
    fn user_presets_come_after_the_builtins() {
        let l = lib();
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);
        assert!(l.is_builtin(0));
        assert!(!l.is_builtin(BUILTIN_COUNT));
        assert_eq!(l.user().len(), 1);
        assert_eq!(l.user()[0].name, "Mine");
    }

    #[test]
    fn a_label_whose_look_matches_resolves_clean() {
        let l = lib();
        let p = &l.all()[0];
        assert_eq!(
            l.resolve(Some(&p.name), &p.lines, &p.style),
            Active::Clean(0)
        );
    }

    #[test]
    fn a_label_whose_lines_differ_resolves_modified() {
        let l = lib();
        let p = l.all()[0].clone();
        let edited = vec![Line {
            text: "edited".to_string(),
            ..Line::default()
        }];
        assert_eq!(
            l.resolve(Some(&p.name), &edited, &p.style),
            Active::Modified(0)
        );
    }

    /// The preset carries the style too, so a style-only edit is just as much a modification
    /// as a line edit. This is the case a lines-only preset model would have missed.
    #[test]
    fn a_label_whose_style_differs_resolves_modified() {
        let l = lib();
        let p = l.all()[0].clone();
        let restyled = Style {
            size_px: 12.0,
            ..p.style.clone()
        };
        assert_eq!(
            l.resolve(Some(&p.name), &p.lines, &restyled),
            Active::Modified(0)
        );
    }

    /// No label (an old config.toml, or a hand-edited one) is not `Custom` on its own: the
    /// look is matched against the list and gets its name back. This is what lets the
    /// migration carry no code at all.
    #[test]
    fn a_missing_label_recovers_the_name_from_the_look() {
        let l = lib();
        let p = l.all()[1].clone();
        assert_eq!(l.resolve(None, &p.lines, &p.style), Active::Clean(1));
    }

    #[test]
    fn a_missing_label_with_no_matching_look_is_custom() {
        let l = lib();
        let odd = vec![Line {
            text: "nothing like a preset".to_string(),
            ..Line::default()
        }];
        assert_eq!(l.resolve(None, &odd, &Style::default()), Active::Custom);
    }

    /// A deleted preset leaves its name behind in config.toml. That is not an error.
    #[test]
    fn a_label_naming_no_preset_falls_back_to_matching_the_look() {
        let l = lib();
        let p = l.all()[2].clone();
        assert_eq!(l.resolve(Some("gone"), &p.lines, &p.style), Active::Clean(2));

        let odd = vec![Line {
            text: "nothing like a preset".to_string(),
            ..Line::default()
        }];
        assert_eq!(l.resolve(Some("gone"), &odd, &Style::default()), Active::Custom);
    }

    /// The legacy flag only ever exists on a config loaded from an old file; no preset carries
    /// it. Comparing it would make such a config permanently `Modified` against every preset.
    #[test]
    fn the_legacy_summary_flag_is_ignored_when_comparing_styles() {
        let legacy = Style {
            show_summary_line: Some(false),
            ..Style::default()
        };
        assert!(style_eq(&legacy, &Style::default()));
        assert_ne!(legacy, Style::default(), "the derived PartialEq still sees it");
    }

    #[test]
    fn apply_replaces_lines_and_style_and_moves_the_label() {
        let l = lib();
        let mut cfg = crate::config::Config::default();
        cfg.style.size_px = 11.0;
        let i = BUILTIN_COUNT; // "Mine"
        l.apply(i, &mut cfg);
        assert_eq!(cfg.lines, l.all()[i].lines);
        assert_eq!(cfg.style.size_px, 99.0);
        assert_eq!(cfg.preset, Some("Mine".to_string()));
        assert_eq!(l.resolve(cfg.preset.as_deref(), &cfg.lines, &cfg.style), Active::Clean(i));
    }
```

- [ ] **Step 2: 실패를 확인한다**

Run: `cargo test --lib settings::presets`
Expected: FAIL — cannot find type `Library`, `Active`, function `style_eq`

- [ ] **Step 3: 구현한다**

`src/settings/presets.rs`의 `builtin()` 아래에 넣는다. `use`에 `Config`를 더한다.

```rust
use crate::config::{
    Config, Line, Style, DEFAULT_PRESET, MAIN_TEMPLATE, SUMMARY_SIZE_RATIO, SUMMARY_TEMPLATE,
};
```

```rust
/// Which preset the current look sits on, and whether anything has been changed on top of it.
/// Computed every frame from the config -- never stored, so it cannot go stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Active {
    /// Exactly the preset at this index.
    Clean(usize),
    /// Started from the preset at this index; edits are layered on top.
    Modified(usize),
    /// Matches no preset, and no label points anywhere useful.
    Custom,
}

/// `Style` derives `PartialEq` over every field, including the legacy `show_summary_line`,
/// which only ever holds a value on a `Style` parsed from an old config.toml -- no preset
/// carries it. Comparing it would leave such a config `Modified` against every preset,
/// forever, over a field nothing reads.
pub fn style_eq(a: &Style, b: &Style) -> bool {
    let strip = |s: &Style| Style {
        show_summary_line: None,
        ..s.clone()
    };
    strip(a) == strip(b)
}

/// The presets the picker offers: the built-ins, then the user's own. Index into `all()` is
/// the picker's currency -- `Active` carries one, and so does `apply`.
pub struct Library {
    all: Vec<Preset>,
    n_builtin: usize,
}

impl Library {
    pub fn new(user: Vec<Preset>) -> Self {
        let mut all = builtin();
        let n_builtin = all.len();
        all.extend(user);
        Self { all, n_builtin }
    }

    pub fn all(&self) -> &[Preset] {
        &self.all
    }

    /// The user's own presets -- what `presets_io::save` writes. The built-ins are not saved.
    pub fn user(&self) -> &[Preset] {
        &self.all[self.n_builtin..]
    }

    pub fn is_builtin(&self, i: usize) -> bool {
        i < self.n_builtin
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.all.iter().position(|p| p.name == name)
    }

    /// The label is a hint, not the truth. When it names a preset, the current look is compared
    /// against that one and the answer is `Clean` or `Modified`. When it does not (missing from
    /// an old file, or naming a preset since deleted), the look itself is matched against the
    /// whole list -- so a config that happens to be exactly a preset gets its name back rather
    /// than reading `Custom`.
    pub fn resolve(&self, label: Option<&str>, lines: &[Line], style: &Style) -> Active {
        if let Some(i) = label.and_then(|n| self.find(n)) {
            return if self.matches(i, lines, style) {
                Active::Clean(i)
            } else {
                Active::Modified(i)
            };
        }
        match (0..self.all.len()).find(|&i| self.matches(i, lines, style)) {
            Some(i) => Active::Clean(i),
            None => Active::Custom,
        }
    }

    fn matches(&self, i: usize, lines: &[Line], style: &Style) -> bool {
        let p = &self.all[i];
        p.lines == lines && style_eq(&p.style, style)
    }

    /// Drops the preset's whole look onto the config and moves the label to it. Everything the
    /// user had layered on top is gone -- the caller is what guards that (see the settings
    /// window's discard prompt).
    pub fn apply(&self, i: usize, cfg: &mut Config) {
        let p = &self.all[i];
        cfg.lines = p.lines.clone();
        cfg.style = p.style.clone();
        cfg.preset = Some(p.name.clone());
    }
}
```

- [ ] **Step 4: 테스트를 통과시킨다**

Run: `cargo test --lib settings::presets`
Expected: PASS

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 경고 없음

- [ ] **Step 5: 커밋**

```bash
git add src/settings/presets.rs
git commit -m "feat(settings): 활성 프리셋 판정 (Library, Active)

- 이름표가 없거나 깨져도 현재 모양을 프리셋 목록과 대조해 이름을 복구
- 레거시 show_summary_line은 스타일 비교에서 제외"
```

---

### Task 4: `presets.toml` 저장소

**Files:**
- Create: `src/settings/presets_io.rs`
- Modify: `src/settings/presets.rs` (`save_as` / `delete`)
- Modify: `src/settings/mod.rs`
- Modify: `src/paths.rs`

**Interfaces:**
- Consumes: `presets::{Preset, Library}`, `config::validate`
- Produces:
  - `paths::presets_path() -> Result<PathBuf>`
  - `presets_io::load(path: &Path) -> Vec<Preset>` — 없거나 깨진 파일은 빈 목록 + 경고 로그
  - `presets_io::save(path: &Path, user: &[Preset]) -> Result<()>`
  - `presets::NameStatus { Empty, Builtin, Overwrite, New }`
  - `Library::check_name(&self, name: &str) -> NameStatus`
  - `Library::save_as(&mut self, name: &str, lines: &[Line], style: &Style) -> usize`
  - `Library::delete(&mut self, i: usize) -> bool`

- [ ] **Step 1: 실패하는 테스트를 쓴다 — 라이브러리 변경**

`src/settings/presets.rs`의 `mod tests`에 덧붙인다.

```rust
    #[test]
    fn check_name_rejects_the_empty_string_and_builtin_names() {
        let l = lib();
        assert_eq!(l.check_name(""), NameStatus::Empty);
        assert_eq!(l.check_name("   "), NameStatus::Empty);
        assert_eq!(l.check_name("Clock only"), NameStatus::Builtin);
        assert_eq!(l.check_name("Mine"), NameStatus::Overwrite);
        assert_eq!(l.check_name("Fresh"), NameStatus::New);
    }

    #[test]
    fn save_as_appends_a_user_preset_and_returns_its_index() {
        let mut l = lib();
        let lines = vec![Line {
            text: "saved".to_string(),
            ..Line::default()
        }];
        let style = Style {
            opacity: 0.5,
            ..Style::default()
        };
        let i = l.save_as("Fresh", &lines, &style);
        assert_eq!(i, BUILTIN_COUNT + 1);
        assert_eq!(l.user().len(), 2);
        assert_eq!(l.all()[i].name, "Fresh");
        assert_eq!(l.resolve(Some("Fresh"), &lines, &style), Active::Clean(i));
    }

    #[test]
    fn save_as_over_an_existing_user_preset_replaces_it_in_place() {
        let mut l = lib();
        let lines = vec![Line {
            text: "replaced".to_string(),
            ..Line::default()
        }];
        let i = l.save_as("Mine", &lines, &Style::default());
        assert_eq!(i, BUILTIN_COUNT, "kept its slot");
        assert_eq!(l.user().len(), 1, "no duplicate");
        assert_eq!(l.all()[i].lines, lines);
    }

    #[test]
    fn delete_drops_a_user_preset_and_refuses_a_builtin() {
        let mut l = lib();
        assert!(!l.delete(0), "built-ins cannot be deleted");
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);

        assert!(l.delete(BUILTIN_COUNT));
        assert_eq!(l.all().len(), BUILTIN_COUNT);
        assert!(l.user().is_empty());
    }

    #[test]
    fn delete_out_of_range_is_ignored() {
        let mut l = lib();
        assert!(!l.delete(999));
        assert_eq!(l.all().len(), BUILTIN_COUNT + 1);
    }
```

- [ ] **Step 2: 실패를 확인한다**

Run: `cargo test --lib settings::presets`
Expected: FAIL — no method `check_name`, `save_as`, `delete`

- [ ] **Step 3: 라이브러리 변경을 구현한다**

`src/settings/presets.rs`의 `impl Library` 안에 덧붙인다.

```rust
    /// What saving under `name` would do. The settings window uses this to label its Save
    /// button and to block the two names it must not take.
    pub fn check_name(&self, name: &str) -> NameStatus {
        let name = name.trim();
        if name.is_empty() {
            return NameStatus::Empty;
        }
        match self.find(name) {
            Some(i) if self.is_builtin(i) => NameStatus::Builtin,
            Some(_) => NameStatus::Overwrite,
            None => NameStatus::New,
        }
    }

    /// Stores the current look under `name` and returns its index in `all()`. An existing user
    /// preset of that name is replaced in place, keeping its slot -- the caller has already
    /// confirmed the overwrite (`NameStatus::Overwrite`).
    ///
    /// Callers must not pass a built-in's name; `check_name` is what rejects it. Doing so
    /// anyway appends a second preset with a duplicate name rather than corrupting a built-in.
    pub fn save_as(&mut self, name: &str, lines: &[Line], style: &Style) -> usize {
        let name = name.trim().to_string();
        let preset = Preset {
            name: name.clone(),
            style: style.clone(),
            lines: lines.to_vec(),
        };
        match self.find(&name) {
            Some(i) if !self.is_builtin(i) => {
                self.all[i] = preset;
                i
            }
            _ => {
                self.all.push(preset);
                self.all.len() - 1
            }
        }
    }

    /// Removes a user preset. Built-ins and out-of-range indices are refused (`false`), so a
    /// stale index from a previous frame cannot delete the wrong thing.
    ///
    /// The caller keeps the config's lines and style as they are and only drops the label --
    /// deleting a preset must not change what is on the wallpaper.
    pub fn delete(&mut self, i: usize) -> bool {
        if self.is_builtin(i) || i >= self.all.len() {
            return false;
        }
        self.all.remove(i);
        true
    }
```

`Active` 아래에 넣는다.

```rust
/// What `Library::check_name` makes of a name typed into the Save-as box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStatus {
    /// Nothing, or only whitespace.
    Empty,
    /// A built-in's name. Built-ins cannot be overwritten.
    Builtin,
    /// An existing user preset. Saving replaces it.
    Overwrite,
    /// Free.
    New,
}
```

- [ ] **Step 4: 라이브러리 테스트를 통과시킨다**

Run: `cargo test --lib settings::presets`
Expected: PASS

- [ ] **Step 5: 실패하는 I/O 테스트를 쓴다**

`src/settings/presets_io.rs`를 새로 만든다.

```rust
//! Loading and saving `presets.toml`, the user's own preset library.
//!
//! Deliberately not `config.toml`: the renderer watches that file and redraws when it changes.
//! Saving or deleting a preset changes nothing on the wallpaper, so it must not land in a file
//! that would make the renderer reload. Nothing outside the settings window reads this one.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{self, Config};
use crate::settings::presets::Preset;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default, rename = "preset")]
    presets: Vec<Preset>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Line, Style};
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-presets-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("presets.toml");
        p
    }

    fn sample() -> Preset {
        Preset {
            name: "My look".to_string(),
            style: Style {
                size_px: 96.0,
                opacity: 0.5,
                ..Style::default()
            },
            lines: vec![
                Line {
                    text: "Deadline".to_string(),
                    size_ratio: 0.25,
                    ..Line::default()
                },
                Line {
                    text: "{hh}:{mm}:{ss}".to_string(),
                    ..Line::default()
                },
            ],
        }
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_library() {
        let p = tmp("missing");
        assert!(!p.exists());
        assert!(load(&p).is_empty());
        assert!(!p.exists(), "loading must not create the file");
    }

    #[test]
    fn presets_round_trip_through_the_file() {
        let p = tmp("round-trip");
        save(&p, &[sample()]).unwrap();
        assert_eq!(load(&p), vec![sample()]);
    }

    #[test]
    fn saving_an_empty_library_leaves_a_file_that_loads_as_empty() {
        let p = tmp("empty");
        save(&p, &[]).unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn a_malformed_file_loads_as_an_empty_library_rather_than_failing() {
        let p = tmp("malformed");
        fs::write(&p, "[[preset\nname = \"broken\"\n").unwrap();
        assert!(load(&p).is_empty());
    }

    /// A hand-edited preset with a value the renderer would refuse is dropped, not applied:
    /// applying it would leave the settings window stuck on "Invalid config" with no way back.
    #[test]
    fn a_preset_that_would_not_validate_is_dropped() {
        let p = tmp("invalid");
        let mut bad = sample();
        bad.name = "Bad".to_string();
        bad.style.opacity = 3.0;
        save(&p, &[bad, sample()]).unwrap();

        let loaded = load(&p);
        assert_eq!(loaded, vec![sample()], "the valid one survives");
    }
}
```

- [ ] **Step 6: 실패를 확인한다**

`src/settings/mod.rs`에 모듈을 등록한다.

```rust
pub mod app;
pub mod lines;
pub mod overrides;
pub mod presets;
pub mod presets_io;
pub mod widgets;
```

Run: `cargo test --lib settings::presets_io`
Expected: FAIL — cannot find function `load`, `save`

- [ ] **Step 7: I/O를 구현한다**

`src/settings/presets_io.rs`의 `struct File` 아래, `mod tests` 위에 넣는다.

```rust
/// Reads the user's presets. A file that is missing, unreadable, or malformed is not an error:
/// the settings window opens with the built-ins alone rather than refusing to start over a
/// broken preset library. The same goes for a single preset that would not validate -- it is
/// dropped and the rest load.
pub fn load(path: &Path) -> Vec<Preset> {
    if !path.exists() {
        return Vec::new();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            return Vec::new();
        }
    };
    let file: File = match toml::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("could not parse {}: {e}", path.display());
            return Vec::new();
        }
    };
    file.presets
        .into_iter()
        .filter(|p| match validates(p) {
            true => true,
            false => {
                tracing::warn!("dropping preset '{}': it does not validate", p.name);
                false
            }
        })
        .collect()
}

/// Whether a preset's look would survive `config::validate` -- the same gate the settings
/// window's own writes go through.
fn validates(p: &Preset) -> bool {
    let probe = Config {
        style: p.style.clone(),
        lines: p.lines.clone(),
        ..Config::default()
    };
    config::validate(&probe).is_ok()
}

/// Writes the library atomically, for the same reason `config::save` does: a plain write
/// truncates before re-filling, and a reader landing in that window would see an empty file.
pub fn save(path: &Path, user: &[Preset]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = File {
        presets: user.to_vec(),
    };
    let text = toml::to_string_pretty(&file)?;

    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("replacing {} with {}", path.display(), tmp.display()))?;
    Ok(())
}
```

- [ ] **Step 8: `presets_path`를 더한다**

`src/paths.rs`의 `config_path` 아래에 넣는다.

```rust
/// `presets.toml`, next to `config.toml`. Only the settings window touches it -- the renderer
/// watches `config.toml` and knows nothing about presets, so saving one does not make the
/// wallpaper redraw.
pub fn presets_path() -> Result<PathBuf> {
    let mut p = config_dir()?;
    std::fs::create_dir_all(&p)?;
    p.push("presets.toml");
    Ok(p)
}
```

`src/paths.rs`의 `mod tests`에 덧붙인다.

```rust
    #[test]
    fn the_presets_file_sits_next_to_the_config() {
        let presets = presets_path().unwrap();
        let cfg = config_path().unwrap();
        assert_eq!(presets.file_name().unwrap(), "presets.toml");
        assert_eq!(presets.parent(), cfg.parent());
    }
```

- [ ] **Step 9: 테스트를 통과시킨다**

Run: `cargo test`
Expected: PASS (전부)

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 경고 없음

- [ ] **Step 10: 커밋**

```bash
git add src/settings/presets.rs src/settings/presets_io.rs src/settings/mod.rs src/paths.rs
git commit -m "feat(settings): presets.toml 저장소와 프리셋 저장/삭제

- 사용자 프리셋을 config.toml 옆 presets.toml에 분리 (렌더러가 watch하지 않음)
- 없거나 깨진 파일, 검증에 실패하는 프리셋은 건너뛰고 계속 진행
- Library::check_name / save_as / delete"
```

---

### Task 5: 프리셋 바 — 콤보, Reset, Delete

이 태스크는 **가드 없이** 프리셋 바를 세운다: 콤보를 고르면 즉시 적용된다. `Save as…`와 전환 확인 행은 Task 6이 얹는다. 둘을 나눈 이유는 `cargo clippy -D warnings` 때문이다 — 쓰기만 하고 읽지 않는 필드는 `dead_code`로 잡히므로, `pending_preset`·`save_as` 필드는 그것을 **읽는 코드와 같은 커밋**에서 태어나야 한다.

**Files:**
- Modify: `src/settings/app.rs`

**Interfaces:**
- Consumes: `presets::{Library, Active, BUILTIN_COUNT}`, `presets_io`, `paths::presets_path`
- Produces:
  - `SettingsApp::library: presets::Library`
  - `SettingsApp::presets_path: PathBuf`
  - `SettingsApp::ui_preset_bar(&mut self, ui: &mut egui::Ui)`
  - `SettingsApp::active(&self) -> presets::Active`
  - `SettingsApp::persist_presets(&mut self)`

- [ ] **Step 1: 판정 헬퍼의 테스트를 쓴다**

egui 위젯 자체는 테스트하지 않는다 — 판정은 이미 `presets.rs`에서 덮여 있고, 여기서 새로 생기는 로직은 `SettingsApp::active`가 config의 세 조각을 `Library::resolve`에 제대로 넘기는지뿐이다.

`src/settings/app.rs`의 `mod tests`가 없으면 파일 끝에 만들고, 있으면 덧붙인다.

```rust
#[cfg(test)]
mod preset_bar_tests {
    use super::*;
    use crate::settings::presets;

    fn app(cfg: Config) -> SettingsApp {
        SettingsApp {
            cfg,
            target: Target::Global,
            monitors: Vec::new(),
            fonts: Vec::new(),
            dirty: false,
            last_write_ms: 0,
            cfg_path: PathBuf::from("config.toml"),
            presets_path: PathBuf::from("presets.toml"),
            library: presets::Library::new(Vec::new()),
            error: None,
            font_registry: FontRegistry::default(),
            font_search: String::new(),
        }
    }

    #[test]
    fn a_fresh_config_is_clean_on_its_own_preset() {
        let a = app(Config::default());
        let i = a.library.find("Clock only").expect("Clock only");
        assert_eq!(a.active(), presets::Active::Clean(i));
    }

    #[test]
    fn a_style_edit_makes_the_active_preset_modified() {
        let mut cfg = Config::default();
        cfg.style.size_px = 123.0;
        let a = app(cfg);
        let i = a.library.find("Clock only").expect("Clock only");
        assert_eq!(a.active(), presets::Active::Modified(i));
    }
}
```

- [ ] **Step 2: 실패를 확인한다**

Run: `cargo test --lib settings::app`
Expected: FAIL — `SettingsApp` has no field `library`, no method `active`

- [ ] **Step 3: 상태를 더한다**

`src/settings/app.rs`의 `use`에 더한다.

```rust
use crate::settings::{lines, overrides, presets, presets_io, widgets};
```

`SettingsApp`에 필드를 더한다.

```rust
pub struct SettingsApp {
    pub cfg: Config,
    pub target: Target,
    pub monitors: Vec<MonitorRef>,
    pub fonts: Vec<String>,
    pub(crate) dirty: bool,
    /// When `config.toml` was last written, for the `SAVE_INTERVAL_MS` throttle.
    pub(crate) last_write_ms: u64,
    pub(crate) cfg_path: PathBuf,
    pub(crate) presets_path: PathBuf,
    /// The built-ins plus the user's own, in picker order.
    pub(crate) library: presets::Library,
    pub(crate) error: Option<String>,
    /// Tracks which font families are safe to render via `FontFamily::Name` (see
    /// `FontRegistry`).
    pub(crate) font_registry: FontRegistry,
    /// Filter text for the font picker's searchable list.
    pub(crate) font_search: String,
}
```

`SettingsApp::new`를 고친다.

```rust
    pub fn new() -> Result<Self> {
        let cfg_path = crate::paths::config_path()?;
        let cfg = config::load_or_create(&cfg_path)?;
        let presets_path = crate::paths::presets_path()?;
        let library = presets::Library::new(presets_io::load(&presets_path));
        let monitors = platform::enumerate_monitors()
            .unwrap_or_default()
            .into_iter()
            .map(|m| MonitorRef {
                id: m.id,
                name: m.name,
            })
            .collect();
        let fonts = crate::platform::fonts::system_families().unwrap_or_default();
        Ok(Self {
            cfg,
            target: Target::Global,
            monitors,
            fonts,
            dirty: false,
            last_write_ms: 0,
            cfg_path,
            presets_path,
            library,
            error: None,
            font_registry: FontRegistry::default(),
            font_search: String::new(),
        })
    }
```

`impl SettingsApp`에 판정 헬퍼를 더한다 (`write` 아래).

```rust
    /// Which preset the current look sits on. Recomputed every frame rather than stored:
    /// every widget in this window can change what it depends on.
    pub fn active(&self) -> presets::Active {
        self.library
            .resolve(self.cfg.preset.as_deref(), &self.cfg.lines, &self.cfg.style)
    }
```

- [ ] **Step 4: 테스트를 통과시킨다**

Run: `cargo test --lib settings::app`
Expected: PASS

- [ ] **Step 5: 프리셋 바를 그린다**

`impl SettingsApp`에 넣는다 (`ui_global` 위).

```rust
    /// The preset picker: what the current look is called, and the four things you can do to
    /// it. Global-only -- a monitor override is a partial change on top of the global look,
    /// which is not a thing a whole-look snapshot can express. (A monitor starts from the
    /// current look anyway: `overrides::enable_style_override` copies it in.)
    fn ui_preset_bar(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        let label = match active {
            presets::Active::Clean(i) => self.library.all()[i].name.clone(),
            presets::Active::Modified(i) => format!("{} *", self.library.all()[i].name),
            presets::Active::Custom => "Custom".to_string(),
        };
        let base = match active {
            presets::Active::Clean(i) | presets::Active::Modified(i) => Some(i),
            presets::Active::Custom => None,
        };
        let modified = matches!(active, presets::Active::Modified(_));

        let mut picked: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.label("Preset:");
            egui::ComboBox::from_id_salt("dc_preset_combo")
                .width(180.0)
                .selected_text(label.as_str())
                .show_ui(ui, |ui| {
                    ui.label("Built-in");
                    for (i, p) in self.library.all().iter().enumerate() {
                        if i == presets::BUILTIN_COUNT {
                            ui.separator();
                            ui.label("Saved");
                        }
                        if ui
                            .selectable_label(base == Some(i), p.name.as_str())
                            .clicked()
                        {
                            picked = Some(i);
                        }
                    }
                });

            if ui
                .add_enabled(modified, egui::Button::new("Reset"))
                .on_hover_text("Throw away the changes and go back to the preset")
                .clicked()
            {
                if let Some(i) = base {
                    self.library.apply(i, &mut self.cfg);
                    self.mark_dirty();
                }
            }

            let deletable = base.is_some_and(|i| !self.library.is_builtin(i));
            if ui
                .add_enabled(deletable, egui::Button::new("Delete"))
                .on_hover_text("Remove this preset. The lines and style on screen stay as they are")
                .clicked()
            {
                if let Some(i) = base {
                    // The look stays; only the label goes. Deleting a preset must not change
                    // what is on the wallpaper.
                    if self.library.delete(i) {
                        self.cfg.preset = None;
                        self.persist_presets();
                        self.mark_dirty();
                    }
                }
            }
        });

        // No guard yet -- Task 6 adds the discard prompt that holds this back when the current
        // preset has unsaved edits on it.
        if let Some(i) = picked {
            self.library.apply(i, &mut self.cfg);
            self.mark_dirty();
        }
    }

    /// Writes the user's presets to `presets.toml`. Failure is shown in the error banner and
    /// otherwise ignored: the library in memory is still right, and nothing on the wallpaper
    /// depends on this file.
    fn persist_presets(&mut self) {
        if let Err(e) = presets_io::save(&self.presets_path, self.library.user()) {
            self.error = Some(format!("Could not save presets: {e}"));
        }
    }
```

`ui_global`의 "Lines" 절 머리에 프리셋 바를 붙인다.

```rust
        ui.heading("Lines");
        self.ui_preset_bar(ui);
        ui.add_space(4.0);
        // Lent out and put back: `lines_editor` needs `&mut Vec<Line>` while `self` is still
        // borrowed by `ui`'s closure-free call chain here.
        let mut list = std::mem::take(&mut self.cfg.lines);
        let lines_changed = lines_editor(ui, &mut list, "global");
        self.cfg.lines = list;
        if lines_changed {
            self.mark_dirty();
        }
        ui.separator();
```

- [ ] **Step 6: 빌드와 테스트**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS, 경고 없음

이 시점의 프리셋 바는 완결된 상태다 — 고르면 즉시 적용되고, `*`가 뜨고, `Reset`·`Delete`가 동작한다. 없는 것은 `Save as…`와 전환 확인 행뿐이다.

- [ ] **Step 7: 커밋**

```bash
git add src/settings/app.rs
git commit -m "feat(settings): 프리셋 바 (콤보 즉시 적용, Reset, Delete)

- Apply 버튼 없음 — 콤보 선택이 곧 적용
- 활성 프리셋과 수정 여부(*)는 매 프레임 계산
- Delete는 라인·스타일을 보존하고 이름표만 떨어뜨림"
```

---

### Task 6: Save as… 박스와 전환 가드

**Files:**
- Modify: `src/settings/app.rs`

**Interfaces:**
- Consumes: `SettingsApp::{library, persist_presets, active}`, `presets::NameStatus`
- Produces:
  - `SettingsApp::pending_preset: Option<usize>`
  - `SettingsApp::save_as: Option<SaveAs>`
  - `settings::app::SaveAs { name: String, then_apply: Option<usize> }`

- [ ] **Step 1: 상태를 더한다**

`SettingsApp`에 필드 두 개를, `library` 바로 아래에 더한다.

```rust
    /// The built-ins plus the user's own, in picker order.
    pub(crate) library: presets::Library,
    /// A preset the user picked while the current one had unsaved edits on it. Held back
    /// until the discard prompt is answered; applying it straight away is exactly what would
    /// throw those edits out without asking.
    pub(crate) pending_preset: Option<usize>,
    /// The open Save-as box, if any.
    pub(crate) save_as: Option<SaveAs>,
    pub(crate) error: Option<String>,
```

`SettingsApp` 정의 아래에 넣는다.

```rust
/// The Save-as box's state. `then_apply` is set when the box was opened from the discard
/// prompt: once the look is safely named, the preset the user had picked is applied.
#[derive(Debug, Default)]
pub(crate) struct SaveAs {
    pub name: String,
    pub then_apply: Option<usize>,
}
```

`SettingsApp::new`의 이니셜라이저에 `library` 바로 아래로 더한다.

```rust
            library,
            pending_preset: None,
            save_as: None,
            error: None,
```

Task 5에서 쓴 테스트 헬퍼 `preset_bar_tests::app`의 이니셜라이저에도 같은 두 줄을 더한다.

```rust
            library: presets::Library::new(Vec::new()),
            pending_preset: None,
            save_as: None,
            error: None,
```

- [ ] **Step 2: 프리셋 바에 Save as… 버튼과 가드를 붙인다**

`ui_preset_bar`의 `Reset` 버튼과 `Delete` 버튼 **사이**에 넣는다.

```rust
            if ui
                .button("Save as\u{2026}")
                .on_hover_text("Store the current lines and style as a preset of your own")
                .clicked()
            {
                self.save_as = Some(SaveAs::default());
                self.pending_preset = None;
            }
```

`ui_preset_bar` 끝의 `picked` 처리를, 가드를 타도록 교체한다 (Task 5의 "No guard yet" 주석도 함께 지운다).

```rust
        if let Some(i) = picked {
            if modified {
                self.pending_preset = Some(i);
            } else {
                self.library.apply(i, &mut self.cfg);
                self.mark_dirty();
            }
        }

        self.ui_discard_prompt(ui);
        self.ui_save_as(ui);
    }

    /// Shown when a preset was picked while the current one had unsaved edits. Inline, not a
    /// modal: the window saves as you type and there is no undo stack to fall back on, so the
    /// one place a confirmation earns its keep is the one click that throws work away.
    fn ui_discard_prompt(&mut self, ui: &mut egui::Ui) {
        let Some(pending) = self.pending_preset else {
            return;
        };
        let from = match self.active() {
            presets::Active::Clean(i) | presets::Active::Modified(i) => {
                self.library.all()[i].name.clone()
            }
            // The edits stopped being edits while the prompt was up (the user undid them by
            // hand). Nothing to discard -- apply and move on.
            presets::Active::Custom => {
                self.library.apply(pending, &mut self.cfg);
                self.pending_preset = None;
                self.mark_dirty();
                return;
            }
        };

        ui.horizontal(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(200, 140, 40),
                format!("\u{26a0} Discard changes to \"{from}\"?"),
            );
            if ui.button("Discard").clicked() {
                self.library.apply(pending, &mut self.cfg);
                self.pending_preset = None;
                self.mark_dirty();
            }
            if ui.button("Save as\u{2026}").clicked() {
                self.save_as = Some(SaveAs {
                    name: String::new(),
                    then_apply: Some(pending),
                });
                self.pending_preset = None;
            }
            if ui.button("Cancel").clicked() {
                self.pending_preset = None;
            }
        });
    }

    /// The name box. Saving stores the current lines and style, moves the label onto the new
    /// preset, and -- when the box was opened from the discard prompt -- applies the preset
    /// the user had picked.
    fn ui_save_as(&mut self, ui: &mut egui::Ui) {
        let Some(mut state) = self.save_as.take() else {
            return;
        };
        let status = self.library.check_name(&state.name);
        let mut keep_open = true;

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .desired_width(180.0)
                    .hint_text("Preset name"),
            );

            let save_label = match status {
                presets::NameStatus::Overwrite => "Overwrite",
                _ => "Save",
            };
            let savable = matches!(
                status,
                presets::NameStatus::New | presets::NameStatus::Overwrite
            );
            if ui
                .add_enabled(savable, egui::Button::new(save_label))
                .clicked()
            {
                let lines = self.cfg.lines.clone();
                let style = self.cfg.style.clone();
                let i = self.library.save_as(&state.name, &lines, &style);
                self.cfg.preset = Some(self.library.all()[i].name.clone());
                self.persist_presets();
                if let Some(next) = state.then_apply {
                    self.library.apply(next, &mut self.cfg);
                }
                self.mark_dirty();
                keep_open = false;
            }
            if ui.button("Cancel").clicked() {
                keep_open = false;
            }

            match status {
                presets::NameStatus::Builtin => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 50, 50),
                        "That is a built-in preset's name",
                    );
                }
                presets::NameStatus::Overwrite => {
                    ui.small("Replaces the preset of that name");
                }
                presets::NameStatus::Empty | presets::NameStatus::New => {}
            }
        });

        if keep_open {
            self.save_as = Some(state);
        }
    }
```

`presets::BUILTIN_COUNT`를 `ui_preset_bar`가 이미 쓰고 있으므로 추가 `use`는 없다.

- [ ] **Step 3: 빌드와 테스트**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS, 경고 없음

- [ ] **Step 4: 손으로 확인한다**

Run: `cargo run -- --settings`

확인할 것 (설계 문서 3.2~3.4):
1. 새 config면 콤보가 `Clock only`, 화면은 시계 한 줄
2. 폰트 크기를 바꾸면 콤보가 `Clock only *`, `Reset`이 활성화된다
3. `Reset`을 누르면 크기가 돌아오고 `*`가 사라진다
4. `*` 상태에서 `D-Day`를 고르면 확인 행이 뜨고 **화면은 아직 바뀌지 않는다**
5. `Cancel` → `Clock only *` 그대로. `Discard` → `D-Day`로 갈리고 스타일도 기본값으로 돌아온다
6. `Save as…`로 `Mine` 저장 → 콤보에 `Saved` 구분선 아래 `Mine`, `*` 사라짐, `Delete` 활성
7. `%APPDATA%\DesktopCountdown\presets.toml`이 생겼고 `Mine`이 들어 있다
8. 내장 이름(`D-Day`)으로 저장 시도 → Save 비활성 + 빨간 안내
9. `Delete` → 화면은 그대로, 콤보는 `Custom`
10. 프리셋을 바꾸는 동안 `config.toml`의 `preset =` 줄이 따라 바뀐다

- [ ] **Step 5: 커밋**

```bash
git add src/settings/app.rs
git commit -m "feat(settings): Save as… 박스와 프리셋 전환 확인 행

- *상태에서 프리셋을 고르면 교체를 보류하고 Discard/Save as…/Cancel를 묻는다
- 내장 이름으로는 저장할 수 없고, 사용자 프리셋 이름이면 덮어쓰기로 바뀐다"
```

---

### Task 7: 문서

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: 없음
- Produces: 없음

- [ ] **Step 1: README의 프리셋 문단을 고친다**

`README.md`의 131~133행을 아래로 교체한다.

```markdown
The settings window ships presets — Clock only, Summary + Clock, D-Day, Days left,
Caption + Clock — that replace the whole look, lines and style together, in one click. A fresh
config starts on Clock only: `{hh}:{mm}:{ss}` on its own.

Picking a preset applies it straight away. Editing on top of it does not touch the preset — the
picker just marks the look as changed (`Clock only *`), and `Reset` puts it back. `Save as…`
stores the current lines and style under a name of your own, so switching presets never costs
you a look you cared to keep; a preset you saved can be deleted again, which drops the name and
leaves the wallpaper as it is. Your own presets live in `presets.toml`, next to `config.toml` —
the renderer does not watch that file, so saving one does not make the countdown redraw.
```

- [ ] **Step 2: `README.md`가 config 예시에서 `show_summary_line`을 광고하지 않는지 확인한다**

Run: `rg -n "show_summary_line|Classic" README.md`
Expected: 결과 없음. 있으면 지운다 — 레거시 필드는 받아만 주는 것이지 쓰라고 있는 것이 아니다.

- [ ] **Step 3: 커밋**

```bash
git add README.md
git commit -m "docs: 프리셋 모델에 맞춰 README 갱신"
```

---

## 최종 검증

- [ ] `cargo test` — 전부 통과
- [ ] `cargo clippy --all-targets -- -D warnings` — 경고 없음
- [ ] `cargo fmt --check` — 차이 없음
- [ ] `git diff master --stat -- src/platform src/app.rs src/countdown.rs src/layout.rs` — **빈 출력.** 렌더러가 바뀌었다면 설계를 벗어난 것이다.
- [ ] 기존 `config.toml`(`[[line]]`을 가진 것)로 렌더러를 띄웠을 때 화면이 그대로다
- [ ] `[[line]]`이 없는 옛 `config.toml`을 띄우면 시계 한 줄이 나오고, 설정 창 콤보가 `Clock only`를 가리킨다
