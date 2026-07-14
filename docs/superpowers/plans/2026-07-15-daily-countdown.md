# Daily Countdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 매일 반복되는 시각(예: 18:00 퇴근)을 향한 카운트다운 — 지나면 `+` 카운트업, 자정에 자연 리셋 — 을 전역 `daily_target` 필드와 daily 토큰 8개로 추가한다.

**Architecture:** 스펙은 `docs/superpowers/specs/2026-07-15-daily-countdown-design.md`. 순수 산술(`countdown.rs`) → config 필드(`schema.rs`) → 토큰 배선(`tokens.rs` + 호출부 2곳) → 설정 창 입력 → 빌트인 프리셋 → 문서 순으로, 각 태스크가 독립적으로 컴파일·테스트된다.

**Tech Stack:** Rust, jiff(시간 산술, serde 지원), serde/toml, egui(설정 창).

## Global Constraints

- `countdown.rs`/`tokens.rs`/`schema.rs`는 순수 유지: Win32 없음, I/O 없음.
- 코드·주석은 영어. UI 문구·토큰 설명도 영어 (기존 관례).
- 커밋 메시지에 자동 생성 문구(Co-Authored-By 등) 금지.
- 테스트는 `TimeZone::fixed(offset(9))` 등 고정 오프셋 존 사용 (기존 관례; DST 없는 결정적 시간).
- `Config`의 스칼라 필드는 `[style]` 등 테이블보다 앞에 두어야 한다 (TOML 제약).
- 모든 태스크 완료 조건: `cargo test` 전체 통과 (앱 실행 중이면 `single_instance` 테스트가 깨지므로 트레이에서 종료 후 실행 — docs/BACKLOG.md 참고).

---

### Task 1: `countdown.rs` — `DailyBreakdown` + `daily_breakdown`

**Files:**
- Modify: `src/countdown.rs` (구조체·함수는 파일 상단 `breakdown` 아래에, 테스트는 기존 `mod tests` 안에)

**Interfaces:**
- Consumes: 기존 `jiff::{Zoned, civil}`.
- Produces: `pub struct DailyBreakdown { pub hours: i64, pub minutes: i64, pub seconds: i64, pub overtime: bool }` (derive `Debug, Clone, Copy, PartialEq, Eq`), `pub fn daily_breakdown(now: &Zoned, target: jiff::civil::Time) -> DailyBreakdown`. Task 3~5가 이 둘을 사용한다.

- [ ] **Step 1: 실패하는 테스트 작성**

`src/countdown.rs`의 `mod tests` 끝에 추가 (기존 `z` 헬퍼 재사용):

```rust
    // ---- daily_breakdown ----

    use jiff::civil::time;

    #[test]
    fn daily_counts_down_before_the_target() {
        let d = daily_breakdown(&z(2026, 7, 15, 15, 44, 30), time(18, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 2,
                minutes: 15,
                seconds: 30,
                overtime: false
            }
        );
    }

    #[test]
    fn daily_flips_to_overtime_exactly_at_the_target() {
        let d = daily_breakdown(&z(2026, 7, 15, 18, 0, 0), time(18, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 0,
                minutes: 0,
                seconds: 0,
                overtime: true
            }
        );
    }

    #[test]
    fn daily_counts_up_after_the_target() {
        let d = daily_breakdown(&z(2026, 7, 15, 19, 20, 5), time(18, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 1,
                minutes: 20,
                seconds: 5,
                overtime: true
            }
        );
    }

    #[test]
    fn daily_overtime_runs_to_the_end_of_the_day() {
        let d = daily_breakdown(&z(2026, 7, 15, 23, 59, 59), time(18, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 5,
                minutes: 59,
                seconds: 59,
                overtime: true
            }
        );
    }

    /// Midnight needs no reset logic: `now.date()` has moved on, so "today's
    /// target" is the next day's and the countdown resumes by construction.
    #[test]
    fn daily_resets_to_a_countdown_at_midnight() {
        let d = daily_breakdown(&z(2026, 7, 16, 0, 0, 1), time(18, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 17,
                minutes: 59,
                seconds: 59,
                overtime: false
            }
        );
    }

    /// A midnight target is overtime all day long: well-defined, if odd.
    #[test]
    fn daily_midnight_target_is_always_overtime() {
        let d = daily_breakdown(&z(2026, 7, 15, 12, 0, 0), time(0, 0, 0, 0));
        assert_eq!(
            d,
            DailyBreakdown {
                hours: 12,
                minutes: 0,
                seconds: 0,
                overtime: true
            }
        );
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test daily_ --lib`
Expected: 컴파일 에러 — `DailyBreakdown`/`daily_breakdown` 미정의.

