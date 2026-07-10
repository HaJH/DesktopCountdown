# DesktopCountdown 설정 창 설계 문서 (계획 2)

- 작성일: 2026-07-11
- 상태: 자율 확정 (사용자가 완성까지 자율 진행 위임). 완성 후 종합 확인.
- 대상: `desktop-countdown.exe --settings`로 뜨는 egui 설정 창
- 선행: 계획 1(렌더러) 완성, master 병합됨

## 1. 목적

`%APPDATA%\DesktopCountdown\config.toml`을 손으로 편집하지 않고 GUI로 편집한다. 렌더러는 이미 이
파일을 감시(200ms 디바운스 + 1Hz 폴)하므로, 설정 창은 파일을 저장할 뿐이고 **IPC는 없다.** 값을
바꾸면 바탕화면의 카운트다운이 바로 바뀐다 — 이 앱의 핫 리로드 강점을 그대로 노출한다.

## 2. 프로세스 구조

설계 §3.1대로 같은 실행 파일이 인자로 역할을 가른다.

| 실행 | 역할 |
|---|---|
| `desktop-countdown.exe` | 렌더러 (계획 1, 완성) |
| `desktop-countdown.exe --settings` | egui 설정 창 |

`main.rs`가 `--settings`를 보면 `settings::run()`을 호출하고 즉시 반환한다. 렌더러 초기화
(DPI 인식, 렌더러 뮤텍스, WorkerW)는 이 경로에서 실행하지 않는다.

설정 창은 자체 명명 뮤텍스(`Local\DesktopCountdown-Settings`)로 단일 인스턴스를 보장한다. 이미
열려 있으면 새 프로세스는 조용히 종료한다(1차 버전은 기존 창을 포그라운드로 올리지 않는다).

트레이 메뉴의 "설정 파일 열기"는 계획 1에서 메모장을 띄운다. **계획 2에서 이를 설정 창 spawn으로
바꾼다** — 트레이가 `notepad.exe` 대신 `현재_exe --settings`를 spawn한다. 이 변경이 계획 2가
계획 1 코드를 건드리는 유일한 지점이다.

## 3. 저장과 반영

**자동 저장.** "적용"/"저장" 버튼이 없다. 위젯 값이 바뀌면 500ms 디바운스 후 `config::save`로
`config.toml`에 쓴다. 렌더러가 그 저장을 감지해 바탕화면에 반영한다. 실제 바탕화면이 라이브
미리보기 역할을 한다.

디바운스가 필요한 이유: 슬라이더를 드래그하면 프레임마다 값이 바뀌는데, 매 프레임 파일을 쓰면
디스크와 렌더러 감시가 폭주한다. 마지막 변경 후 500ms 잠잠하면 한 번 쓴다.

**저장 전 검증.** `config::validate`를 재사용해, 통과할 때만 쓴다. 위젯이 범위를 강제(슬라이더
min/max, 색은 피커)하므로 사실상 invalid가 만들어지지 않지만, target 날짜(2월 30일 등)만은 위젯으로
못 막으므로 파싱 실패 시 저장하지 않고 빨간 경고를 띄운다.

**단방향.** 설정 창은 시작 시 `config.toml`을 한 번 읽어 메모리에 담고, 이후 자기 메모리 상태를
저장한다. 설정 창이 열린 동안 파일을 외부에서 직접 편집하면 설정 창이 덮어쓴다 — 1차 버전은 이
경우를 다루지 않는다(설정 창 열려 있으면 파일 직접 편집 안 한다고 가정).

## 4. 창 레이아웃

eframe 0.35 네이티브 창 하나. 세로로 세 영역:

```
┌─────────────────────────────────────────┐
│ 대상: [전역 기본값 ▼]                     │  ← 상단: 편집 대상 선택
├──────────────────────┬──────────────────┤
│ 컨트롤 (스크롤)       │ 미리보기          │
│  목표 시각            │ ┌──────────────┐ │
│  글자                 │ │ 3m 2w 0d     │ │  ← 근사 미리보기
│  레이아웃             │ │ 2544:18:07   │ │     (어두운 배경)
│  일반                 │ └──────────────┘ │
│                       │                  │
└──────────────────────┴──────────────────┘
```

**미리보기는 근사다.** egui가 자체 폰트로 두 줄을 그린다. 실제 DirectWrite/Direct2D 렌더와 픽셀
단위로 같지 않다(폰트 렌더링·안티에일리어싱·아웃라인 모드가 다름). 색·크기·앵커·요약줄 유무의
감을 주는 용도이며, "정확한 표시는 바탕화면에서 확인"이라는 안내 문구를 함께 둔다. 미리보기는
설정 창이 바탕화면을 가릴 때를 위한 편의다.

## 5. 편집 대상 선택 (모니터별 오버라이드)

상단 ComboBox로 편집 대상을 고른다: **"전역 기본값"** + 연결된 각 모니터(`monitors::enumerate()`로
열거, `name` 표시).

