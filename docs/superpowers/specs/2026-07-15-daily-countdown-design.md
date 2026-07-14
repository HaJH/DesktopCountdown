# DesktopCountdown Daily countdown 설계 문서

- 작성일: 2026-07-15
- 상태: 사용자 승인 (구두 설계안 기준; 본 문서 리뷰 대기)
- 대상: config 스키마, countdown 산술, 토큰, 설정 창
- 선행: 줄 리스트(계획 3), 프리셋 모델, macOS 포트 완성. v1.2.0 기준.

## 1. 목적

매일 반복되는 시각(예: 18:00 퇴근)을 향한 카운트다운을 벽지에 표시한다. 타깃 시각이
지나면 경과 시간을 `+`와 함께 세서 올라가고(야근 경과), 자정이 지나면 다음 날 타깃을
향한 카운트다운으로 돌아간다.

기존 날짜 카운트다운(전역 `target` 하나)과의 관계는 **공존**이다: config에 전역
`daily_target` 시각 필드를 하나 추가하고, 그 값을 참조하는 **daily 전용 토큰 세트**를
새로 만든다. 라인 시스템은 그대로이므로 한 화면에 D-day 라인과 퇴근 카운트다운 라인을
자유롭게 섞을 수 있고, 프리셋도 자연스럽게 추가된다.

검토 후 버린 대안:

- **라인별 타깃 지정** — 가장 유연하지만 라인 편집 UI에 타깃 종류+시각 입력이
  라인마다 붙고, 프리셋 의미(라인+스타일 스냅샷)에 타깃이 들어가야 하는지 등 파생
  결정이 많다. daily 타깃 하나로 충분한 용례에 과하다.
- **모드 전환(날짜 ↔ daily)** — 구현은 가장 단순하지만 D-day와 daily를 동시에 표시할
  수 없다.

## 2. Config 스키마

```toml
target = "2026-12-31T23:59:59"
daily_target = "18:00:00"        # 신규. 기본값 18:00:00
```

- `Config`에 `daily_target: jiff::civil::Time` 필드 추가. serde 기본값 `18:00:00`.
- **항상 직렬화한다** (`skip_serializing_if` 없음): config를 열어본 사용자가 키의
  존재를 발견할 수 있게 한다. `target`과 같은 취급.
- 필드 위치는 `target` 바로 다음, `preset` 앞. TOML은 테이블 뒤의 스칼라를 거부하므로
  스칼라 필드는 `[style]` 위에 있어야 한다(기존 `preset` 필드와 같은 제약).
- 검증 추가 없음: `jiff::civil::Time` 파싱 자체가 00:00:00–23:59:59 범위를 보장한다.
- `migrate` 불필요: serde default가 채운다.

**호환성 노트.** `deny_unknown_fields` 때문에 구버전 바이너리는 `daily_target`이 든
config를 거부한다(파싱 실패 → 시작 시라면 기본 config로 살지 못하고 에러, 실행 중
reload라면 이전 config 유지). 다운그레이드 호환은 약속한 적 없으므로 기록만 한다.

## 3. countdown.rs — 순수 산술

기존 `breakdown`과 같은 파일에, 같은 스타일(순수 함수, Win32/I/O 없음)로 추가한다.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyBreakdown {
    pub hours: i64,     // 0..=23, 항상 (하루 안에서만 재므로)
    pub minutes: i64,   // 0..=59
    pub seconds: i64,   // 0..=59
    /// 오늘의 타깃 시각을 지났고 아직 자정 전이면 true. 값들은 경과 시간이 된다.
    pub overtime: bool,
}