- [ ] **Step 3: 최소 구현**

`src/countdown.rs`의 `breakdown` 함수 아래에 추가:

```rust
/// The within-day countdown to `target` (a clock time, not a date): time left
/// before it, time elapsed since it (`overtime`) after, resetting when
/// midnight makes "today's target" the next day's.
///
/// `hours` stays in 0..=23 except on a DST-lengthened 25-hour day, where the
/// whole-hour count can reach 24.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyBreakdown {
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    /// Past today's target and before midnight; the fields hold elapsed time.
    pub overtime: bool,
}

pub fn daily_breakdown(now: &Zoned, target: jiff::civil::Time) -> DailyBreakdown {
    // A civil time that does not exist today (a DST gap) is resolved by
    // jiff's compatible disambiguation; the range error cannot happen for a
    // date a running clock can produce.
    let today_target = now
        .date()
        .to_datetime(target)
        .to_zoned(now.time_zone().clone())
        .expect("today's date at a valid clock time is within jiff's range");
    let secs = today_target.timestamp().as_second() - now.timestamp().as_second();
    let (overtime, secs) = if secs > 0 { (false, secs) } else { (true, -secs) };
    DailyBreakdown {
        hours: secs / 3600,
        minutes: (secs / 60) % 60,
        seconds: secs % 60,
        overtime,
    }
}
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test daily_ --lib`
Expected: 6 passed.

Run: `cargo test --lib`
Expected: 전체 통과 (기존 테스트 무영향).

- [ ] **Step 5: 커밋**

```bash
git add src/countdown.rs
git commit -m "feat(countdown): daily_breakdown - within-day countdown with overtime"
```

---

### Task 2: config 스키마 — `daily_target` 필드

**Files:**
- Modify: `src/config/schema.rs`

**Interfaces:**
- Produces: `Config::daily_target: jiff::civil::Time` (기본값 18:00:00, 항상 직렬화). Task 3(app.rs)·Task 4(설정 창)가 읽고 쓴다.

- [ ] **Step 1: 실패하는 테스트 작성**

`src/config/schema.rs`의 `mod tests`에 추가:

```rust
    #[test]
    fn daily_target_defaults_to_six_pm() {
        let cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(cfg.daily_target, jiff::civil::time(18, 0, 0, 0));
    }

    #[test]
    fn daily_target_parses_from_a_clock_time_string() {
        let cfg: Config =
            toml::from_str("target = \"2026-10-24T09:00:00\"\ndaily_target = \"07:30:00\"\n")
                .unwrap();
        assert_eq!(cfg.daily_target, jiff::civil::time(7, 30, 0, 0));
    }

    /// Always serialized (no skip): a user reading their config.toml should
    /// discover the key exists.
    #[test]
    fn daily_target_is_always_written_back() {
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(
            text.contains("daily_target = \"18:00:00\""),
            "missing daily_target in:\n{text}"
        );
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test daily_target --lib`
Expected: 컴파일 에러 — `daily_target` 필드 없음.

- [ ] **Step 3: 최소 구현**

`src/config/schema.rs`에서 세 군데 수정.

`d_target()` 옆에 기본값 함수 추가:

```rust
fn d_daily_target() -> jiff::civil::Time {
    jiff::civil::time(18, 0, 0, 0)
}
```

`Config` 구조체의 `target` 필드 바로 아래(그리고 `preset` 위)에 추가:

```rust
    /// The clock time the daily tokens count to (`{dailyHh}` and friends --
    /// see `crate::tokens`). Unlike `target` this recurs: every day counts
    /// down to it anew, counts up past it, and resets at midnight.
    ///
    /// Must stay above `[style]`: TOML rejects a scalar written after a table.
    #[serde(default = "d_daily_target")]
    pub daily_target: jiff::civil::Time,
```

