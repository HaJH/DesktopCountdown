# DesktopCountdown 설정 창 구현 계획 (계획 2/2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `desktop-countdown.exe --settings`로 뜨는 egui 설정 창을 만들어, `config.toml`을 손으로 편집하지 않고 GUI로 편집하고 자동 저장한다. 렌더러가 파일을 감시하므로 바탕화면에 즉시 반영된다.

**Architecture:** eframe 0.35 네이티브 창. 순수 헬퍼(색·날짜·앵커 변환, 디바운스, 모니터 오버라이드 조립)를 Win32/egui와 분리해 전부 단위 테스트한다. `SettingsApp`(eframe `App`)이 `Config`를 들고 위젯으로 편집하며, 변경 후 500ms 디바운스로 `config::save`한다. 계획 1의 `config`/`monitors`/`paths`를 재사용하고, IPC는 없다.

**Tech Stack:** Rust 2021 / eframe + egui 0.35 / `windows` 0.62 (DirectWrite 폰트 열거) / `jiff` 0.2 / 계획 1의 `config`·`monitors`·`paths`

**설계 문서:** `docs/superpowers/specs/2026-07-11-settings-window-design.md`

## Global Constraints

- 대상 OS Windows 10 1809+. Rust edition 2021, rustc 1.92+.
- 추가 크레이트: `eframe = "0.35"` (egui 포함). `egui_extras`는 쓰지 않는다.
- **eframe/egui 0.35의 정확한 API(`App` 트레이트 메서드 시그니처, 위젯 반환 타입, `run_native` 인자)는 이 문서의 코드와 다를 수 있다. 컴파일 에러가 나면 컴파일러가 요구하는 형태를 따른다. 로직을 바꾸지 않는 한 시그니처 조정은 계획 이탈이 아니다.** 같은 규칙이 `windows` 0.62에도 적용된다.
- **이 egui 0.35의 확인된 API 사실(Task 1에서 실측):** `eframe::App`의 필수 메서드는
  `fn update(..)`가 아니라 **`fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`**이다.
  주어진 `ui`는 여백/배경이 없으므로 `egui::CentralPanel::default().show(ui, |ui| { .. })`로 감싼다.
  이 버전에서 패널(`CentralPanel`/`TopBottomPanel`/`SidePanel`)의 `show(ui, ..)`는 **`&mut Ui`를
  받는다**(구버전의 `&Context`가 아니다). `Context`가 필요하면 `ui.ctx()`로 얻는다(재도색은
  `ui.ctx().request_repaint_after(..)`). 종료 시 flush는 `fn on_exit(&mut self)`를 오버라이드한다
  (glow 비활성 시 인자 없음). 이 문서의 이후 코드는 이 API를 전제로 쓰였다.
- 계획 1의 `config`(schema/io/merge/validate), `monitors::enumerate`, `paths::config_path`, `color::parse_hex`를 재사용한다. 이들 공개 API를 바꾸지 않는다.
- 순수 헬퍼 모듈(`settings/widgets.rs`, `settings/overrides.rs`)에는 egui·Win32 의존을 넣지 않는다 — 그래야 `cargo test`로 UI 없이 검증된다.
- 코드와 코드 주석은 영어. 커밋 메시지·README·UI 문자열은 한국어.
- 커밋 메시지에 자동 생성 문구(`Co-Authored-By`, `Generated with` 등) 금지. 제목 + 불릿 몇 개.
- 테스트는 `cargo test`의 기본 병렬 실행에서 통과해야 한다. `--test-threads=1` 금지. 프로세스 전역 상태(뮤텍스 등)를 건드리는 테스트는 `static Mutex`로 직렬화한다.
- 각 태스크는 커밋으로 끝난다.

---

### Task 1: eframe 의존성 + `--settings` 진입점

빌드 게이트. eframe이 `windows` 0.62와 한 바이너리에서 링크되는지 먼저 확인한다. 실패하면 이후
태스크가 전부 막히므로 첫 번째다.

**Files:**
- Modify: `Cargo.toml`
- Create: `src/settings/mod.rs`
- Modify: `src/lib.rs`, `src/main.rs`

**Interfaces:**
- Consumes: 없음
- Produces: `pub fn settings::run() -> anyhow::Result<()>`

- [ ] **Step 1: `Cargo.toml`에 eframe 추가**

`[dependencies]`에 추가:

```toml
eframe = "0.35"
```

- [ ] **Step 2: 빈 설정 창 모듈 작성**

`src/settings/mod.rs`:

```rust
//! The egui settings window, launched via `desktop-countdown.exe --settings`.
//! It edits config.toml; the renderer watches the file and applies changes.

use anyhow::Result;

/// Opens the settings window and blocks until it is closed.
pub fn run() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([720.0, 560.0])
            .with_title("DesktopCountdown 설정"),
        ..Default::default()
    };
    eframe::run_native(
        "DesktopCountdown 설정",
        native_options,
        Box::new(|_cc| Ok(Box::new(SettingsApp::default()))),
    )
    .map_err(|e| anyhow::anyhow!("eframe run failed: {e}"))?;
    Ok(())
}

#[derive(Default)]
struct SettingsApp {}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("설정 창 (구현 예정)");
        });
    }
}
```

- [ ] **Step 3: `lib.rs`와 `main.rs` 배선**

`src/lib.rs`에 `pub mod settings;` 추가.

`src/main.rs`의 `main`을 수정해 `--settings` 인자를 분기한다. 기존 렌더러 경로는 그대로 두고, 인자
분기만 앞에 넣는다:

```rust
fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--settings") {
        // The settings window is a plain GUI process: no DPI-per-monitor setup,
        // no renderer mutex, no WorkerW. It only edits config.toml.
        let _guard = logging::init(&paths::log_dir()?);
        return desktop_countdown::settings::run();
    }

    // ... 기존 렌더러 main 본문 그대로 ...
}
```

`--settings` 경로는 렌더러의 단일 인스턴스 뮤텍스를 잡지 않아야 한다(렌더러와 설정 창이 동시에
떠야 하므로). 기존 `SingleInstance::acquire()` 호출이 렌더러 경로 안에만 있는지 확인하고, 아니면
분기 뒤로 옮긴다.