- **전역 기본값 선택 시:** `target`(전역 전용) + 모든 `[style]`/`[layout]`/`[general]` 필드를 편집한다.
- **특정 모니터 선택 시:**
  - `이 모니터에 표시` 체크박스 → `DisplayOverride.enabled`
  - `전역과 다르게 설정` 체크박스 → 켜면 그 모니터의 오버라이드 필드가 나타난다(전역값으로 초기화).
    끄면 해당 모니터의 오버라이드에서 **style/anchor/offset 필드만** 제거하고(전역값을 따름),
    `enabled`는 유지한다 — 표시 여부와 스타일 오버라이드는 독립이다. `enabled`와 스타일이 모두
    전역과 같아져 `[[display]]`에 `id`만 남으면 그 항목 자체를 제거한다(빈 오버라이드 정리).
  - target·autostart는 전역 전용이므로 모니터 대상에는 없다.

설정 파일에 아직 없는 모니터를 선택해 편집하면 새 `[[display]]` 항목을 만든다(id는
`monitors::enumerate()`의 안정적 디바이스 인터페이스 이름).

## 6. 위젯 매핑

| config 필드 | 위젯 | 범위/비고 |
|---|---|---|
| `target` | 년/월/일/시/분/초 `DragValue` 6개 | jiff 파싱 실패 시 빨간 경고, 저장 안 함 |
| `style.font_family` | `ComboBox` | 시스템 폰트 목록(§7) |
| `style.font_weight` | `Slider` | 100–900, step 100 |
| `style.size_px` | `Slider` | 16–240 |
| `style.mode` | `ComboBox` | fill / outline / both |
| `style.color` | egui 색 피커 | `#RRGGBB` ↔ `[u8;3]` 변환 |
| `style.outline_color` | egui 색 피커 | 〃 |
| `style.outline_width_px` | `Slider` | 0–10 |
| `style.opacity` | `Slider` | 0.0–1.0 |
| `style.letter_spacing_em` | `Slider` | -0.05–0.4 |
| `style.shadow` | `Checkbox` | |
| `style.tabular_figures` | `Checkbox` | |
| `style.show_summary_line` | `Checkbox` | |
| `layout.anchor` | 3×3 버튼 그리드 | 선택된 방향 하이라이트 |
| `layout.offset_px` | `DragValue` 2개 | x, y (±픽셀) |
| `general.autostart` | `Checkbox` | 전역 전용 |

색은 config가 `#RRGGBB` 문자열이므로, 로드 시 `color::parse_hex`로 `[u8;3]`을 얻고 저장 시 다시
`#RRGGBB`로 포맷한다.

## 7. 시스템 폰트 열거

`ComboBox`용 설치 폰트 목록이 필요하다. DirectWrite `GetSystemFontCollection`으로 패밀리 이름을
열거한다(로케일 우선, 없으면 en-us). 렌더러의 `render::text`가 이미 `GetSystemFontCollection`을
쓰지만, 설정 창은 `render`를 의존하지 않는 게 깔끔하므로 열거 로직을 별도 함수로 둔다.

새 모듈 `src/fonts.rs`에 `pub fn system_families() -> anyhow::Result<Vec<String>>`를 두고, 렌더러와
설정 창이 공유한다. `render::text`의 폰트 존재 확인도 이 모듈을 쓰도록 정리할 수 있으나, 계획 1
코드 변경을 최소화하기 위해 1차 버전에서는 `fonts.rs`를 신설만 하고 `render::text`는 건드리지 않는다.

폰트 열거가 실패하면 빈 목록 대신 `["Consolas", "Segoe UI"]` 폴백을 쓰고 로그를 남긴다.

## 8. 모듈 경계

새 모듈(설정 창은 egui/eframe만 알고 Win32는 최소):

- `src/settings/mod.rs` — `pub fn run() -> anyhow::Result<()>`. eframe 앱 진입, 창 생성.
- `src/settings/app.rs` — `SettingsApp`(eframe `App` 구현). config 상태, 디바운스 저장, 대상 선택.
- `src/settings/widgets.rs` — 순수 헬퍼: anchor 3×3 그리드, 색 변환, target DragValue 묶음.
  Win32 없음, 단위 테스트 대상.
- `src/fonts.rs` — 시스템 폰트 열거(Win32/DirectWrite).

재사용(계획 1):
- `config`(schema/io/merge/validate) — 그대로.
- `monitors::enumerate` — 대상 목록.
- `paths::config_path` — 파일 위치.
- `single_instance` 패턴 — 설정 창용 별도 뮤텍스.
- `autostart` — 자동 시작은 렌더러가 config를 읽어 반영하므로, 설정 창은 config만 쓰면 된다
  (설정 창이 직접 레지스트리를 건드리지 않는다).

계획 1 변경(최소):
- `main.rs` — `--settings` 인자 분기.
- `tray.rs` — `OpenConfig`가 메모장 대신 `--settings` 프로세스를 spawn.

## 9. 데이터 흐름