`impl Default for Config`에 추가:

```rust
            target: d_target(),
            daily_target: d_daily_target(),
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test --lib config`
Expected: 신규 3개 포함 전체 통과. `default_config_round_trips_through_toml`이 라운드트립을 자동 커버.

Run: `cargo test`
Expected: 전체 통과 (설정 창 등 `Config` 리터럴을 만드는 코드는 `..Config::default()`나 로드 경유라 컴파일에 영향 없음; 깨지면 해당 지점에 `daily_target: d_daily_target()` 상당을 채울 것).

- [ ] **Step 5: 커밋**

```bash
git add src/config/schema.rs
git commit -m "feat(config): daily_target clock-time field, default 18:00"
```

---

### Task 3: `tokens.rs` — daily 토큰 8개 + `render` 배선

**Files:**
- Modify: `src/tokens.rs` (시그니처, `value`, `TOKENS`, 테스트)
- Modify: `src/countdown.rs` (테스트 헬퍼 2곳만)
- Modify: `src/app.rs:310-354` (`render`/`resolve`)
- Modify: `src/settings/app.rs:628-702` (미리보기)

**Interfaces:**
- Consumes: Task 1의 `DailyBreakdown`/`daily_breakdown`, Task 2의 `cfg.daily_target`.
- Produces: `pub fn render(template: &str, b: &Breakdown, d: &DailyBreakdown) -> String`, `pub const TOKENS: [(&str, &str); 21]`. Task 5의 프리셋 가드 테스트가 사용.

- [ ] **Step 1: 실패하는 테스트 작성**

`src/tokens.rs`의 `mod tests`에 추가 — 먼저 헬퍼 (기존 `b()` 아래):

```rust
    /// Hand-built: the token layer is tested against the struct, not against
    /// `daily_breakdown`'s arithmetic (countdown.rs covers that).
    fn daily(overtime: bool) -> DailyBreakdown {
        DailyBreakdown {
            hours: 2,
            minutes: 15,
            seconds: 30,
            overtime,
        }
    }
```

신규 테스트:

```rust
    #[test]
    fn daily_tokens_render_padded_and_unpadded() {
        let d = daily(false);
        assert_eq!(
            render("{dailyHh}:{dailyMm}:{dailySs}", &b(), &d),
            "02:15:30"
        );
        assert_eq!(
            render("{dailyHours}:{dailyMinutes}:{dailySeconds}", &b(), &d),
            "2:15:30"
        );
        assert_eq!(render("{dailyMinutesTotal}", &b(), &d), "135");
    }

    #[test]
    fn daily_sign_is_empty_while_counting_down_and_plus_in_overtime() {
        assert_eq!(
            render("{dailySign}{dailyHh}:{dailyMm}:{dailySs}", &b(), &daily(false)),
            "02:15:30"
        );
        assert_eq!(
            render("{dailySign}{dailyHh}:{dailyMm}:{dailySs}", &b(), &daily(true)),
            "+02:15:30"
        );
    }
```

- [ ] **Step 2: `render` 시그니처 확장 + 토큰 구현**

`src/tokens.rs` 수정. import:

```rust
use crate::countdown::{Breakdown, DailyBreakdown};
```

`render`와 `value` (doc comment의 "from `b`"는 "from `b`/`d`"로):

```rust
pub fn render(template: &str, b: &Breakdown, d: &DailyBreakdown) -> String {
```

본문에서 `value(name, b)` 호출을 `value(name, b, d)`로. `value`:

```rust
fn value(name: &str, b: &Breakdown, d: &DailyBreakdown) -> Option<String> {
```

match에 8개 arm 추가 (기존 `"ss"` arm 아래, `_` 위):

```rust
        "dailyHours" => d.hours.to_string(),
        "dailyMinutes" => d.minutes.to_string(),
        "dailySeconds" => d.seconds.to_string(),
        "dailyMinutesTotal" => (d.hours * 60 + d.minutes).to_string(),
        "dailyHh" => format!("{:02}", d.hours),
        "dailyMm" => format!("{:02}", d.minutes),
        "dailySs" => format!("{:02}", d.seconds),
        "dailySign" => {
            if d.overtime {
                "+".to_string()
            } else {
                String::new()
            }
        }
```