- [ ] **Step 4: 빌드와 실행 확인**

Run: `cargo build`
Expected: 성공. eframe 의존성이 받아지고 링크된다(첫 빌드는 winit/wgpu 컴파일로 몇 분 걸린다).

Run: `cargo run -- --settings`
Expected: "설정 창 (구현 예정)" 라벨이 있는 창이 뜬다. 창을 닫으면 프로세스가 종료한다.
`cargo run`(인자 없음)은 여전히 렌더러로 동작한다.

빌드가 링크 단계에서 실패하면(windows 0.62 + wgpu 심볼 충돌 등) 여기서 멈추고 보고한다 — 설계의
전제(한 바이너리)가 깨진 것이다.

- [ ] **Step 5: 커밋**

```bash
git add Cargo.toml Cargo.lock src/settings/mod.rs src/lib.rs src/main.rs
git commit -m "설정 창: eframe 진입점과 --settings 분기

- eframe 0.35 추가, 빈 설정 창이 뜨는지 확인
- main이 --settings를 렌더러와 분리해 처리(뮤텍스/DPI/WorkerW 없음)"
```

---

### Task 2: 색·앵커 변환과 디바운스 (순수 헬퍼)

설계 §6, §9, §11. egui·Win32 없는 순수 함수. 전부 단위 테스트.

**Files:**
- Create: `src/settings/widgets.rs`
- Modify: `src/settings/mod.rs`

**Interfaces:**
- Consumes: `crate::color::parse_hex`, `crate::config::Anchor`
- Produces:
  - `pub fn hex_to_rgb(hex: &str) -> [u8; 3]`
  - `pub fn rgb_to_hex(rgb: [u8; 3]) -> String`
  - `pub fn anchor_to_cell(a: Anchor) -> (usize, usize)` — (row, col), 0..3
  - `pub fn cell_to_anchor(row: usize, col: usize) -> Anchor`
  - `pub fn should_save(dirty: bool, ms_since_last_change: u64, debounce_ms: u64) -> bool`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/settings/widgets.rs` 하단:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Anchor;

    #[test]
    fn hex_rgb_round_trips() {
        assert_eq!(hex_to_rgb("#FF8800"), [255, 136, 0]);
        assert_eq!(rgb_to_hex([255, 136, 0]), "#FF8800");
        // Round-trip through both directions.
        for c in [[0, 0, 0], [255, 255, 255], [18, 52, 86]] {
            assert_eq!(hex_to_rgb(&rgb_to_hex(c)), c);
        }
    }

    #[test]
    fn hex_to_rgb_falls_back_on_garbage() {
        // An invalid stored colour must not panic; fall back to white.
        assert_eq!(hex_to_rgb("not-a-colour"), [255, 255, 255]);
        assert_eq!(hex_to_rgb(""), [255, 255, 255]);
    }

    #[test]
    fn rgb_to_hex_is_uppercase_six_digits() {
        assert_eq!(rgb_to_hex([0, 0, 0]), "#000000");
        assert_eq!(rgb_to_hex([171, 205, 239]), "#ABCDEF");
    }

    #[test]
    fn anchor_grid_round_trips_all_nine() {
        use Anchor::*;
        for a in [TopLeft, TopCenter, TopRight, MiddleLeft, Center, MiddleRight,
                  BottomLeft, BottomCenter, BottomRight] {
            let (r, c) = anchor_to_cell(a);
            assert!(r < 3 && c < 3);
            assert_eq!(cell_to_anchor(r, c), a);
        }
    }

    #[test]
    fn anchor_cells_are_positioned_correctly() {
        assert_eq!(anchor_to_cell(Anchor::TopLeft), (0, 0));
        assert_eq!(anchor_to_cell(Anchor::Center), (1, 1));
        assert_eq!(anchor_to_cell(Anchor::BottomRight), (2, 2));
        assert_eq!(cell_to_anchor(0, 2), Anchor::TopRight);
        assert_eq!(cell_to_anchor(2, 0), Anchor::BottomLeft);
    }

    #[test]
    fn should_save_only_after_debounce_and_when_dirty() {
        assert!(!should_save(false, 9999, 500)); // not dirty
        assert!(!should_save(true, 100, 500));    // dirty but too soon
        assert!(should_save(true, 500, 500));     // dirty and settled (boundary)
        assert!(should_save(true, 700, 500));     // dirty and settled
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/settings/mod.rs`에 `mod widgets;` 추가 후,

Run: `cargo test settings::widgets`
Expected: 컴파일 실패. `cannot find function 'hex_to_rgb'`.

- [ ] **Step 3: 최소 구현 작성**

`src/settings/widgets.rs` 상단:

```rust
//! Pure conversion helpers for the settings widgets. No egui, no Win32.

use crate::color::parse_hex;
use crate::config::Anchor;

/// Config stores colours as `#RRGGBB`; egui's picker wants `[u8; 3]`.
/// Invalid stored colours fall back to white rather than panicking.
pub fn hex_to_rgb(hex: &str) -> [u8; 3] {
    match parse_hex(hex) {
        Some(c) => [c.r, c.g, c.b],
        None => [255, 255, 255],
    }
}

pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// The 3x3 anchor grid: row 0 = top, col 0 = left.
pub fn anchor_to_cell(a: Anchor) -> (usize, usize) {
    use Anchor::*;
    match a {
        TopLeft => (0, 0), TopCenter => (0, 1), TopRight => (0, 2),
        MiddleLeft => (1, 0), Center => (1, 1), MiddleRight => (1, 2),
        BottomLeft => (2, 0), BottomCenter => (2, 1), BottomRight => (2, 2),
    }
}

pub fn cell_to_anchor(row: usize, col: usize) -> Anchor {
    use Anchor::*;
    match (row, col) {
        (0, 0) => TopLeft, (0, 1) => TopCenter, (0, 2) => TopRight,
        (1, 0) => MiddleLeft, (1, 2) => MiddleRight,
        (2, 0) => BottomLeft, (2, 1) => BottomCenter, (2, 2) => BottomRight,
        _ => Center, // (1,1) and any out-of-range
    }
}

