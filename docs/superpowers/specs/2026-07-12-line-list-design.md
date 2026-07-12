# DesktopCountdown 줄 리스트 + 토큰 템플릿 설계 문서 (계획 3)

- 작성일: 2026-07-12
- 상태: 사용자 승인
- 대상: config 스키마, 렌더러, 설정 창 전반
- 선행: 계획 1(렌더러), 계획 2(설정 창) 완성, master 병합됨

## 1. 목적

화면에 사용자가 직접 쓴 문구를 얹고, 보조 표시를 `3m 2w 0d` 말고 `106 days left` 같은 형태로도
쓸 수 있게 한다.

두 요구를 따로 구현하면 "커스텀 텍스트 슬롯"과 "요약줄 포맷"이라는 **같은 일을 하는 두 기구**가
생긴다. 토큰이 들어갈 수 있는 문자열 하나면 둘 다 표현되기 때문이다. 그래서 요약줄이라는 전용
개념을 없애고, 화면을 **위에서 아래로 쌓이는 줄들의 리스트**로 일반화한다. 기존 요약줄과 본문은
기본 프리셋의 두 줄이 된다.

## 2. 데이터 모델

### 2.1 `[[line]]`

줄은 `Config` 최상위의 배열이다. `[style]`은 모든 줄이 공유하는 공통 스타일로 남는다.

```toml
target = "2026-12-31T23:59:59"

[style]                    # 전 줄 공통: 폰트, 굵기, 기준 크기, 모드, 색, 외곽선, 그림자, 자간, 불투명도
font_family = "Consolas"
size_px = 64.0             # 각 줄의 size_ratio가 곱해지는 기준
color = "#FFFFFF"

[[line]]
text = "{months}m {weeks}w {days}d"
size_ratio = 0.28

[[line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
```

```rust
pub struct Line {
    pub text: String,                  // 토큰 템플릿. 빈 문자열이면 그리지 않는다.
    pub size_ratio: f32,               // 기본 1.0. style.size_px에 곱해진다.
    pub align: Align,                  // 기본 Center
    pub color: Option<String>,         // None이면 style.color 상속
}

pub enum Align { Left, Center, Right }  // serde: kebab-case
```

줄별로 여는 속성은 이 셋뿐이다. 폰트·굵기·모드·외곽선·그림자·자간·불투명도는 전 줄 공통(`[style]`)
으로 남긴다 — 줄마다 열면 설정 창의 줄 편집 UI가 위젯 대여섯 개짜리 행이 되고, 실제로 그렇게까지
쓸 일이 없다.

기본 리스트는 위 예시 그대로다. 즉 **기본 설정의 화면은 지금과 픽셀 단위로 같다**
(`size_ratio = 0.28`은 현재 하드코딩된 `SUMMARY_RATIO`와 같은 값).

### 2.2 검증

`config::validate`에 추가한다.

- `size_ratio`: 유한하고 `> 0`. (`ConfigError::SizeRatio`)
- `color`: `Some`이면 `#RRGGBB` (기존 `check_color` 재사용)
- `text`: 길이 상한 200자. 넘으면 `ConfigError::TextTooLong`.

## 3. 토큰

`src/tokens.rs`의 순수 함수 하나가 담당한다. Win32도 I/O도 없다.

```rust
pub fn render(template: &str, b: &Breakdown) -> String
```

| 토큰 | 의미 | 예 (2026-07-12 → 2026-10-24) |
|---|---|---|
| `{months}` `{weeks}` `{days}` | 달력 분해 (기존 요약줄과 동일) | `3` `2` `0` |
| `{daysTotal}` | 남은 총 일수 (내림) | `106` |
| `{hoursTotal}` | 남은 총 시간 | `2544` |
| `{minutesTotal}` `{secondsTotal}` | 남은 총 분 / 총 초 | `152658` `9159487` |
| `{hours}` | 하루 안의 시간 (0–23) | `18` |
| `{minutes}` `{seconds}` | 분 / 초 (0–59) | `18` `7` |
| `{hh}` `{mm}` `{ss}` | 2자리 제로패딩. `{hh}`는 **총 시간**이라 자릿수를 넘으면 그대로 | `2544` `18` `07` |

`{hh}:{mm}:{ss}`가 현재 본문 포맷(`format_main`)과 정확히 같다.

규칙:

- **정의되지 않은 토큰은 그대로 남긴다.** `{dayz}`라고 쓰면 화면에 `{dayz}`가 보인다. 오타를 조용히
  삼키지 않기 위해서다.