`TOKENS`를 21개로 (기존 13개 뒤에 추가):

```rust
pub const TOKENS: [(&str, &str); 21] = [
    // ...existing 13 entries unchanged...
    ("{dailyHh}", "hours to the daily target, zero-padded (elapsed once past it)"),
    ("{dailyMm}", "minutes to the daily target, zero-padded"),
    ("{dailySs}", "seconds to the daily target, zero-padded"),
    ("{dailyHours}", "hours to the daily target (elapsed once past it)"),
    ("{dailyMinutes}", "minutes to the daily target (0-59)"),
    ("{dailySeconds}", "seconds to the daily target (0-59)"),
    ("{dailyMinutesTotal}", "total minutes to (or past) the daily target"),
    ("{dailySign}", "\"+\" once the daily target has passed, else empty"),
];
```

- [ ] **Step 3: 기존 테스트 호출부를 새 시그니처로**

`src/tokens.rs` 테스트: 기존의 모든 `render(TEMPLATE, &b)` / `render(TEMPLATE, &b())` 호출에 세 번째 인자 `&daily(false)`를 붙인다. 예:

```rust
// before
assert_eq!(render("{hh}:{mm}:{ss}", &b()), "2496:00:00");
// after
assert_eq!(render("{hh}:{mm}:{ss}", &b(), &daily(false)), "2496:00:00");
```

`every_advertised_token_resolves`도 동일하게 `render(token, &b, &daily(false))` — `{dailySign}`은 카운트다운 중 빈 문자열을 내므로 `assert_ne!(rendered, token)`을 그대로 통과한다.

`src/countdown.rs` 테스트 헬퍼 2곳 (이 파일에서 render를 쓰는 유일한 곳):

```rust
    /// Daily tokens are not under test here; any value satisfies `render`.
    const DAILY_ZERO: DailyBreakdown = DailyBreakdown {
        hours: 0,
        minutes: 0,
        seconds: 0,
        overtime: false,
    };

    fn format_main(b: &Breakdown) -> String {
        crate::tokens::render("{hh}:{mm}:{ss}", b, &DAILY_ZERO)
    }
    fn format_summary(b: &Breakdown) -> String {
        crate::tokens::render("{months}m {weeks}w {days}d", b, &DAILY_ZERO)
    }
```

- [ ] **Step 4: 프로덕션 호출부 2곳 배선**

`src/app.rs` — import 확장:

```rust
use crate::countdown::{breakdown, daily_breakdown, Breakdown, DailyBreakdown};
```

`render()` (라인 310 부근) — daily 분해를 함께 계산. `target`과 달리 캐시할 `Zoned`가 없고(매일 바뀜) 산술 몇 번이라 매 틱 재계산한다:

```rust
    fn render(&mut self) -> Result<()> {
        let now = Zoned::now();
        let b = breakdown(&now, &self.target);
        let d = daily_breakdown(&now, self.cfg.daily_target);
        let resolved = self.resolve(&b, &d);
```

같은 함수의 복구 경로(라인 328 부근) `let resolved = self.resolve(&b);`도 `self.resolve(&b, &d)`로.

`resolve` (라인 339 부근):

```rust
    fn resolve(&self, b: &Breakdown, d: &DailyBreakdown) -> Vec<Vec<Line>> {
        // ...
                        text: tokens::render(&l.text, b, d),
```

`src/settings/app.rs` — `ui_preview`(라인 632·640 부근):

```rust
        let b = self.preview_breakdown();
        let d = self.preview_daily();
        // ...
                    let text = crate::tokens::render(&l.text, &b, &d);
```

`preview_breakdown` 아래에 추가:

```rust
    /// The daily companion to `preview_breakdown`. Infallible: a clock time
    /// on today's date always resolves.
    fn preview_daily(&self) -> crate::countdown::DailyBreakdown {
        crate::countdown::daily_breakdown(&jiff::Zoned::now(), self.cfg.daily_target)
    }
```

- [ ] **Step 5: 전체 테스트 통과 확인**

Run: `cargo test`
Expected: 전체 통과 — tokens 신규 2개, 기존 tokens/countdown 테스트 전부 새 시그니처로 통과.

