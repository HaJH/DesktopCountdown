# DesktopCountdown 프리셋 모델 설계 문서 (계획 4)

- 작성일: 2026-07-14
- 상태: 사용자 승인 대기
- 대상: config 스키마, 설정 창 (렌더러 무변경)
- 선행: 계획 3(줄 리스트 + 토큰 템플릿) 완성, master 병합됨

## 1. 목적

현재 프리셋은 "설정 창의 줄 편집기에 붙은 콤보 + Apply 버튼"에 불과하다. 세 가지 문제가 있다.

1. **기본값이 어색하다.** 신규 설치가 `{months}m {weeks}w {days}d` + `{hh}:{mm}:{ss}` 두 줄로
   시작하는데, 시계 한 줄이 더 자연스러운 출발점이다.
2. **`Classic`이라는 이름이 아무것도 설명하지 않는다.** 나머지 프리셋은 구성(`Clock only`,
   `Caption + Clock`)이나 내용(`D-Day`, `Days left`)을 말하는데, `Classic`만 "예전에 하드코딩돼
   있던 레이아웃"이라는 **유래**를 말한다. 사용자는 고르기 전까지 뭐가 나올지 알 수 없다.
3. **모델이 없다.** 프리셋은 한 번 적용하면 흔적 없이 사라지는 일회성 채우기다. "지금 이게 어떤
   프리셋인지", "내가 그 위에 뭘 고쳤는지", "내가 만든 모양을 저장해두기"가 전부 불가능하다.

3번이 본질이다. 프리셋을 **이름 붙은 모양의 스냅샷**으로 일반화하고, 그 위에 비파괴적인
오버라이드를 얹을 수 있게 한다. 1·2번은 그 정리에 딸려 온다.

## 2. 프리셋 모델

### 2.1 프리셋은 이름표, 진실은 현재 상태

```rust
pub struct Preset {
    pub name: String,
    pub lines: Vec<Line>,   // 줄별 문구·크기비율·정렬·색
    pub style: Style,       // 글꼴·굵기·기준크기·모드·색·외곽선·그림자·자간·불투명도
}
```

프리셋은 **라인 목록과 스타일을 함께 담는 완전한 모양 스냅샷**이다. 프리셋을 고르면 둘 다 갈린다.

`config.toml`에는 지금과 똑같이 **완전히 해석된** `[[line]]`과 `[style]`이 들어간다. 프리셋은
거기에 이름표 한 줄을 더할 뿐이다.

```toml
target = "2026-10-24T09:00:00"
preset = "Clock only"          # 새 필드. 렌더러는 읽지 않는다.

[style]
font_family = "Consolas"
size_px = 64.0

[[line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
```

```rust
pub struct Config {
    pub target: DateTime,
    /// 현재 모양이 어느 프리셋에서 출발했는지 가리키는 이름표. 설정 창 전용 —
    /// 렌더러는 이 필드를 읽지 않는다. 가리키는 프리셋이 없어졌거나(삭제) 필드가
    /// 없으면(구버전 config·손편집) 이름은 계산으로 복구한다(2.2).
    pub preset: Option<String>,
    pub style: Style,
    pub layout: Layout,
    pub general: General,
    pub lines: Vec<Line>,
    pub displays: Vec<DisplayOverride>,
}
```

**렌더러 코드는 한 줄도 바뀌지 않는다.** 렌더러가 읽는 `[[line]]`·`[style]`은 지금과 같은 모양의
완전히 해석된 값이고, `preset` 필드는 무시하면 그만이다. 프리셋은 순전히 설정 창만의 개념이다.

이름 = 식별자다. 슬러그 매핑을 두지 않는다. 사용자 프리셋 이름은 내장 프리셋 이름과 충돌할 수
없다(3.3).

### 2.2 활성 프리셋과 수정 상태는 저장하지 않고 계산한다