/// The settings window saves 500 ms after the last edit, not on every frame.
pub fn should_save(dirty: bool, ms_since_last_change: u64, debounce_ms: u64) -> bool {
    dirty && ms_since_last_change >= debounce_ms
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test settings::widgets`
Expected: 5개 PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/settings/
git commit -m "설정 창: 색·앵커 변환과 디바운스 순수 헬퍼"
```

---

### Task 3: target 6필드 ↔ DateTime 변환 (순수 헬퍼)

설계 §6. target을 년/월/일/시/분/초 6개 정수로 편집하고 `jiff::civil::DateTime`으로 왕복한다.
잘못된 날짜(2월 30일 등)를 검출한다.

**Files:**
- Modify: `src/settings/widgets.rs`

**Interfaces:**
- Consumes: `jiff::civil::DateTime`
- Produces:
  - `pub struct DateFields { pub year: i16, pub month: i8, pub day: i8, pub hour: i8, pub minute: i8, pub second: i8 }`
  - `pub fn fields_from_datetime(dt: jiff::civil::DateTime) -> DateFields`
  - `pub fn datetime_from_fields(f: &DateFields) -> Option<jiff::civil::DateTime>` — 잘못된 날짜면 `None`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/settings/widgets.rs`의 `mod tests`에 추가:

```rust
    use jiff::civil::datetime;

    #[test]
    fn datetime_fields_round_trip() {
        let dt = datetime(2026, 10, 24, 9, 30, 15, 0);
        let f = fields_from_datetime(dt);
        assert_eq!((f.year, f.month, f.day, f.hour, f.minute, f.second),
                   (2026, 10, 24, 9, 30, 15));
        assert_eq!(datetime_from_fields(&f), Some(dt));
    }

    #[test]
    fn invalid_dates_return_none() {
        let feb30 = DateFields { year: 2026, month: 2, day: 30, hour: 0, minute: 0, second: 0 };
        assert_eq!(datetime_from_fields(&feb30), None);
        let month13 = DateFields { year: 2026, month: 13, day: 1, hour: 0, minute: 0, second: 0 };
        assert_eq!(datetime_from_fields(&month13), None);
        let hour24 = DateFields { year: 2026, month: 1, day: 1, hour: 24, minute: 0, second: 0 };
        assert_eq!(datetime_from_fields(&hour24), None);
    }

    #[test]
    fn leap_day_is_valid_in_a_leap_year() {
        let f = DateFields { year: 2028, month: 2, day: 29, hour: 12, minute: 0, second: 0 };
        assert!(datetime_from_fields(&f).is_some());
        let f = DateFields { year: 2026, month: 2, day: 29, hour: 12, minute: 0, second: 0 };
        assert_eq!(datetime_from_fields(&f), None); // 2026 is not a leap year
    }
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run: `cargo test settings::widgets`
Expected: 컴파일 실패. `cannot find type 'DateFields'`.

- [ ] **Step 3: 최소 구현 작성**

`src/settings/widgets.rs`에 추가:

```rust
use jiff::civil::DateTime;

/// The countdown target as six editable integers. egui has no date picker,
/// so each field is a DragValue and this validates the combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateFields {
    pub year: i16,
    pub month: i8,
    pub day: i8,
    pub hour: i8,
    pub minute: i8,
    pub second: i8,
}

pub fn fields_from_datetime(dt: DateTime) -> DateFields {
    DateFields {
        year: dt.year(),
        month: dt.month(),
        day: dt.day(),
        hour: dt.hour(),
        minute: dt.minute(),
        second: dt.second(),
    }
}

/// Returns `None` for an impossible date (Feb 30, month 13, hour 24, ...).
/// `jiff::civil::DateTime::new` validates the whole combination including leap years.
pub fn datetime_from_fields(f: &DateFields) -> Option<DateTime> {
    DateTime::new(f.year, f.month, f.day, f.hour, f.minute, f.second, 0).ok()
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test settings::widgets`
Expected: 8개 PASS (기존 5 + 신규 3).

`DateTime::new`의 인자 순서나 반환 타입이 다르면(`jiff` 0.2 시그니처) 컴파일러를 따른다. 로직은
"6필드를 검증된 DateTime으로, 실패 시 None"이다.

- [ ] **Step 5: 커밋**

```bash
git add src/settings/widgets.rs
git commit -m "설정 창: target 6필드 ↔ DateTime 변환과 날짜 검증"
```

---

### Task 4: 모니터 오버라이드 조립 (순수 헬퍼)

설계 §5. 가장 복잡한 순수 로직. "이 모니터를 전역과 다르게" 토글이 `Config.displays`의
`DisplayOverride`를 어떻게 바꾸는지.

**Files:**
- Create: `src/settings/overrides.rs`
- Modify: `src/settings/mod.rs`

**Interfaces:**
- Consumes: `crate::config::{Config, DisplayOverride, Style, Layout}`
- Produces:
  - `pub fn find_override<'a>(cfg: &'a Config, id: &str) -> Option<&'a DisplayOverride>`
  - `pub fn set_enabled(cfg: &mut Config, id: &str, name: &str, enabled: bool)`
  - `pub fn enable_style_override(cfg: &mut Config, id: &str, name: &str)` — 전역 style/layout을 그 모니터의 오버라이드로 복사
  - `pub fn disable_style_override(cfg: &mut Config, id: &str)` — style/anchor/offset 필드를 None으로; 빈 항목이면 제거
  - `pub fn has_style_override(o: &DisplayOverride) -> bool` — style/anchor/offset 중 하나라도 Some인가

- [ ] **Step 1: 실패하는 테스트 작성**

`src/settings/overrides.rs` 하단:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Anchor, Config, DrawMode};

    const ID: &str = "MON-A";
    const NAME: &str = "DISPLAY1";

    #[test]
    fn set_enabled_creates_then_updates_override() {
        let mut cfg = Config::default();
        assert!(find_override(&cfg, ID).is_none());

        set_enabled(&mut cfg, ID, NAME, false);
        let o = find_override(&cfg, ID).unwrap();
        assert_eq!(o.enabled, Some(false));
        assert_eq!(o.name.as_deref(), Some(NAME));
        assert!(!has_style_override(o), "enabling toggle must not add style fields");

        set_enabled(&mut cfg, ID, NAME, true);
        assert_eq!(find_override(&cfg, ID).unwrap().enabled, Some(true));
        assert_eq!(cfg.displays.len(), 1, "must not duplicate the entry");
    }

    #[test]
    fn enable_style_override_copies_global_values() {
        let mut cfg = Config::default();
        cfg.style.size_px = 80.0;
        cfg.layout.anchor = Anchor::TopCenter;

        enable_style_override(&mut cfg, ID, NAME);
        let o = find_override(&cfg, ID).unwrap();
        assert!(has_style_override(o));
        assert_eq!(o.size_px, Some(80.0));
        assert_eq!(o.anchor, Some(Anchor::TopCenter));
        // A field the user has not changed still mirrors the global default.
        assert_eq!(o.mode, Some(DrawMode::Fill));
    }

    #[test]
    fn disable_style_override_clears_style_but_keeps_enabled() {
        let mut cfg = Config::default();
        set_enabled(&mut cfg, ID, NAME, false);      // enabled = Some(false)
        enable_style_override(&mut cfg, ID, NAME);    // adds style fields

        disable_style_override(&mut cfg, ID);
        let o = find_override(&cfg, ID).unwrap();
        assert!(!has_style_override(o), "style fields must be cleared");
        assert_eq!(o.enabled, Some(false), "enabled must survive");
    }

    #[test]
    fn disable_removes_entry_when_nothing_left() {
        let mut cfg = Config::default();
        enable_style_override(&mut cfg, ID, NAME);   // only style fields, enabled is None
        assert_eq!(cfg.displays.len(), 1);

        disable_style_override(&mut cfg, ID);
        // enabled is None and style is cleared → the entry holds only id+name → remove it.
        assert!(cfg.displays.is_empty(), "empty override should be pruned");
    }

    #[test]
    fn disable_keeps_entry_when_enabled_is_set() {
        let mut cfg = Config::default();
        set_enabled(&mut cfg, ID, NAME, false);
        enable_style_override(&mut cfg, ID, NAME);
        disable_style_override(&mut cfg, ID);
        assert_eq!(cfg.displays.len(), 1, "enabled=Some keeps the entry alive");
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/settings/mod.rs`에 `pub mod overrides;` 추가 후,

Run: `cargo test settings::overrides`
Expected: 컴파일 실패. `cannot find function 'find_override'`.

- [ ] **Step 3: 최소 구현 작성**

`src/settings/overrides.rs` 상단. `src/config/schema.rs`의 `DisplayOverride` 필드를 정확히 참조한다
(id, name, enabled, anchor, offset_px, 그리고 style 필드들: font_family, font_weight, size_px, mode,
color, outline_color, outline_width_px, opacity, letter_spacing_em, shadow, tabular_figures,
show_summary_line).

```rust
//! Pure logic for per-monitor overrides in the settings window. No egui, no Win32.

use crate::config::{Config, DisplayOverride};

pub fn find_override<'a>(cfg: &'a Config, id: &str) -> Option<&'a DisplayOverride> {
    cfg.displays.iter().find(|d| d.id == id)
}