시작: `--settings` → 설정 뮤텍스 확보(이미 있으면 종료) → `config::load_or_create` → 모니터 열거
→ 폰트 열거 → eframe 창.

편집 루프(매 프레임): 위젯이 메모리 상태를 갱신 → 변경이 있으면 "dirty" + 마지막 변경 시각 기록
→ dirty이고 마지막 변경 후 500ms 경과하면: 메모리 상태를 `Config`로 조립 → `validate` → 통과 시
`config::save`, 실패 시 경고 표시하고 저장 보류 → dirty 해제.

종료: 창을 닫으면 마지막 dirty 상태를 한 번 flush(디바운스 대기 중이던 변경을 잃지 않도록).

## 10. 에러 처리

- 폰트 열거 실패 → 폴백 목록 + 로그.
- 모니터 열거 실패 → "전역 기본값"만 표시.
- config 저장 실패(디스크 오류 등) → 창에 경고 배너, 다음 변경 때 재시도.
- target 파싱 실패(잘못된 날짜) → 해당 필드 빨간 표시, 저장 보류(나머지 필드도 저장 안 함 —
  부분 저장으로 파일이 어중간해지는 것 방지).
- 로깅은 계획 1의 `logging`을 재사용(`%LOCALAPPDATA%\DesktopCountdown\log.txt`). 단, 설정 창은
  콘솔이 있어도 되므로(디버그 편의) 릴리스에서도 `--settings`는 콘솔을 붙이지 않되 로그는 남긴다.

## 11. 테스트 전략

- `settings/widgets.rs` 순수 헬퍼 — 단위 테스트: 색 `#RRGGBB` ↔ `[u8;3]` 왕복, target 6필드 ↔
  `jiff::civil::DateTime` 왕복(윤년/월말 경계), anchor 그리드 인덱스 ↔ `Anchor` 매핑.
- `fonts::system_families` — 스모크 테스트: 비어 있지 않고 중복 없음(실제 시스템 의존).
- 디바운스 로직 — 순수 함수(`should_save(dirty, last_change, now, debounce)`)로 뽑아 단위 테스트.
- eframe UI 자체는 자동 테스트하지 않는다(수동 확인). `SettingsApp`의 상태 전이(대상 선택 →
  오버라이드 조립, 위젯 값 → Config 조립)는 UI 없이 호출 가능한 순수 메서드로 뽑아 테스트한다.

## 12. 비목표 (1차 버전)

- 실시간 정확 미리보기(실제 렌더러 파이프라인을 창 안에 임베드) — 근사 미리보기로 갈음.
- 설정 창이 열린 동안의 외부 파일 편집 동기화(파일→창 역방향 감시).
- 이미 열린 설정 창을 포그라운드로 올리기.
- 프리셋/테마 저장·불러오기.
- 설정 창 다국어(UI 한국어 고정).
- target에 대한 달력 위젯(6개 DragValue로 갈음).

## 13. 크레이트

계획 1은 렌더러만 구현해 `eframe`을 넣지 않았다. 계획 2에서 `eframe = "0.35"`를 추가한다(egui는
eframe에 포함). eframe은 winit/wgpu 등 무거운 의존성을 끌어와 빌드 시간이 늘고 바이너리가 커진다 —
이는 설정 창을 별도 프로세스로 둔 이유이기도 하다(렌더러 바이너리 경로는 이 무게를 안 진다… 단,
같은 exe이므로 실제로는 한 바이너리에 합쳐진다. 링커가 `--settings` 미사용 시 egui 코드를 죽은
코드로 두지만 바이너리 크기는 늘어난다. 1차 버전은 이를 감수한다).

target을 6개 DragValue로 다루므로 `egui_extras`의 DatePicker는 쓰지 않는다. 색 피커는 egui 기본
(`color_edit_button_srgb`)을 쓴다.

첫 구현 태스크에서 `eframe` 추가 후 `cargo build`가 통과하는지(windows 0.62와 공존, 링크 성공)를
반드시 확인한다.

## 14. 자율 확정한 주요 결정 (완성 후 재작업 후보)

사용자 취향이 강하게 갈릴 수 있어, 재작업 요청이 쉽도록 대안과 함께 기록한다.

1. **자동 저장 vs 명시적 저장 버튼** → 자동 저장 채택. 핫 리로드 강점을 살리고 버튼 클릭을 없앴다.
   대안: "적용" 버튼으로 명시적 저장(실수 방지, 한 단계 추가).
2. **창 안 근사 미리보기 포함** → 포함. 설정 창이 바탕화면을 가릴 때 대비.
   대안: 미리보기 생략하고 실제 바탕화면에만 의존(구현 단순, 창 안에서 즉시 확인 불가).
3. **모니터별 오버라이드 UI: 대상 ComboBox** → 채택. 전역/모니터를 드롭다운으로 전환.
   대안: 탭 바, 또는 전역만 GUI로 두고 모니터별은 파일 직접 편집.
4. **target: 6개 DragValue** → 채택. 의존성 없이 간단.
   대안: `egui_extras` DatePicker(달력 UI, 의존성 추가).