- 짝이 안 맞는 `{`, `}`도 그대로 남긴다.
- 만료 시(`expired`) 모든 값은 0이다 — `Breakdown`이 이미 그렇게 동작한다.

`Breakdown`에 `total_minutes`/`total_seconds`가 없으므로 필드를 추가하지 않고 `total_hours`와
`minutes`/`seconds`로부터 계산한다. 단 `daysTotal`은 `total_hours / 24`로 유도된다.

`countdown::format_main`/`format_summary`는 토큰 기본 템플릿으로 대체되므로 **삭제한다**
(설정 창 미리보기도 토큰 경로를 쓴다).

## 4. 렌더러

`render::Lines { summary, main }`과 summary 전용 분기를 걷어내고 `&[Line]`을 순회하도록
일반화한다. `Painter::compose`/`paint`가 두 줄을 하드코딩하던 중복이 사라진다.

- 각 줄의 em 크기 = `style.size_px * line.size_ratio` (하한 1.0px로 클램프)
- 빈 `text`(치환 후 기준)는 줄 자체를 건너뛴다 — 간격도 소모하지 않는다.
- 줄 간 간격: 지금처럼 `style.size_px * LINE_GAP_RATIO`(0.12)를 **인접한 모든 줄 쌍 사이에**
  균일 적용한다. 잉크 기준(`ink_span`)이라는 성질은 그대로 유지한다.
- 캔버스 폭 = 가장 넓은 줄의 폭. 각 줄은 그 폭 안에서 `align`대로 배치한다(Left/Center/Right).
- 색: `line.color`가 `Some`이면 그 색, 아니면 `style.color`. 외곽선·그림자 색은 전 줄 공통.
- 줄이 하나도 없거나 전부 비면 캔버스는 최소 크기(1x1)로 두고 아무것도 그리지 않는다.

기존 테스트(잉크 기준 간격, 캔버스가 잉크에 밀착, 프리멀티플라이드 알파, 아웃라인 모드 등)는 새
구조에 맞춰 옮기고, 다음을 추가한다.

- 정렬: Left/Center/Right 각각에서 짧은 줄의 잉크 x 위치가 기대대로인지
- 줄별 색: 서로 다른 색을 준 두 줄의 픽셀 색이 실제로 다른지
- 빈 줄 건너뛰기: 빈 줄을 사이에 넣어도 캔버스 높이가 변하지 않는지
- 3줄 이상에서 간격이 균일한지

## 5. 마이그레이션

기존 `config.toml`에는 `[[line]]`이 없고 `[style].show_summary_line`이 있다. `deny_unknown_fields`
때문에 필드를 그냥 지우면 **기존 파일이 파싱 실패**한다(설정 창이 모든 필드를 직렬화하므로 기존
사용자 파일에는 반드시 이 키가 있다).

그래서:

1. `Style::show_summary_line`을 `Option<bool>`로 바꾸고 `#[serde(default, skip_serializing)]`를 단다
   — **읽되 다시 쓰지 않는 레거시 필드**. `DisplayOverride::show_summary_line`도 같다.
2. 로드 직후(`config::io::load`) `migrate(&mut cfg)`를 한 번 돌린다: `cfg.lines`가 비어 있으면
   `show_summary_line`에 따라 기본 리스트를 합성한다.
   - `None` 또는 `Some(true)` → `[요약줄, 본문]`
   - `Some(false)` → `[본문]`
3. `[[display]]`의 줄 오버라이드도 같은 규칙으로 합성하되, 레거시 `show_summary_line`이
   `Some`일 때만 줄 리스트를 만든다(그 외에는 오버라이드 없음 = 전역 리스트 사용).
4. 다음 저장부터 파일에는 `[[line]]`만 남는다.

**줄 0개는 "화면을 비운다"가 아니라 "미설정"으로 본다.** 설정 창은 마지막 줄을 지울 수 없게 막고,
화면을 비우려면 모니터 오버라이드의 `enabled = false`를 쓴다. 손으로 `[[line]]`을 전부 지운
파일은 기본 리스트로 복원된다.

## 6. 모니터별 오버라이드

기존 규칙대로 `[[display]]`에서 줄 리스트도 덮어쓸 수 있다. 단 **줄 단위 병합이 아니라 리스트
통째 교체**다.

```rust
pub struct DisplayOverride {
    // ...기존 필드...
    #[serde(default, rename = "line", skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<Line>>,
}
```