fn find_or_create<'a>(cfg: &'a mut Config, id: &str, name: &str) -> &'a mut DisplayOverride {
    if let Some(idx) = cfg.displays.iter().position(|d| d.id == id) {
        &mut cfg.displays[idx]
    } else {
        cfg.displays.push(DisplayOverride {
            id: id.to_string(),
            name: Some(name.to_string()),
            ..Default::default()
        });
        cfg.displays.last_mut().expect("just pushed")
    }
}

/// True if the override carries any style/anchor/offset field (i.e. not just id/name/enabled).
pub fn has_style_override(o: &DisplayOverride) -> bool {
    o.anchor.is_some()
        || o.offset_px.is_some()
        || o.font_family.is_some()
        || o.font_weight.is_some()
        || o.size_px.is_some()
        || o.mode.is_some()
        || o.color.is_some()
        || o.outline_color.is_some()
        || o.outline_width_px.is_some()
        || o.opacity.is_some()
        || o.letter_spacing_em.is_some()
        || o.shadow.is_some()
        || o.tabular_figures.is_some()
        || o.show_summary_line.is_some()
}

pub fn set_enabled(cfg: &mut Config, id: &str, name: &str, enabled: bool) {
    find_or_create(cfg, id, name).enabled = Some(enabled);
}

/// Copies the global style + layout into the monitor's override so the user can
/// tweak from the current appearance rather than from blank defaults.
pub fn enable_style_override(cfg: &mut Config, id: &str, name: &str) {
    let g_style = cfg.style.clone();
    let g_layout = cfg.layout.clone();
    let o = find_or_create(cfg, id, name);
    o.anchor = Some(g_layout.anchor);
    o.offset_px = Some(g_layout.offset_px);
    o.font_family = Some(g_style.font_family);
    o.font_weight = Some(g_style.font_weight);
    o.size_px = Some(g_style.size_px);
    o.mode = Some(g_style.mode);
    o.color = Some(g_style.color);
    o.outline_color = Some(g_style.outline_color);
    o.outline_width_px = Some(g_style.outline_width_px);
    o.opacity = Some(g_style.opacity);
    o.letter_spacing_em = Some(g_style.letter_spacing_em);
    o.shadow = Some(g_style.shadow);
    o.tabular_figures = Some(g_style.tabular_figures);
    o.show_summary_line = Some(g_style.show_summary_line);
}