매 프레임 현재 `lines` + `style`을 이름표가 가리키는 프리셋과 비교한다. `Line`과 `Style`이 이미
`PartialEq`를 파생하고 있어 비교는 공짜다. 레거시 `Style::show_summary_line`은 비교에서 제외한다
(직렬화되지 않는 필드라 프리셋 정의에는 애초에 들어가지 않는다).

```rust
pub enum Active {
    /// 이름표가 가리키는 프리셋과 정확히 일치
    Clean(usize),
    /// 이름표는 유효하지만 그 위에 수정이 얹혀 있음
    Modified(usize),
    /// 이름표가 없거나 깨졌고, 어떤 프리셋과도 일치하지 않음
    Custom,
}
```

이름표가 **없거나 깨진** 경우에는 현재 모양을 전체 프리셋 목록과 대조해 정확히 일치하는 것이
있으면 그 이름을 쓴다. 덕분에 구버전 `config.toml`(이름표 없음)이나 손으로 고친 파일도 자연스러운
이름을 얻고, **마이그레이션에 별도 코드가 필요 없다.** 일치하는 것이 없으면 `Custom`이다.

| 상태 | 콤보 표시 |
| --- | --- |
| `Clean` | `Clock only` |
| `Modified` | `Clock only *` |
| `Custom` | `Custom` |

### 2.3 오버라이드는 비파괴적이고 휘발성이다

라인이나 스타일을 직접 편집해도 **프리셋 정의는 건드려지지 않는다.** 이름표가 유지된 채 `*` 표시만
붙는다.

다른 프리셋을 고르면 그 수정은 날아간다. 아까운 모양은 `Save as…`로 이름을 붙여 남긴다 — 안전망이
Apply 게이트가 아니라 **이름 붙여 저장하는 행위** 자체다.

### 2.4 사용자 프리셋 저장소

정의는 `config.toml` 옆 `presets.toml`에 모은다.

```toml
[[preset]]
name = "My look"

[preset.style]
font_family = "Consolas"
font_weight = 700
size_px = 96.0
# ... Style 전 필드

[[preset.line]]
text = "{hh}:{mm}:{ss}"
size_ratio = 1.0
align = "center"
```

**렌더러는 이 파일을 watch하지 않는다.** 그래서 프리셋을 저장하거나 지워도 카운트다운이 다시
그려지지 않는다. "현재 상태"(`config.toml`)와 "내 라이브러리"(`presets.toml`)가 파일 단위로
분리되어, 프리셋만 따로 백업하거나 다른 PC로 옮기기도 쉽다.

파일이 없으면 사용자 프리셋 0개로 시작하고, 첫 저장 때 만들어진다. 파싱에 실패하면 사용자
프리셋 없이 계속 진행하고 로그에 남긴다 — 프리셋 라이브러리가 깨졌다고 설정 창이 뜨지 않으면
안 된다.

## 3. 설정 창 UI

### 3.1 프리셋 바는 전역 탭 전용

지금 `lines_editor`는 전역 탭과 모니터별 오버라이드 패널이 `salt`로 구분해 공유한다. 프리셋이
스타일까지 담게 되면 모니터 패널에 프리셋을 두는 것이 앞뒤가 안 맞는다 — 프리셋은 "전체 모양
스냅샷"인데 오버라이드는 "일부만 덮어쓰기"이기 때문이다.

`lines_editor`를 둘로 쪼갠다.

- `preset_bar(...)` — 전역 탭에서만 호출
- `lines_editor(...)` — 줄 행 편집·재정렬·토큰 참조. 전역과 모니터 패널이 공유 (지금과 동일)

### 3.2 프리셋 바

```
Preset: ( Clock only *    v )   [ Reset ]  [ Save as… ]  [ Delete ]
```

- **콤보** — `── Built-in ──` 5개, `── Saved ──` 사용자 프리셋. 고르는 즉시 라인 + 스타일 교체.
  **Apply 버튼은 없다** (4.1)