- [ ] **Step 6: 커밋**

```bash
git add src/tokens.rs src/countdown.rs src/app.rs src/settings/app.rs
git commit -m "feat(tokens): 8 daily tokens wired through renderer and preview"
```

---

### Task 4: 설정 창 — Daily target 입력

**Files:**
- Modify: `src/settings/app.rs` (`ui_date_fields` 아래에 메서드 추가, `ui_global` 한 줄)

**Interfaces:**
- Consumes: Task 2의 `self.cfg.daily_target`, 기존 `self.mark_dirty()`.
- Produces: UI만. 후속 태스크 의존 없음.

- [ ] **Step 1: 입력 메서드 추가**

`src/settings/app.rs`의 `ui_date_fields` 바로 아래에 추가. 날짜 필드와 달리 h/m/s는 조합 무효(2월 31일류)가 없어 scratch-copy(temp storage) 없이 매 프레임 config에서 읽고 바로 되쓴다:

```rust
    /// The daily target's clock time. Unlike `ui_date_fields` there is no
    /// invalid combination to hold mid-edit (every clamped h/m/s is a valid
    /// time), so no scratch copy in temp storage is needed.
    fn ui_daily_target(&mut self, ui: &mut egui::Ui) {
        let t = self.cfg.daily_target;
        let (mut hour, mut minute, mut second) = (t.hour(), t.minute(), t.second());
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Daily target");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut hour)
                        .range(0..=23)
                        .prefix("Hour "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut minute)
                        .range(0..=59)
                        .prefix("Min "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut second)
                        .range(0..=59)
                        .prefix("Sec "),
                )
                .changed();
        });
        if changed {
            self.cfg.daily_target = jiff::civil::time(hour, minute, second, 0);
            self.mark_dirty();
        }
    }
```

`ui_global`(라인 774 부근)의 "Target time" 섹션에 끼워 넣는다:

```rust
        ui.heading("Target time");
        self.ui_date_fields(ui);
        ui.add_space(6.0);
        self.ui_daily_target(ui);
        ui.separator();
```

- [ ] **Step 2: 빌드 + 테스트**

Run: `cargo test`
Expected: 전체 통과 (UI 유닛 테스트는 없음; 컴파일이 관문).

- [ ] **Step 3: 실기 확인**

Run: `cargo run -- --settings` (또는 실행 중인 앱의 트레이 → Open settings)
Expected: Global 탭 Target time 섹션에 "Daily target Hour/Min/Sec" 행. 값을 바꾸면 ~100ms 내 config.toml에 `daily_target`이 반영되고, `{dailyHh}` 토큰을 라인에 넣으면 미리보기·벽지가 즉시 따라온다.

- [ ] **Step 4: 커밋**

```bash
git add src/settings/app.rs
git commit -m "feat(settings): daily target clock-time editor in the global tab"
```

---

### Task 5: 빌트인 프리셋 "Daily countdown"

**Files:**
- Modify: `src/settings/presets.rs`

**Interfaces:**
- Consumes: Task 3의 토큰 (가드 테스트), 기존 `preset` 헬퍼·`Library` 충돌 규칙 (presets.toml에 같은 이름의 사용자 프리셋이 있는 극단 케이스는 기존 `Library::new`가 drop+보존 처리 — 추가 작업 없음).
- Produces: 픽커에 여섯 번째 프리셋.

- [ ] **Step 1: 실패하는 테스트 작성**

`src/settings/presets.rs`의 `mod tests`에 추가:

```rust
    #[test]
    fn builtin_list_includes_daily_countdown() {
        assert!(builtin().iter().any(|p| p.name == "Daily countdown"));
    }

    /// Every builtin template must render with no `{` left behind -- an
    /// unresolvable token in a shipped preset would print as a typo.
    #[test]
    fn builtin_templates_use_only_known_tokens() {
        let now = jiff::civil::datetime(2026, 7, 15, 12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(9)))
            .unwrap();
        let target = jiff::civil::datetime(2026, 10, 24, 9, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::offset(9)))
            .unwrap();
        let b = crate::countdown::breakdown(&now, &target);
        let d = crate::countdown::daily_breakdown(&now, jiff::civil::time(18, 0, 0, 0));
        for p in builtin() {
            for l in &p.lines {
                let rendered = crate::tokens::render(&l.text, &b, &d);
                assert!(
                    !rendered.contains('{'),
                    "unresolved token in preset '{}': {rendered}",
                    p.name
                );
            }
        }
    }
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test builtin --lib`
Expected: `builtin_list_includes_daily_countdown` FAIL (목록에 없음). `builtin_templates_use_only_known_tokens`는 기존 5개로는 통과.

- [ ] **Step 3: 프리셋 추가**

`src/settings/presets.rs`:

```rust
pub const BUILTIN_COUNT: usize = 6;
```

`builtin()`의 vec 마지막에 추가:

```rust
        preset(
            "Daily countdown",
            &[("{dailySign}{dailyHh}:{dailyMm}:{dailySs}", 1.0)],
        ),
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test`
Expected: 전체 통과 — `builtin_count_matches_the_list`·`builtin_names_are_unique`가 6개 기준으로 통과. 다른 곳에서 5를 하드코딩한 데가 없는지 `git grep -n "BUILTIN_COUNT\|builtin()" src`로 확인 (모두 상수/함수 경유면 끝).

- [ ] **Step 5: 커밋**

```bash
git add src/settings/presets.rs
git commit -m "feat(presets): builtin Daily countdown preset"
```

---

### Task 6: 문서 — README, CONFIGURATION.md

**Files:**
- Modify: `README.md` (Features, Known limitations)
- Modify: `docs/CONFIGURATION.md` (예시 TOML, 토큰 표, 프리셋 수)

**Interfaces:**
- Consumes: 확정된 동작 전부. 코드 변경 없음.

- [ ] **Step 1: README**

Features 리스트(`- **Multi-monitor.**` 앞)에 추가:

```markdown
- **Daily countdown.** A second, recurring target: a clock time (say 18:00) that every day
  counts down to, counts up past (`+00:12:34`), and resets at midnight.
```

Known limitations의 해당 항목 교체:

```markdown
- At the target time the countdown stops at `00:00:00`. It does not count up (the daily
  tokens are the exception — they do) and does not notify.
```

- [ ] **Step 2: CONFIGURATION.md**

"Lines and tokens" 예시 TOML의 `target` 아래에 한 줄:

```toml
target = "2026-12-31T23:59:59"
daily_target = "18:00:00"   # a clock time the daily tokens count to, every day
```

토큰 표에 행 추가:

```markdown
| `{dailyHh}` `{dailyMm}` `{dailySs}` | To the daily target, zero-padded: hours, minutes, seconds. Past it they count up until midnight resets the cycle |
| `{dailyHours}` `{dailyMinutes}` `{dailySeconds}` | The same, without the padding |
| `{dailyMinutesTotal}` | Total minutes to (or past) the daily target |
| `{dailySign}` | Empty while counting down, `+` once the daily target has passed |
```

표 아래(기존 "An unknown token…" 문단 앞이나 뒤)에 짧은 설명 문단:

```markdown
The `daily*` tokens count to `daily_target` — a clock time, not a date. Every day counts
down to it anew, counts up past it (put `{dailySign}` in front to show the `+`), and resets
at midnight. `target` and `daily_target` are independent; one line can use either, or both.
```

Presets 절의 문장 교체:

```markdown
Six ship with the app: Clock only, Summary + Clock, D-Day, Days left, Caption + Clock,
Daily countdown. A fresh config starts on Clock only: `{hh}:{mm}:{ss}` on its own.
```

- [ ] **Step 3: 커밋**

```bash
git add README.md docs/CONFIGURATION.md
git commit -m "docs: daily countdown - daily_target key, tokens, preset"
```

---

## 최종 검증

- [ ] `cargo test` 전체 통과 (앱 종료 상태에서).
- [ ] `cargo run` 후 라인에 `퇴근 {dailySign}{dailyHh}:{dailyMm}:{dailySs}` 입력 → 벽지에 카운트다운 표시. daily target을 현재 시각 1분 뒤로 잡고 넘겨서 `+00:00:01`로 뒤집히는지 확인.
- [ ] 설정 창 프리셋 픽커에 "Daily countdown"이 보이고 적용되는지 확인.