/// Clears the style/anchor/offset fields (monitor follows global again), keeps `enabled`,
/// and prunes the whole entry if nothing meaningful remains.
pub fn disable_style_override(cfg: &mut Config, id: &str) {
    if let Some(o) = cfg.displays.iter_mut().find(|d| d.id == id) {
        o.anchor = None;
        o.offset_px = None;
        o.font_family = None;
        o.font_weight = None;
        o.size_px = None;
        o.mode = None;
        o.color = None;
        o.outline_color = None;
        o.outline_width_px = None;
        o.opacity = None;
        o.letter_spacing_em = None;
        o.shadow = None;
        o.tabular_figures = None;
        o.show_summary_line = None;
    }
    // Prune entries that now hold only id (+name): no enabled, no style.
    cfg.displays.retain(|d| d.enabled.is_some() || has_style_override(d));
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test settings::overrides`
Expected: 5개 PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/settings/
git commit -m "설정 창: 모니터별 오버라이드 조립 순수 로직

- 표시 여부와 스타일 오버라이드를 독립 처리
- 다르게 설정 켜면 전역값 복사, 끄면 style만 제거하고 enabled 유지
- 빈 오버라이드 항목 정리"
```

---

### Task 5: 시스템 폰트 열거

설계 §7. DirectWrite `GetSystemFontCollection`으로 설치 폰트 패밀리 이름을 얻는다.

**Files:**
- Create: `src/fonts.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: 없음 (DirectWrite 직접)
- Produces: `pub fn system_families() -> anyhow::Result<Vec<String>>`

- [ ] **Step 1: 스모크 테스트 작성**

`src/fonts.rs` 하단. 실제 시스템 의존이라 스모크 테스트다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_a_nonempty_sorted_unique_list() {
        let fams = system_families().unwrap();
        assert!(!fams.is_empty(), "no font families enumerated");
        // No duplicates.
        let mut sorted = fams.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), fams.len(), "duplicate families in list");
        // Every Windows install has at least one common family.
        assert!(fams.iter().any(|f| f == "Segoe UI" || f == "Consolas" || f == "Arial"),
                "expected a common family, got {} families", fams.len());
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod fonts;` 추가 후,

Run: `cargo test fonts`
Expected: 컴파일 실패. `cannot find function 'system_families'`.

- [ ] **Step 3: 구현 작성**

`src/fonts.rs`. `render/text.rs`가 이미 `GetSystemFontCollection`을 쓰므로 그 패턴을 참고하되,
여기서는 전체 패밀리를 열거한다.

```rust
//! System font family enumeration for the settings window's font picker.
//! Shared conceptually with the renderer, but kept separate so `settings` need not
//! depend on `render`.

use anyhow::{anyhow, Result};
use windows::core::HSTRING;
use windows::Win32::Graphics::DirectWrite::*;

const FALLBACK: [&str; 2] = ["Consolas", "Segoe UI"];

/// Returns installed font family names, sorted and de-duplicated. On failure,
/// returns a small fallback list rather than erroring, so the picker always has options.
pub fn system_families() -> Result<Vec<String>> {
    match enumerate() {
        Ok(mut v) if !v.is_empty() => {
            v.sort();
            v.dedup();
            Ok(v)
        }
        other => {
            if let Err(e) = &other {
                tracing::warn!("font enumeration failed: {e:#}, using fallback");
            }
            Ok(FALLBACK.iter().map(|s| s.to_string()).collect())
        }
    }
}

fn enumerate() -> Result<Vec<String>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, false)?;
        let collection = collection.ok_or_else(|| anyhow!("no system font collection"))?;

        let count = collection.GetFontFamilyCount();
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let family = collection.GetFontFamily(i)?;
            let names = family.GetFamilyNames()?;
            // Prefer the user's locale, fall back to index 0.
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            let locale = HSTRING::from("ko-kr");
            let _ = names.FindLocaleName(&locale, &mut index, &mut exists);
            if !exists.as_bool() {
                index = 0;
            }
            let len = names.GetStringLength(index)? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(index, &mut buf)?;
            let name = String::from_utf16_lossy(&buf[..len]);
            if !name.is_empty() {
                out.push(name);
            }
        }
        Ok(out)
    }
}
```

`Cargo.toml`의 windows 피처에 `Win32_Graphics_DirectWrite`가 이미 있다(계획 1). 없으면 추가한다.

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test fonts -- --nocapture`
Expected: 1개 PASS. 폰트 패밀리가 수십~수백 개 나온다.

- [ ] **Step 5: 커밋**

```bash
git add src/fonts.rs src/lib.rs
git commit -m "fonts: 시스템 폰트 패밀리 열거

- DirectWrite GetSystemFontCollection으로 설치 폰트 목록
- 정렬·중복 제거, 실패 시 폴백 목록"
```

---

### Task 6: SettingsApp 상태와 config 조립

설계 §3, §9. `SettingsApp`이 `Config`를 들고, 현재 편집 대상(전역/모니터)을 추적하며, 저장 시
메모리 상태를 검증·저장한다. **UI 위젯은 다음 태스크. 여기서는 상태 구조와 저장 로직만.**

**Files:**
- Create: `src/settings/app.rs`
- Modify: `src/settings/mod.rs`

**Interfaces:**
- Consumes: `crate::config::{Config, load_or_create, save, validate}`, `crate::paths::config_path`,
  `crate::monitors`, `crate::settings::widgets`, `crate::settings::overrides`
- Produces:
  - `pub enum Target { Global, Monitor(usize) }` — index into the monitor list
  - `pub struct SettingsApp { pub cfg: Config, pub target: Target, pub monitors: Vec<MonitorRef>, pub fonts: Vec<String>, dirty: bool, last_change_ms: ..., cfg_path: PathBuf, error: Option<String> }`
  - `pub struct MonitorRef { pub id: String, pub name: String }`
  - `pub fn SettingsApp::new() -> anyhow::Result<SettingsApp>`
  - `pub fn SettingsApp::mark_dirty(&mut self)`
  - `pub fn SettingsApp::save_if_due(&mut self, now_ms: u64)` — 디바운스 통과 시 검증·저장
  - `pub fn SettingsApp::flush(&mut self)` — 종료 시 강제 저장

- [ ] **Step 1: 저장 로직 테스트 작성**

`src/settings/app.rs` 하단. UI 없이 저장 로직을 테스트한다. 임시 파일 경로를 주입할 수 있게
`new`와 별도로 테스트용 생성자를 둔다.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-settings-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("config.toml");
        p
    }

    fn app_with(path: std::path::PathBuf) -> SettingsApp {
        SettingsApp {
            cfg: Config::default(),
            target: Target::Global,
            monitors: vec![],
            fonts: vec!["Consolas".into()],
            dirty: false,
            last_change_ms: 0,
            cfg_path: path,
            error: None,
        }
    }

    #[test]
    fn save_if_due_writes_only_after_debounce() {
        let path = tmp_path("debounce");
        let mut app = app_with(path.clone());
        app.cfg.style.size_px = 123.0;
        app.mark_dirty();
        app.last_change_ms = 1_000;

        app.save_if_due(1_200); // 200ms < 500ms debounce
        assert!(!path.exists(), "should not save before debounce elapses");

        app.save_if_due(1_500); // 500ms elapsed
        assert!(path.exists(), "should save after debounce");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("123"), "saved file must reflect the edit");
    }

    #[test]
    fn save_clears_dirty() {
        let path = tmp_path("clears");
        let mut app = app_with(path);
        app.mark_dirty();
        app.last_change_ms = 0;
        app.save_if_due(1_000);
        assert!(!app.dirty, "dirty must clear after a successful save");
    }

    #[test]
    fn invalid_config_is_not_saved_and_sets_error() {
        let path = tmp_path("invalid");
        let mut app = app_with(path.clone());
        app.cfg.style.opacity = 5.0; // out of range → validate rejects
        app.mark_dirty();
        app.last_change_ms = 0;
        app.save_if_due(1_000);
        assert!(!path.exists(), "invalid config must not be written");
        assert!(app.error.is_some(), "an error message must be surfaced");
    }

    #[test]
    fn flush_forces_a_pending_save() {
        let path = tmp_path("flush");
        let mut app = app_with(path.clone());
        app.cfg.style.size_px = 77.0;
        app.mark_dirty();
        app.last_change_ms = 999_999; // debounce not elapsed by wall clock
        app.flush();
        assert!(path.exists(), "flush must write even if debounce has not elapsed");
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/settings/mod.rs`에 `mod app;` 추가 후,

Run: `cargo test settings::app`
Expected: 컴파일 실패. `cannot find type 'SettingsApp'`.

- [ ] **Step 3: 구현 작성**

`src/settings/app.rs`. `SettingsApp`의 필드는 테스트가 직접 만들 수 있게 같은 모듈이라 접근 가능
(private이어도 `mod tests`에서 보임). eframe `App` 구현은 다음 태스크에서 채우므로 여기서는
`update`를 최소 스텁으로 둔다.

```rust
//! Settings window state and save logic. The eframe UI lives in `render_ui` (Task 7).

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;

use crate::config::{self, Config};
use crate::monitors;

const DEBOUNCE_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Global,
    Monitor(usize),
}

#[derive(Debug, Clone)]
pub struct MonitorRef {
    pub id: String,
    pub name: String,
}

pub struct SettingsApp {
    pub cfg: Config,
    pub target: Target,
    pub monitors: Vec<MonitorRef>,
    pub fonts: Vec<String>,
    pub(crate) dirty: bool,
    pub(crate) last_change_ms: u64,
    pub(crate) cfg_path: PathBuf,
    pub(crate) error: Option<String>,
    // Wall-clock origin so we can express "ms since start" as a u64 for the debounce.
    start: Instant,
}

impl SettingsApp {
    pub fn new() -> Result<Self> {
        let cfg_path = crate::paths::config_path()?;
        let cfg = config::load_or_create(&cfg_path)?;
        let monitors = monitors::enumerate()
            .unwrap_or_default()
            .into_iter()
            .map(|m| MonitorRef { id: m.id, name: m.name })
            .collect();
        let fonts = crate::fonts::system_families().unwrap_or_default();
        Ok(Self {
            cfg,
            target: Target::Global,
            monitors,
            fonts,
            dirty: false,
            last_change_ms: 0,
            cfg_path,
            error: None,
            start: Instant::now(),
        })
    }

    pub fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_change_ms = self.now_ms();
    }

    /// Saves if dirty and the debounce window has elapsed. Invalid configs are not
    /// written; the error is surfaced instead. Never blanks the file.
    pub fn save_if_due(&mut self, now_ms: u64) {
        if !crate::settings::widgets::should_save(
            self.dirty,
            now_ms.saturating_sub(self.last_change_ms),
            DEBOUNCE_MS,
        ) {
            return;
        }
        self.write();
    }

    /// Forces a save of any pending change, ignoring the debounce (used on window close).
    pub fn flush(&mut self) {
        if self.dirty {
            self.write();
        }
    }

    fn write(&mut self) {
        match config::validate(&self.cfg) {
            Ok(()) => match config::save(&self.cfg_path, &self.cfg) {
                Ok(()) => {
                    self.dirty = false;
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("저장 실패: {e}")),
            },
            Err(e) => self.error = Some(format!("잘못된 설정: {e}")),
        }
    }
}
```

`src/settings/mod.rs`의 `run()`에서 `SettingsApp::default()` 대신 `SettingsApp::new()?`를 쓰도록
바꾸고, `SettingsApp`의 `Default` 파생을 제거한다. `eframe::App` 구현은 Task 1의 스텁을
`SettingsApp`으로 옮기되, **이 egui 0.35 API(`fn ui`, 패널 `show(ui)`, `ui.ctx()`)를 따른다:**

```rust
impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ui, |ui| {
            ui.label("설정 창 (UI 구현 예정)");
        });
        let now = self.now_ms();
        self.save_if_due(now);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test settings::app`
Expected: 4개 PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/settings/
git commit -m "설정 창: SettingsApp 상태와 디바운스 저장 로직

- 검증 통과 시에만 저장, 실패 시 에러 표시(파일 안 건드림)
- 종료 시 flush로 대기 중 변경 저장"
```

---

### Task 7: 설정 창 UI — 컨트롤 위젯과 미리보기

설계 §4, §6. eframe UI를 채운다. **수동 확인 태스크** — egui UI는 자동 테스트하지 않는다.

**Files:**
- Modify: `src/settings/app.rs`

**Interfaces:**
- Consumes: `crate::settings::{widgets, overrides}`, `crate::config::{Anchor, DrawMode}`
- Produces: 없음 (UI만)

- [ ] **Step 1: `ui`에 UI 구현**

`SettingsApp::ui`(이 egui 0.35의 필수 메서드, `fn ui(&mut self, ui: &mut Ui, frame)`)를 채운다.
아래는 로직 골격이다. 정확한 위젯 API는 컴파일러를 따른다(예:
`ui.add(egui::Slider::new(&mut v, range))`, `egui::ComboBox`, `ui.color_edit_button_srgb(&mut rgb)`,
`egui::DragValue::new(&mut v)`).

이 egui 0.35에서 패널의 `show(ui, ..)`는 `&mut Ui`를 받는다. 주어진 최상위 `ui` 안에서 패널을
배치한다:

핵심 구조:
1. 상단 `egui::TopBottomPanel::top("target").show_inside(ui, |ui| { .. })`: 편집 대상 ComboBox.
2. 우측 `egui::SidePanel::right("preview").show_inside(ui, |ui| { .. })`: 근사 미리보기(어두운 배경
   위 두 줄 텍스트 + "정확한 표시는 바탕화면에서 확인").
3. `egui::CentralPanel::default().show_inside(ui, |ui| egui::ScrollArea::vertical().show(ui, |ui| { .. }))`:
   대상에 따른 컨트롤.

(패널을 `App::ui`가 준 `ui` 안에 넣을 때는 `show_inside(ui, ..)`를 쓴다. `show(ui, ..)`도 Ui를 받지만
`show_inside`가 중첩 배치에 맞다 — 컴파일러/문서를 따라 맞는 쪽을 쓴다.)

대상별 편집 규칙:
- **전역:** `target`(6 DragValue), `[style]` 전체, `[layout]`, `[general].autostart`를 `self.cfg`에
  직접 편집.
- **모니터(i):** `MonitorRef` = `self.monitors[i]`. `이 모니터에 표시` 체크박스 →
  `overrides::set_enabled`. `전역과 다르게 설정` 체크박스 → 켜면 `overrides::enable_style_override`,
  끄면 `overrides::disable_style_override`. 켜져 있으면(=`overrides::find_override`가 style을 가짐)
  그 오버라이드의 `Some` 필드들을 편집.

**어떤 위젯이든 값이 바뀌면 `self.mark_dirty()`를 호출한다.** egui 위젯의 반환
`Response::changed()`로 감지한다:

```rust
if ui.add(egui::Slider::new(&mut self.cfg.style.size_px, 16.0..=240.0).text("크기")).changed() {
    self.mark_dirty();
}
```

색은 `widgets::hex_to_rgb`로 `[u8;3]`을 만들어 `color_edit_button_srgb`에 넘기고, 바뀌면
`widgets::rgb_to_hex`로 되돌려 `self.cfg.style.color`에 쓴 뒤 `mark_dirty`.

target은 6개 `DragValue`(년 2000..=2100, 월 1..=12, 일 1..=31, 시 0..=23, 분 0..=59, 초 0..=59)로
`DateFields`를 편집하고, `widgets::datetime_from_fields`가 `Some`이면 `self.cfg.target`에 반영,
`None`이면 빨간 "잘못된 날짜" 라벨을 띄우고 target을 갱신하지 않는다.

anchor는 3×3 버튼 그리드. `widgets::anchor_to_cell`로 현재 선택을 하이라이트, 버튼 클릭 시
`widgets::cell_to_anchor`로 설정.

`self.error`가 `Some`이면 상단에 빨간 배너로 표시.

미리보기: `egui::Frame`에 어두운 배경, 현재 유효 스타일(전역이면 전역, 모니터면 그 오버라이드가
전역에 병합된 값 — `config::effective_for(&self.cfg, &id)`)의 색·요약줄 유무로 두 줄
(`0m 0w 0d` / `0000:00:00` 예시 또는 실제 target까지 남은 시간)을 그린다. 폰트·정확한 크기는
근사다.

전체를 다 보여주긴 길므로, 구현자는 위 규칙을 따라 위젯을 배치한다. **모든 편집 위젯은
`.changed()` → `mark_dirty()` 규칙을 지킨다.**

- [ ] **Step 2: 빌드와 clippy**

Run: `cargo build`
Expected: 성공.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 클린.

- [ ] **Step 3: 수동 확인**

Run: `cargo run -- --settings`

확인:
1. 전역 대상에서 크기 슬라이더를 움직이면, 0.5초 뒤 `%APPDATA%\DesktopCountdown\config.toml`의
   `size_px`가 바뀐다(파일을 열어 확인, 또는 렌더러를 같이 띄워 바탕화면이 바뀌는지).
2. 색 피커로 색을 바꾸면 `color`가 `#RRGGBB`로 저장된다.
3. target 일자를 30 → 31로, 2월에서 올리면 "잘못된 날짜" 표시가 뜨고 저장되지 않는다.
4. 대상을 모니터로 바꾸고 "이 모니터에 표시"를 끄면 `[[display]]`에 `enabled = false`가 생긴다.
5. "전역과 다르게 설정"을 켜면 그 모니터의 style 필드가 config에 생기고, 끄면 사라진다.
6. `opacity`를 슬라이더 최대(1.0)로 둬도 저장된다(범위 안).

로그(`%LOCALAPPDATA%\DesktopCountdown\log.txt`)에 에러가 없는지 확인한다. 결과를 보고한다.

- [ ] **Step 4: 커밋**

```bash
git add src/settings/app.rs
git commit -m "설정 창: 컨트롤 위젯과 근사 미리보기 UI

- 전역/모니터 대상 선택, 모든 편집이 디바운스 저장으로 이어짐
- target 날짜 검증, anchor 3×3 그리드, 색 피커
- 근사 미리보기(정확한 표시는 바탕화면)"
```

---

### Task 8: 트레이 연동 + 설정 창 단일 인스턴스

설계 §2. 트레이 "설정 파일 열기"가 메모장 대신 설정 창을 띄운다. 설정 창은 단일 인스턴스.

**Files:**
- Modify: `src/app.rs` (트레이 OpenConfig 처리), `src/tray.rs` (메뉴 라벨), `src/settings/mod.rs`

**Interfaces:**
- Consumes: `crate::single_instance` 패턴
- Produces: 없음

- [ ] **Step 1: 트레이 라벨 변경**

`src/tray.rs`에서 "설정 파일 열기" 메뉴 라벨을 "설정 열기"로 바꾼다(더 이상 파일이 아니라 창).
`TrayCommand::OpenConfig`는 이름을 유지해도 되고 `OpenSettings`로 바꿔도 된다 — 바꾸면 `app.rs`의
매치 암도 함께 바꾼다.

- [ ] **Step 2: OpenConfig가 설정 창을 spawn**

`src/app.rs`의 `TrayCommand::OpenConfig` 처리(현재 `notepad.exe` spawn)를 현재 실행 파일에
`--settings`를 붙여 spawn하도록 바꾼다:

```rust
Some(TrayCommand::OpenConfig) => {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).arg("--settings").spawn() {
                tracing::error!("opening the settings window failed: {e:#}");
            }
        }
        Err(e) => tracing::error!("current_exe failed: {e:#}"),
    }
}
```

- [ ] **Step 3: 설정 창 단일 인스턴스**

`src/settings/run()`의 맨 앞에서 명명된 뮤텍스로 중복 실행을 막는다. 계획 1의
`single_instance` 패턴을 재사용하되, 다른 이름을 쓴다. `single_instance` 모듈이 이름을
받도록 일반화되어 있지 않으면, `settings/mod.rs`에 로컬로 같은 패턴을 구현한다:

```rust
pub fn run() -> Result<()> {
    let _instance = match acquire_settings_mutex() {
        Ok(g) => g,
        Err(_) => {
            // Already open: exit quietly. (Bringing the existing window forward is a non-goal.)
            tracing::info!("settings window already open, exiting");
            return Ok(());
        }
    };
    // ... eframe run ...
}
```

`acquire_settings_mutex`는 `CreateMutexW`로 `Local\DesktopCountdown-Settings`를 만들고
`ERROR_ALREADY_EXISTS`면 `Err`를 낸다(계획 1 `single_instance::acquire`와 동일 구조, 이름만 다름).
`Cargo.toml`의 windows 피처에 `Win32_System_Threading`이 이미 있다.

- [ ] **Step 4: eframe App의 종료 시 flush**

설정 창을 닫을 때 대기 중이던 저장을 flush해야 한다. 이 egui 0.35는 `fn on_exit(&mut self)`
(glow 비활성 시 인자 없음)를 제공한다. 오버라이드해서 `self.flush()`를 호출한다:

```rust
impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) { /* Task 7 */ }
    fn on_exit(&mut self) {
        self.flush();
    }
}
```

`on_exit`의 정확한 시그니처(glow feature 여부)는 컴파일러를 따른다. 만약 이 경로가 창 X 버튼
닫기에서 안 불리면, `ui`에서 `ui.ctx().input(|i| i.viewport().close_requested())`를 감지해 flush하는
방식으로 폴백한다.

- [ ] **Step 5: 빌드·테스트·수동 확인**

Run: `cargo build && cargo test`
Expected: 성공, 전체 테스트 통과.

Run: 렌더러(`cargo run`)를 띄운 상태에서 트레이 아이콘 우클릭 → "설정 열기".
Expected: 설정 창이 뜬다. 한 번 더 누르면 새 창이 뜨지 않는다(단일 인스턴스). 설정 창에서 값을
바꾸고 창을 닫으면 마지막 변경까지 저장되고 바탕화면에 반영된다.

- [ ] **Step 6: 커밋**

```bash
git add src/app.rs src/tray.rs src/settings/mod.rs
git commit -m "설정 창: 트레이 연동과 단일 인스턴스

- 트레이 설정 메뉴가 메모장 대신 --settings 창을 띄움
- 설정 창은 별도 뮤텍스로 중복 실행 방지
- 창 닫을 때 대기 중 변경 flush"
```

---

### Task 9: 릴리스 빌드와 README 갱신

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: 없음
- Produces: 없음

- [ ] **Step 1: 릴리스 빌드 확인**

Run: `cargo build --release`
Expected: 성공. eframe 포함으로 바이너리가 계획 1보다 커진다(수 MB 증가 예상).

Run: `target\release\desktop-countdown.exe --settings`
Expected: 설정 창이 뜬다. `target\release\desktop-countdown.exe`(인자 없음)는 렌더러로 동작.

- [ ] **Step 2: README에 설정 창 섹션 추가**

`README.md`의 "설정" 섹션을 갱신한다. 기존 "메모장으로 config.toml 편집" 안내를 "트레이 → 설정
열기로 GUI 편집, 또는 config.toml 직접 편집"으로 바꾼다. 설정 창이 값 변경 시 자동 저장하고
바탕화면에 즉시 반영된다는 점, target/스타일/레이아웃/모니터별 오버라이드를 GUI로 편집한다는 점,
근사 미리보기이며 정확한 표시는 바탕화면에서 확인한다는 점을 적는다.

- [ ] **Step 3: 전체 검증**

Run: `cargo test`
Expected: 전부 통과(계획 1 92개 + 계획 2 신규). ignored는 계획 1의 4개(라이브 데스크톱 필요).

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 클린.

Run: `cargo fmt --check`
Expected: 클린. 어긋나면 `cargo fmt` 적용.

- [ ] **Step 4: 커밋**

```bash
git add README.md
git commit -m "설정 창: README에 GUI 편집 안내 추가"
```

---

## 완료 조건

- `cargo test`가 전부 통과(계획 2 순수 헬퍼 + 계획 1 회귀 없음).
- `cargo clippy --all-targets -- -D warnings` 클린, `cargo fmt --check` 클린.
- `desktop-countdown.exe --settings`로 설정 창이 뜨고, 값을 바꾸면 500ms 뒤 `config.toml`에 저장되고
  렌더러가 바탕화면에 반영한다.
- 트레이 "설정 열기"가 설정 창을 띄운다. 잘못된 설정은 저장되지 않는다.
- 모니터별 오버라이드를 GUI로 켜고 끌 수 있다.

## 자율 확정 결정 (완성 후 재작업 후보)

설계 §14 참조. 자동 저장 / 근사 미리보기 포함 / 대상 ComboBox / target 6-DragValue를 채택했다.
사용자가 완성 후 다른 방향을 원하면 해당 태스크만 재작업한다.