- **Reset** — `Modified`일 때만 활성. 이름표가 가리키는 프리셋으로 되돌린다
- **Save as…** — 이름 입력 인라인 행을 연다. 현재 라인 + 스타일을 그 이름으로 저장하고 이름표를
  옮긴다 → `*` 사라짐
- **Delete** — 활성 프리셋이 사용자 프리셋일 때만 활성. **라인·스타일은 건드리지 않고 이름표만
  떨어뜨린다** — 화면은 그대로다. 이후 상태는 2.2의 이름 복구 규칙을 그대로 탄다: 남은 프리셋 중
  현재 모양과 정확히 일치하는 것이 있으면 그 이름이 붙고, 없으면 `Custom`이 된다

### 3.3 이름 규칙

`Save as…`에서 받은 이름이

- 빈 문자열이면 → 거부 (Save 버튼 비활성)
- 내장 프리셋 이름과 같으면 → 거부하고 이유를 표시 (내장은 덮어쓸 수 없다)
- 기존 사용자 프리셋 이름과 같으면 → 덮어쓰기 확인을 받고 진행

### 3.4 전환 가드

`Modified` 상태에서 다른 프리셋을 고르면 교체를 **보류**하고 프리셋 줄 바로 아래에 인라인 확인
행을 띄운다. 모달 창이 아니다.

```
Preset: ( Clock only *    v )
  ⚠ Discard changes to "Clock only"?   [ Discard ]  [ Save as… ]  [ Cancel ]
```

- **Discard** — 보류했던 프리셋으로 교체
- **Save as…** — 이름 입력 행으로 넘어간다. 저장이 끝나면 보류했던 프리셋으로 교체
- **Cancel** — 보류를 취소하고 원래 프리셋에 머문다

보류 중인 선택은 `SettingsApp`의 `pending_preset: Option<usize>` 한 칸이면 된다. 설정 창이 이미
라이브 저장이므로 되돌릴 스택이 필요 없고, 그래서 Undo 버튼은 두지 않는다 — 확인 행과 중복이다.

`Clean` 상태에서 고를 때는 확인 없이 바로 교체한다.

## 4. 무엇이 사라지는가

### 4.1 프리셋 Apply 버튼

Apply 버튼이 존재한 유일한 이유는 코드 주석에 적힌 그대로였다 — 프리셋이 라인 목록을 되돌릴 수
없이 덮어쓰기 때문에 콤보 선택만으로 실행되면 안 된다는 것. 새 모델에서는 공들인 모양이
**프리셋으로 저장되어 있고**, 콤보 선택이 날리는 것은 아직 이름 붙이지 않은 임시 수정뿐이며,
그마저도 전환 가드(3.4)가 잡는다.

Apply를 없애면 프리셋 콤보가 나머지 위젯(색·크기 슬라이더)과 동작이 일치한다. 설정 창의 라이브
저장 아키텍처도 그대로 둘 수 있다.

### 4.2 `default_lines(summary: bool)`의 bool 인자

`Config::default()`와 `migrate()` 폴백이 **둘 다** 시계 한 줄이 되므로 분기가 사라진다.

```rust
/// 기본 라인 목록: 시계 한 줄.
pub fn default_lines() -> Vec<Line> {
    vec![Line { text: MAIN_TEMPLATE.to_string(), ..Line::default() }]
}
```

레거시 `Style::show_summary_line` / `DisplayOverride::show_summary_line`은 `migrate`가 더 이상
읽지 않는다. 필드 자체는 남긴다 — `deny_unknown_fields`가 걸려 있어 지우면 그 필드를 가진 기존
`config.toml`이 **파싱 단계에서 거부**되기 때문이다. `skip_serializing`이라 파일로 다시 새어나가지도
않는다. 주석을 "읽지 않지만 옛 파일을 거부하지 않기 위해 받아만 준다"로 고친다.

## 5. 기본값과 네이밍