pub fn daily_breakdown(now: &Zoned, target: jiff::civil::Time) -> DailyBreakdown
```

동작:

- 오늘의 타깃 = `now.date().at(target)`을 `now`의 타임존으로 zoned화. 초 단위 차이로
  hours/minutes/seconds를 분해한다.
- `now < 타깃` → 남은 시간, `overtime = false`.
- `now >= 타깃` → 경과 시간, `overtime = true`. 정각에는 `+00:00:00`부터 시작한다.
- **자정 리셋은 별도 로직 없이 성립한다**: 자정이 지나면 `now.date()`가 바뀌어 "오늘의
  타깃"이 다음 날 것이 되므로 자연히 카운트다운으로 돌아간다.
- DST 등으로 오늘 그 시각이 존재하지 않으면 jiff의 기본 disambiguation(compatible)이
  보정한다. zoned 변환 실패는 실 날짜 범위에서 도달 불가 — 기존 `breakdown`의
  `.expect` 관례를 따른다.

엣지: `daily_target = 00:00:00`이면 하루 종일 `overtime`(자정부터 경과를 셈)이다.
이상하지만 잘 정의된 동작이고, 문서화만 한다.

## 4. tokens.rs — 신규 토큰 8개

`render`가 두 분해를 모두 받도록 시그니처를 바꾼다. 호출부는 `app.rs::resolve`와
설정 창 미리보기 두 곳.

```rust
pub fn render(template: &str, b: &Breakdown, d: &DailyBreakdown) -> String
```

| 토큰 | 값 | 예 (15:44:30, 타깃 18:00) | 예 (19:20:05, 타깃 18:00) |
|---|---|---|---|
| `{dailyHh}` `{dailyMm}` `{dailySs}` | 2자리 제로패딩 | `02` `15` `30` | `01` `20` `05` |
| `{dailyHours}` `{dailyMinutes}` `{dailySeconds}` | 패딩 없음 | `2` `15` `30` | `1` `20` `5` |
| `{dailyMinutesTotal}` | 총 분 (내림) | `135` | `80` |
| `{dailySign}` | 카운트다운 중 `""`, 경과 중 `"+"` | `` | `+` |

- 값의 의미는 `overtime`에 따라 남은 시간 ↔ 경과 시간으로 바뀐다. 부호는 값에 붙이지
  않고 `{dailySign}` 토큰으로 분리해서 템플릿 작성자가 위치를 정한다.
  예: `퇴근 {dailySign}{dailyHh}:{dailyMm}:{dailySs}` → 퇴근 전 `퇴근 02:15:30`,
  퇴근 후 `퇴근 +01:20:05`.
- `{dailyHoursTotal}`은 만들지 않는다 — 하루 안이므로 `{dailyHours}`와 항상 같다.
  `{dailySecondsTotal}`도 뚜렷한 용례가 없어 뺀다(나중에 추가해도 호환 문제 없음).
- 미정의 토큰·짝 안 맞는 중괄호 처리 규칙은 기존 그대로.
- `TOKENS` 상수가 13 → 21개. 설정 창 토큰 도움말은 이 상수를 순회하므로 자동 반영.
  설명 문구는 기존처럼 영어.

## 5. app.rs — 틱 경로

`render()`에서 기존 `breakdown(&now, &self.target)` 옆에
`daily_breakdown(&now, self.cfg.daily_target)`을 계산해 `resolve`에 함께 넘긴다.

- `target`과 달리 캐시할 `Zoned`가 없다(매일 바뀌므로). 산술 몇 번이라 매 틱 재계산.
- `reload`에 daily 관련 처리 추가 없음 — `self.cfg` 교체로 끝.
- 텍스트 변화 기반 redraw-skip(`last_lines`)은 변경 없이 그대로 동작한다.

## 6. 설정 창

- **Global 탭 "Target time" 섹션**에 daily 타깃 입력을 추가한다: "Daily target" 라벨 +
  h/m/s 필드 3개. 날짜 필드와 달리 조합 무효(2월 31일류)가 없으므로 egui temp storage
  scratch-copy 트릭 없이 각각 범위 클램프(0–23, 0–59, 0–59)된 필드면 충분하다.
- 미리보기: `preview_breakdown` 옆에 daily 분해도 계산해 `tokens::render`에 넘긴다.
- **빌트인 프리셋 "Daily countdown"** 추가: `{dailySign}{dailyHh}:{dailyMm}:{dailySs}`
  (ratio 1.0) 한 줄, `Style::default()`. 토큰을 몰라도 프리셋 하나로 기능을 발견하게
  하는 것이 목적. `BUILTIN_COUNT` 5 → 6. presets.toml에 이미 "Daily countdown"이라는
  사용자 프리셋이 있는 극단적 경우는 기존 `Library::new`의 충돌 규칙이 처리한다:
  picker에서 빠지되 `dropped`로 보존되어 파일 재작성에서도 살아남는다. 추가 작업 없음.

## 7. 문서

- `README.md`: Features에 daily countdown 한 줄. Known limitations의 "At the target
  time the countdown stops at 00:00:00. It does not count up." 항목에 daily 카운트다운은
  예외(카운트업함)임을 명시.
- `docs/CONFIGURATION.md`: `daily_target` 키, 신규 토큰 8개, 프리셋 표 갱신.

## 8. 테스트

- `countdown.rs`: 타깃 전(남은 시간 분해) / 정각(`overtime=true`, 0:0:0) / 후(경과
  분해) / 자정 직전(경과 23:59:59에 근접) / 자정 직후(다음 날 카운트다운 복귀) /
  타깃 00:00:00(항상 overtime).
- `tokens.rs`: 신규 토큰 렌더, `{dailySign}` 양쪽 상태, 패딩/비패딩,
  `{dailyMinutesTotal}` 유도값. `every_advertised_token_resolves`가 신규 토큰을 자동
  커버.
- `schema.rs`: 기본값 18:00, 파싱, round-trip(기본 config round-trip 테스트가 자동
  커버), `daily_target`이 항상 직렬화되는지.
- `presets.rs`: `builtin_count_matches_the_list` 등 기존 테스트가 6개 기준으로 갱신.

## 9. 영향받는 파일

| 파일 | 변경 |
|---|---|
| `src/config/schema.rs` | `Config::daily_target` 필드 + 기본값 |
| `src/countdown.rs` | `DailyBreakdown`, `daily_breakdown` + 테스트 |
| `src/tokens.rs` | `render` 시그니처 확장, 토큰 8개, `TOKENS` 21개 + 테스트 |
| `src/app.rs` | 매 틱 daily 분해 계산, `resolve`에 전달 |
| `src/settings/app.rs` | Daily target 입력 3필드, 미리보기 daily 분해 |
| `src/settings/presets.rs` | 빌트인 "Daily countdown", `BUILTIN_COUNT = 6` |
| `README.md`, `docs/CONFIGURATION.md` | §7 |

## 10. 비목표 (YAGNI)

- 요일 제한(주말 제외 등) — 필드 추가만으로 나중에 확장 가능
- daily 타깃 복수 개(출근+퇴근 동시)
- 기존 날짜 `target`의 카운트업 — 별개 기능, 이번 범위 아님
- 조건부 라인(퇴근 후 다른 문구로 교체)
- 알림/사운드