`merge::effective_for`는 `Some`이면 그 리스트를, `None`이면 전역 리스트를 쓴다. `Effective`에
`lines: Vec<Line>` 필드가 추가된다.

TOML에서는 `[[display.line]]` 블록이 된다. 설정 창의 "오버라이드 켜기"는 현재 전역 리스트를
복사해 넣고 편집하게 한다(기존 `overrides.rs`가 다른 스타일 필드에 하는 것과 동일한 동작).

## 7. 설정 창

"Show summary line" 체크박스가 있던 자리에 **Lines 섹션**이 들어간다. 전역 편집과 모니터
오버라이드 편집이 같은 위젯을 공유한다(기존 `style_fields`와 같은 구조).

```
Preset: [ Classic ▾ ]  [Apply]      ← 적용하면 줄 리스트를 통째로 교체
▸ Available tokens                   ← 접기 패널, §3 표를 그대로 표시

Lines
  1 [{months}m {weeks}w {days}d]  [0.28] [Center ▾] [inherit ☑ ■] ↑ ↓ ✕
  2 [{hh}:{mm}:{ss}            ]  [1.00] [Center ▾] [inherit ☑ ■] ↑ ↓ ✕
  [+ Add line]
```

- 텍스트: `TextEdit::singleline` (개행 없음)
- 크기 비율: `DragValue` 0.05–4.0, 소수 둘째 자리
- 정렬: 3항 콤보(Left/Center/Right)
- 색: "inherit" 체크박스 + 색 버튼. 체크를 풀면 `Some(현재 상속색)`으로 초기화된다.
- `↑`/`↓`: 순서 교환. 첫/마지막 줄에서 해당 버튼은 비활성.
- `✕`: 삭제. **줄이 하나뿐이면 비활성**(§5).
- `+ Add line`: 빈 텍스트, `size_ratio = 1.0`, Center, 상속색인 줄을 끝에 추가.

프리셋은 코드 상수 배열이다.

| 이름 | 줄 |
|---|---|
| Classic | `{months}m {weeks}w {days}d` (0.28) / `{hh}:{mm}:{ss}` (1.0) |
| Clock only | `{hh}:{mm}:{ss}` (1.0) |
| D-Day | `D-{daysTotal}` (1.0) / `{hh}:{mm}:{ss}` (0.3) |
| Days left | `{daysTotal} days left` (0.35) / `{hh}:{mm}:{ss}` (1.0) |
| Caption + Clock | `Deadline` (0.25) / `{hh}:{mm}:{ss}` (1.0) / `{daysTotal} days left` (0.25) |

프리셋 적용은 되돌리기가 없다(자동 저장이므로 즉시 파일에 쓰인다). 드롭다운에서 고르고 `Apply`를
눌러야 적용되게 해서 실수로 리스트가 날아가지 않게 한다.

미리보기(egui)는 각 줄을 `size_ratio` 비례 크기, 줄별 색, 정렬로 근사해 그린다. 기존 안내 문구
("Preview is approximate")는 그대로 둔다.

저장 스로틀(`SAVE_INTERVAL_MS = 100`)과 dirty 처리는 기존 그대로 재사용한다.

## 8. 영향받는 파일

| 파일 | 변경 |
|---|---|
| `src/tokens.rs` | 신규. 토큰 치환 + 테스트 |
| `src/config/schema.rs` | `Line`/`Align` 추가, `Config::lines`, `DisplayOverride::lines`, `show_summary_line` 레거시화, 검증 |
| `src/config/io.rs` | 로드 후 `migrate` 호출 |
| `src/config/merge.rs` | `Effective::lines` |
| `src/countdown.rs` | `format_main`/`format_summary` 삭제 |
| `src/render/mod.rs` | `Lines` 제거, `&[Line]` 순회, 정렬·줄별 색 |
| `src/app.rs` | 매 틱 `tokens::render`로 줄 텍스트 생성 |
| `src/settings/app.rs` | Lines 섹션, 프리셋, 토큰 도움말, 미리보기 |
| `src/settings/overrides.rs` | 줄 리스트 오버라이드 켜기/끄기 |
| `README.md` | 줄 리스트·토큰·프리셋 설명 |

## 9. 비목표 (YAGNI)

- 줄 안에서의 개행 (설정 창 입력칸은 한 줄짜리)
- 줄별 폰트/굵기/외곽선/그림자
- 조건부 토큰(`{days}일 남음` vs `만료됨` 같은 분기), 복수형 처리
- 목표 시각 자체를 표시하는 토큰(`{targetDate}` 등)
- 프리셋 사용자 저장/공유