| 항목 | 이전 | 이후 |
| --- | --- | --- |
| `Config::default().lines` | 요약 + 시계 | **시계 한 줄** |
| `migrate()` 폴백 | 요약 + 시계 (`show_summary_line` 기본 true) | **시계 한 줄** |
| `Classic` 프리셋 | — | **`Summary + Clock`**으로 개명 |
| `PRESETS` 순서 | Classic, Clock only, … | **Clock only**, Summary + Clock, D-Day, Days left, Caption + Clock |

`Summary + Clock`을 고른 이유: 코드가 이 줄을 일관되게 `summary`라 부르고 있고
(`SUMMARY_TEMPLATE`, `SUMMARY_SIZE_RATIO`, 레거시 `show_summary_line`, README 서술), 기존
`Caption + Clock` / `Clock only`와 같은 **구성 서술** 패턴이라 목록이 정돈된다.

내장 프리셋 5개는 전부 `Style::default()`를 담는다. 그래서 내장 프리셋을 고르는 것은 모양을
기본값으로 되돌리는 동작이기도 하다 — 무의미한 부수효과가 아니라 **복구 지점**으로 쓸모가 있다.

## 6. 모니터별 오버라이드 (변경 없음)

`enable_style_override`는 이미 원하는 대로 동작한다.

> Copies the global style + layout + line list into the monitor's override so the user can
> tweak from the current appearance rather than from blank defaults.

"Override for this monitor"를 켜면 그 순간의 **전역 상태(= 활성 프리셋 + 그 위의 수정)가 통째로
복사**된다. 프리셋 피커를 모니터 패널에서 빼도 빈 기본값에서 시작하는 일은 없다. 전역에서 프리셋을
고르고 다듬은 뒤 모니터 탭에서 체크를 켜면 그 모양이 그대로 들어오고, 거기서 그 모니터만 다르게
만지면 된다.

이 복사는 **스냅샷 포크**이며 그대로 둔다. 한 번 켠 모니터는 이후 전역 프리셋 변경을 따라오지
않는다 — 모든 `Option` 필드가 `Some`으로 채워지기 때문이다. 다시 따라오게 하려면 체크를 껐다
켜면 되고, 그러면 그 시점의 전역 값이 새로 복사된다. `DisplayOverride`에는 `preset` 이름표를 두지
않는다.

## 7. 딸려오는 정리

- `README.md` 프리셋 문단 (현재 "The default is Classic: …" 서술)
- `settings::lines` 테스트 — `the_classic_preset_is_the_default_list`는 이제 성립하지 않는다
  (기본 목록 = `Clock only` 프리셋). `every_preset_builds_at_least_one_line_…`은 프리셋이 스타일을
  담게 되면서 "정렬·색이 기본값" 단언이 무의미해지므로 다시 쓴다
- `config::schema` 테스트 — `default_config_has_the_classic_two_lines`,
  `migrate_synthesizes_the_classic_list_…`, `migrate_honours_the_legacy_summary_flag`
- `config::io` 테스트 — `default_lines(false)`를 쓰는 곳
- `settings::overrides` 테스트 — `default_lines(true)`를 쓰는 곳

## 8. 새 테스트로 덮을 것

- 활성 프리셋 판정: `Clean` / `Modified` / `Custom` 세 갈래, 그리고 이름표가 없을 때 정확히
  일치하는 프리셋을 찾아 이름을 복구하는 경로
- 스타일만 바꿔도 `Modified`가 된다 (프리셋이 스타일을 담으므로)
- `Reset`이 라인과 스타일을 **둘 다** 되돌린다
- `Save as…` → `Clean`이 되고 `presets.toml`에 왕복 직렬화된다
- 내장 프리셋 이름으로 저장 시도 → 거부
- `Delete`가 라인·스타일을 보존하고 이름표만 떨어뜨린다 (이후 상태는 이름 복구 규칙을 따른다)
- `presets.toml`이 없거나 깨져도 설정 창이 뜬다
