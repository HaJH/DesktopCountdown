# DesktopCountdown 설계 문서

- 작성일: 2026-07-10
- 상태: 승인됨. 스파이크 결과에 따라 §3.3/§8/§9/§12 개정 (2026-07-10)
- 대상 플랫폼: Windows 10 1809 이상 / Windows 11
- 언어: Rust (2021 edition, rustc 1.92)

## 1. 목적

바탕화면 배경에 마감까지 남은 시간을 상시 표시하는 네이티브 앱. Wallpaper Engine처럼 데스크톱
아이콘 아래 레이어에 그려지므로 작업을 방해하지 않고, 바탕화면을 볼 때마다 남은 시간이 눈에 들어온다.

## 2. 표시 사양

두 줄로 표시한다.

```
3m 2w 0d
2544:18:07
```

**아랫줄이 주 지표**다. 목표까지 남은 **총 시간**을 `시:분:초`로 보여준다. 시 자리는 최소 2자리로
0-패딩하고, 필요하면 자릿수가 늘어난다(`99:59:59` → `100:00:00`). 분·초는 항상 2자리.

**윗줄은 보조 요약**이다. 남은 기간을 캘린더 기준 개월 수로 먼저 떼고, 그 나머지를 주와 일로 나눈다.
개월 수에 상한이 없다 — 1년 반이 남았으면 `18m 0w 0d`로 표시하며 연 단위는 쓰지 않는다.
`style.show_summary_line = false`로 끌 수 있다.

정렬은 가운데. 자릿수가 줄어들며 줄 폭이 변하는 것은 감수한다(마감 직전 몇 차례 있는 일이다).

비고정폭 폰트를 골랐을 때 매초 숫자가 좌우로 흔들리는 문제는 별개다. DirectWrite 타이포그래피로
OpenType `tnum`(tabular figures)을 기본 활성화해 방지한다. 폰트가 `tnum`을 지원하지 않으면 흔들림이
남지만, 그건 폰트 선택의 결과로 받아들인다.

목표 시각에 도달하면 `00:00:00` / `0m 0w 0d`에서 멈춘다. 경과 시간을 세지 않고, 알림도 띄우지 않는다.

시각 계산은 로컬 시간대 기준이다. `target`은 시간대 없는 로컬 civil datetime으로 저장한다.

### 2.1 계산 규칙

남은 시간 `d = target - now`. `d < 0`이면 `d = 0`으로 클램프하고 `expired = true`.

아랫줄:

- `total_hours = floor(d / 1시간)` — 상한 없음
- `minutes = floor(d / 1분) % 60`
- `seconds = floor(d / 1초) % 60`

윗줄은 캘린더 연산이다. `now`에 1개월씩 더해 `target`을 넘지 않는 최대 횟수가 `months`다.
월말 클램핑은 `jiff`의 기본 동작을 따른다(1/31에 1개월을 더하면 2/28 또는 2/29).
남은 일수를 7로 나눠 `weeks`와 `days`를 구한다. 시·분·초는 윗줄에 반영하지 않는다.

즉 윗줄과 아랫줄은 같은 기간을 서로 다른 해상도로 중복 표현한다. 아랫줄만 봐도 충분하고,
윗줄은 감을 잡기 위한 것이다.

## 3. 아키텍처

### 3.1 프로세스 분리

같은 실행 파일이 인자에 따라 두 역할을 한다.

| 실행 | 역할 |
|---|---|
| `desktop-countdown.exe` | 트레이 아이콘 + 렌더러. Win32 메시지 루프. |
| `desktop-countdown.exe --settings` | egui 설정 창. 별도 프로세스. |

트레이 메뉴의 "설정"은 두 번째 프로세스를 spawn할 뿐이다. **둘 사이에 IPC가 없다.** 설정 창은
`config.toml`을 저장하고, 렌더러는 그 파일을 감시하다 바뀌면 다시 읽는다.

이렇게 나누는 이유는 두 가지다. egui가 쓰는 winit 이벤트 루프와 Win32 메시지 루프를 한 스레드에
섞지 않아도 되고, 설정 창이 패닉으로 죽어도 렌더러는 영향받지 않는다.

렌더러 프로세스는 명명된 뮤텍스로 단일 인스턴스를 보장한다. 설정 창도 마찬가지로 중복 실행을 막는다.

### 3.2 모듈 경계

모듈은 "Win32를 아는가"로 가른다. 모르는 쪽은 전부 단위 테스트 대상이다.

**순수 모듈:**

- `countdown` — 목표 시각과 현재 시각을 받아 `Breakdown { months, weeks, days, total_hours,
  minutes, seconds, expired }`를 계산하고 두 줄 문자열로 포맷한다. 시간 계산은 `jiff`로 한다.
- `config` — TOML 스키마, 전역 기본값과 모니터별 오버라이드 병합, 유효성 검사.
- `layout` — 모니터 사각형·앵커·오프셋·텍스트 크기를 받아 그릴 사각형을 계산한다.

**Win32 모듈:**

- `workerw` — 벽지 레이어 창을 찾고 우리 창을 그 자식으로 만든다. 상세는 §3.3.
  부모가 살아있는지 감시하다 사라지면 재부착한다.
- `dcomp` — DirectComposition 디바이스·타깃·비주얼·표면을 소유한다. 표면에 그릴 때 쓸
  `ID2D1DeviceContext`를 내주고, 그린 뒤 커밋한다. 창 크기가 바뀌면 표면을 다시 만든다.
- `render` — DirectWrite 텍스트 레이아웃 + Direct2D 드로잉. 그리기 방식 3종
  (`fill` / `outline` / `both`), 그림자, 자간을 처리한다. 아웃라인은 커스텀 `IDWriteTextRenderer`로
  글리프 런을 받아 `GetGlyphRunOutline`으로 지오메트리를 뽑아 stroke한다.

  `render`는 **DirectComposition을 모른다.** 그리는 대상을 `&ID2D1RenderTarget`으로만 받는다.
  실행 시에는 `dcomp`가 넘겨주는 디바이스 컨텍스트(`ID2D1DeviceContext`는 `ID2D1RenderTarget`를
  상속한다)를 받고, 테스트에서는 WIC 비트맵 렌더 타깃을 받아 픽셀을 읽어 검증한다.

  텍스트 안티에일리어싱은 반드시 `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`로 둔다. 기본값인 ClearType은
  서브픽셀 렌더링이라 투명 배경 위에서 알파 채널이 망가진다.

  **두 줄의 세로 배치와 캔버스 높이는 줄 상자가 아니라 잉크(실제 글리프) 기준이다.**
  `DWRITE_TEXT_METRICS::height`는 폰트의 ascent+descent로 정해지는 줄 상자이지 문자열의 잉크가
  아니다. CJK 폰트는 한글·한자를 담으려 ascent가 1.16em까지 커지는데 카운트다운이 그리는 숫자는
  cap height(약 0.7em)까지만 닿으므로, 줄 상자를 그대로 쌓으면 두 줄 사이에 약 0.6em의 죽은
  공간이 생긴다 — 코드가 남긴다고 믿는 간격(0.12em)의 5배다. 그래서 각 줄의 잉크 범위를
  글리프 아웃라인(아웃라인 모드가 이미 쓰는 그 지오메트리)의 바운드로 재서, 잉크 사이 간격이
  `LINE_GAP_RATIO`가 되도록 배치한다. `GetOverhangMetrics`는 문자열이 아니라 폰트 전역 글리프
  박스를 보고하므로(측정해보면 줄 상자의 2.6배를 잉크라고 답한다) 쓰지 않는다.

  잉크 측정은 공짜가 아니므로(240px 두 줄 기준 0.11ms) 레이아웃은 갱신당 한 번만 한다:
  `compose`가 `Composed`를 만들어 표면 크기 산정과 `paint`가 같이 쓴다.

  그림자는 블러 없는 오프셋 그림자다(같은 글자를 (2,2)만큼 옮겨 검정 반투명으로 먼저 그린다).
  `ID2D1DeviceContext`를 쓰게 되었으므로 가우시안 블러 이펙트도 기술적으로 가능해졌지만,
  1차 버전 범위 밖으로 둔다.

  불투명도(`style.opacity`)는 브러시 알파에 곱해 넣는다. 별도의 비주얼 불투명도를 쓰지 않는다.
- `monitors` — 모니터 열거, 안정적인 디바이스 ID 조회, DPI.
- `tray` — 트레이 아이콘과 메뉴(설정 / 다시 불러오기 / 종료).
- `autostart` — `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 등록/해제.

### 3.3 창 구성

**이 절의 내용은 전부 실측으로 확인했다.** 근거는 `docs/superpowers/plans/spike-result.md`.

#### 벽지 레이어 창 찾기

두 가지 배치가 실제로 존재한다.

- **고전**: `SHELLDLL_DefView`가 최상위 `WorkerW` 안에 있고, 벽지용 `WorkerW`는 그 다음 최상위 형제다.
- **Progman의 자식**: `SHELLDLL_DefView`가 `Progman` 안에 그대로 있고, 벽지용 `WorkerW`는 `Progman`의
  자식으로 아이콘 아래에 놓인다. 개발 환경(Windows 11 build 26200)이 이쪽이다.

고전 방식을 먼저 시도하고, 실패하면 `FindWindowExW(progman, NULL, "WorkerW", NULL)`로 폴백한다.
둘 다 없을 때만 `Progman`에 `0x052C`를 보내 생성을 요청하고 다시 찾는다. WorkerW가 이미 있으면
이 메시지는 아무 일도 하지 않는다.

#### 자식 창 만들기

**`CreateWindowEx`에 다른 프로세스(explorer.exe)가 소유한 창을 부모로 넘길 수 없다.** `NULL`을
반환하고 `GetLastError()`는 0이다. 순서는 이렇다.

1. 부모 없이 `WS_POPUP`으로 최상위 창을 만든다. 확장 스타일은 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`.
2. `SetParent(hwnd, workerw)`.
3. `SetWindowLongPtrW(hwnd, GWL_STYLE, WS_CHILD | WS_VISIBLE)` — `SetParent`는 스타일을 고치지 않는다.
4. `SetWindowPos(..., SWP_FRAMECHANGED | SWP_NOACTIVATE)` + `ShowWindow(hwnd, SW_SHOW)`.

좌표는 `ScreenToClient(workerw, ..)`로 부모 클라이언트 좌표로 바꿔 넘긴다.

**창에 `WS_EX_LAYERED`를 쓰지 않는다.** 픽셀은 DirectComposition 비주얼이 공급한다.

#### 왜 DirectComposition인가

배경 레이어에서 안정적으로 합성되는 자식 창은 **자기 DWM 비주얼을 갖는 것뿐이다.** 실측 결과:

| 방식 | 결과 |
|---|---|
| 평범한 자식 창 + GDI 페인트 | Explorer가 벽지를 다시 칠할 때 덮인다 |
| 레이어드 자식 + `SetLayeredWindowAttributes` | 보인다. 단 균일 알파뿐 |
| 레이어드 자식 + `UpdateLayeredWindow` | **`Ok`를 반환하면서 아무것도 그리지 않는다** |
| 자식 창 + DirectComposition | 픽셀별 알파가 정상 합성된다 |

글자 가장자리 안티에일리어싱에는 픽셀별 알파가 필요하므로 DirectComposition을 쓴다. 덕분에 벽지가
무엇인지 알 필요가 전혀 없다.

프로세스마다 D3D11 디바이스 하나와 `IDCompositionDevice` 하나를 만들고, 창마다
`IDCompositionTarget` + `IDCompositionVisual` + `IDCompositionSurface`를 둔다.
`surface.BeginDraw()`가 돌려주는 오프셋을 모든 그리기 좌표에 더해야 한다 — 표면이 아틀라스의 일부일
수 있다.

#### 크기와 z-순서

**창 크기는 모니터 전체가 아니라 글자 바운딩 박스 + 그림자 여백만큼만 잡는다.** 실제로 칠하는 건
글자 몇 개뿐이다. 문자열 폭이 변하면 `SetWindowPos`로 창을 다시 잡고 표면을 다시 만들어 가운데
정렬을 유지한다.

**단, 위치·크기가 그대로면 `SetWindowPos`를 부르지 않는다.** `tabular_figures` 덕에 초가 바뀌어도
글자 폭은 그대로라 매초 같은 rect로 다시 놓게 되는데, 움직이지 않는 창을 `SetWindowPos`하는 것은
공짜가 아니다: 자식 창이 무효화되고 Explorer가 그 자리의 벽지를 다시 칠하며, DirectComposition
내용은 그 다음 프레임에야 다시 합성된다 — 초당 한 번 벽지색 프레임이 스치는 깜빡임이 된다.
마지막으로 놓은 rect(부모 클라이언트 좌표)를 기억했다가 달라졌을 때만 호출한다. z-순서도 건드리지
않는다(`SWP_NOZORDER`). 창 스타일은 생성 후 바뀌지 않으므로 `SWP_FRAMECHANGED`도 쓰지 않는다.

Wallpaper Engine 같은 다른 벽지 앱도 같은 WorkerW에 자식 창을 붙인다. 실측에서는 우리 창이 그 위에
남았지만 보장된 순서는 아니므로, 매 갱신마다 우리 창 위에 **남의 창**이 있는지 확인하고 있으면
`SetWindowPos(HWND_TOP)`으로 올린다. "내가 최상단 자식인가?"로 판정하면 안 된다 — 모니터가 여럿이면
우리 자식 창도 여럿이고 그중 하나만 최상단일 수 있어서, 나머지가 매 갱신마다 서로를 밀어내며
z-순서를 영원히 뒤섞는다(그 자체가 벽지 재도색을 유발한다).

프로세스는 시작 시 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`를 호출한다. WorkerW
좌표계는 가상 데스크톱의 물리 픽셀이고, 이 환경에서는 원점이 음수다(최좌측 모니터가 X=-3840).

## 4. 설정

위치: `%APPDATA%\DesktopCountdown\config.toml`. 없으면 기본값으로 생성한다.

```toml
target = "2026-10-24T09:00:00"

[style]
font_family = "Consolas"
font_weight = 400          # 100..900
size_px = 64               # 물리 픽셀
mode = "fill"              # fill | outline | both
color = "#FFFFFF"
outline_color = "#000000"
outline_width_px = 1.5
opacity = 0.85             # 0.0..1.0
letter_spacing_em = 0.02
shadow = true
tabular_figures = true
show_summary_line = true

[layout]
anchor = "center"          # top-left | top-center | top-right
                           # middle-left | center | middle-right
                           # bottom-left | bottom-center | bottom-right
offset_px = [0, 0]         # 앵커 적용 후 더할 오프셋. +x는 오른쪽, +y는 아래.

[general]
autostart = false

# 모니터별 오버라이드. 없으면 전역값을 그대로 쓴다.
[[display]]
id = "MONITOR\\DEL41A8\\{4d36e96e-e325-11ce-bfc1-08002be10318}\\0001"
name = "DISPLAY1 (세로)"
enabled = true
anchor = "top-center"
size_px = 48
```

### 4.1 모니터 식별

`\\.\DISPLAY1` 같은 이름은 케이블을 바꿔 꽂거나 모니터를 껐다 켜면 뒤바뀐다. 대신
`EnumDisplayDevicesW`를 `EDD_GET_DEVICE_INTERFACE_NAME`으로 호출해 얻는 디바이스 인터페이스 이름
(`\\?\DISPLAY#DEL41A8#...`)을 키로 쓰고, 사람이 읽을 이름은 `name`에 따로 저장한다.

`name`은 표시 전용이며 식별에 쓰지 않는다. 1차 버전에서는 `\\.\DISPLAY1 (2560×1440)` 형태로 생성한다.

설정 파일에 없는 모니터가 새로 연결되면 전역 기본값으로 표시한다(`enabled` 기본값은 `true`).

### 4.2 병합 규칙

`[[display]]` 항목에 **존재하는 필드만** 전역값을 덮어쓴다. 오버라이드 가능한 필드는 `enabled`,
`anchor`, `offset_px`, 그리고 `[style]`의 모든 필드다. 스타일 필드는 `[[display]]` 안에서 중첩 없이
같은 층위에 쓴다(위 예시의 `size_px`처럼). `target`과 `[general]`은 전역 전용이다.

### 4.3 앵커 기준

앵커는 **모니터의 전체 사각형** 기준이다. 작업 영역(work area)이 아니다.

`center`는 텍스트 블록의 중심을 모니터 중심에 맞추고, `top-left`는 블록의 좌상단을 모니터 좌상단에
맞춘다. 나머지도 같은 규칙이다. 기본 여백은 0이며, 가장자리에서 띄우려면 `offset_px`를 쓴다.

하단 앵커는 작업표시줄에 가려질 수 있다. 배경 레이어는 작업표시줄 아래에 있기 때문이다.
`offset_px`의 y를 음수로 주어 올린다.

### 4.4 크기와 DPI

`size_px`는 물리 픽셀 기준이다. DPI 배율이 다른 모니터에서 체감 크기가 다르면 그 모니터에만
오버라이드로 조정한다.

전역값에 DPI 배율을 자동으로 곱하지 않는다. 자동 배율과 오버라이드가 겹치면 "왜 이 모니터만 크지"를
설명하는 규칙이 두 겹이 되고, 사용자가 오버라이드로 되돌리려 할 때 어떤 값을 넣어야 하는지 알 수 없다.

## 5. 데이터 흐름

### 5.1 시작

DPI 인식 설정 → 단일 인스턴스 뮤텍스 → 설정 로드 → 모니터 열거 → WorkerW 확보 →
활성 모니터마다 자식 창 생성 → 트레이 아이콘 등록 → 타이머 시작.

### 5.2 매 틱

현재 시각으로 카운트다운을 계산해 두 줄 문자열을 만든다. **이전 프레임과 문자열이 같으면 렌더를
건너뛴다.** 다르면 각 창에 그리고 `UpdateLayeredWindow`로 올린다.

타이머는 고정 1000ms 간격이 아니라 **다음 초 경계에 맞춰 깨운다.** 고정 간격이면 드리프트가 쌓여
초가 건너뛰는 순간이 생긴다.

### 5.3 외부 변화

| 이벤트 | 대응 |
|---|---|
| `config.toml` 변경 | `notify` 감시 → 파일 이벤트가 컨트롤러 창에 `WM_CONFIG_DIRTY`를 post(폴링 없음) → 80ms 디바운스 타이머 → 재파싱 + 즉시 재렌더(다음 틱을 기다리지 않는다). 창 재생성은 *패널을 띄울 모니터 집합*이 바뀐 경우에만 한다 — 스타일·앵커·offset 변경은 재렌더/재배치로 충분하고, 재생성하면 슬라이더 드래그마다 창이 파괴·재생성되어 깜빡인다. |
| `WM_DISPLAYCHANGE` | 모니터 재열거 → 창 재배치 |
| `WM_DPICHANGED` | 해당 모니터 창 재배치 |
| Explorer 재시작 | 2초 주기로 부모 WorkerW 유효성 확인. 사라졌으면 처음부터 재확보 후 재부착. |

## 6. 에러 처리

원칙: **마지막으로 유효했던 상태를 유지한다.**

| 상황 | 대응 |
|---|---|
| 설정 TOML 파싱 실패 | 이전 설정 유지. 트레이 아이콘에 경고 표시, 로그에 이유 기록. |
| 설정한 폰트가 없음 | 시스템 기본 고정폭 폰트로 폴백. 로그 기록. |
| WorkerW 확보 실패 | 지수 백오프 재시도(최대 60초). 그 후 트레이에 실패 상태 표시. |
| D3D11/DirectComposition 디바이스 소실 | D3D11 디바이스부터 재생성해 타깃·비주얼·표면을 다시 만든다. 실패하면 다음 틱에 재시도. |
| 패닉 | panic hook이 로그를 남기고 트레이 아이콘을 정리한 뒤 종료. |

로그는 `%LOCALAPPDATA%\DesktopCountdown\log.txt`에 `tracing`으로 남긴다. 콘솔 창이 없는 앱이라
로그 파일이 유일한 진단 수단이다.

## 7. 테스트 전략

Win32 경계선을 따라 갈린다.

**단위 테스트 (순수 모듈):**

- `countdown`: 정확히 0이 되는 순간, 그 1초 전, 만료 후 `00:00:00` 유지.
  총 시간이 `100:00:00` → `99:59:59`로 자릿수가 줄 때.
  월말 경계(1/31 → 2월), 윤년 2월 29일 통과.
- `config`: 오버라이드 없음 / 일부만 있음 / 전부 있음. 잘못된 색상·범위 밖 opacity 거부.
- `layout`: 음수 좌표 모니터(X=-3840), 세로 모니터(1440×2560)에서 9개 앵커 × 오프셋 조합.

**렌더 스모크 테스트:** `render`는 그리는 대상을 `&ID2D1RenderTarget`으로만 받으므로, 테스트는 WIC
비트맵 렌더 타깃을 넘겨 오프스크린으로 그린 뒤 픽셀을 읽는다. 알파 커버리지가 0보다 큰지, 글자
바운딩 박스가 기대한 사각형 안에 들어오는지 확인한다. DirectComposition은 관여하지 않는다.

`dcomp` 모듈은 별도 스모크 테스트를 갖는다: 디바이스 생성, 숨은 최상위 창에 타깃 부착, 표면에
한 번 그리기까지가 성공하는지. WorkerW는 필요 없다.

**픽셀 단위 골든 이미지 비교는 하지 않는다.** 폰트 힌팅과 안티에일리어싱이 머신마다 달라 깨지는
테스트가 된다.

**수동 검증:** 스파이크(§8)와, 4개 모니터에서의 실제 표시 확인.

## 8. 스파이크 (완료)

구현 첫 작업으로 수행했다. **A안(레이어드 자식 창 + `UpdateLayeredWindow`)은 실패했고,
DirectComposition으로 전환했다.** 상세 결과와 재현 방법은
`docs/superpowers/plans/spike-result.md`에 있으며, 검증에 쓴 진단 바이너리는 커밋 `64f08cc`에 남아 있다.

확인된 것:

- 벽지 레이어 창은 `Progman`의 자식 `WorkerW`다. `0x052C`는 이미 존재하므로 무효였다.
- `CreateWindowEx`에 타 프로세스 창을 부모로 넘길 수 없다. `SetParent`를 써야 한다.
- `UpdateLayeredWindow`는 자식 창에서 `Ok`를 반환하면서 아무것도 그리지 않는다.
  같은 코드가 최상위 창에서는 정상 동작하므로, 픽셀 버퍼는 문제가 아니었다.
- 평범한 자식 창은 Explorer의 벽지 재도색에 덮인다.
- DirectComposition 자식 창은 픽셀별 알파를 정상 합성한다. Wallpaper Engine 표면 위, 데스크톱
  아이콘 아래에 그려지고, 바탕화면 새로 고침에도 유지된다.

## 9. 폴백: 벽지 직접 합성 (채택하지 않음)

DirectComposition이 깨질 경우에만 꺼내 쓴다. 지금은 필요 없다.

`WS_EX_LAYERED` 자식 창에 `SetLayeredWindowAttributes(alpha 255)`를 걸어 완전 불투명하게 만든 뒤,
`IDesktopWallpaper` COM으로 모니터별 벽지 경로와 맞춤 모드(채우기 / 맞춤 / 늘이기 / 바둑판 / 가운데 /
걸치기)를 얻어 WIC로 디코드하고, 창 영역에 해당하는 부분만 잘라 배경으로 깐 다음 그 위에 글자를 그린다.
이 방식이 실제로 보이고 바탕화면 새로 고침에도 유지되는 것은 스파이크에서 확인했다.

**평범한(레이어드 아닌) 자식 창으로는 안 된다** — Explorer의 벽지 재도색에 덮인다.

추가로 필요한 것: 맞춤 모드별 소스 사각형 계산(순수 함수, 단위 테스트 대상), 벽지 변경 감지
(`WM_SETTINGCHANGE`), 단색 배경 처리(`IDesktopWallpaper::GetBackgroundColor`), 슬라이드쇼 대응.
DirectWrite 렌더링 코드, 설정, 트레이, 시간 계산, 레이아웃은 그대로 공유한다.

## 10. 비목표

이번 버전에서 하지 않는다.

- 여러 목표 / 반복 목표
- 애니메이션·전환 효과
- 토스트 알림
- 항상-위(always-on-top) 모드
- 다국어 (UI는 한국어)
- 벽지 슬라이드쇼 대응 (DirectComposition에서는 애초에 불필요)

`§5.2`의 "문자열이 같으면 렌더 스킵"과 `§3.3`의 "창을 글자 크기만큼만" 두 결정은 애니메이션을
붙이려면 되돌려야 한다. 애니메이션이 비목표이므로 지금은 이 최적화를 택한다.

DirectComposition을 쓰게 되면서 애니메이션·블러·트랜지션이 기술적으로는 훨씬 가까워졌다. 그래도
1차 버전에서는 하지 않는다.

## 11. 크레이트

| 크레이트 | 용도 |
|---|---|
| `windows` | Win32, Direct2D, DirectWrite, DirectComposition, Direct3D11, DXGI, WIC, COM, 레지스트리 |
| `windows-numerics` | `Vector2` — `windows` 0.62의 D2D 좌표 타입. 별도 크레이트라 직접 의존성이 필요하다 |
| `jiff` | 로컬 시간대 civil datetime 계산 |
| `serde`, `toml` | 설정 직렬화 |
| `notify` | 설정 파일 감시 |
| `tray-icon` | 트레이 아이콘 |
| `eframe` / `egui` | 설정 창 (별도 프로세스) |
| `tracing`, `tracing-appender` | 로깅 |
| `thiserror`, `anyhow` | 에러 타입 / 전파 |

## 12. 알려진 리스크

**WorkerW 트릭은 비공식 동작이다.** `0x052C`는 문서화되지 않은 메시지이고, Explorer의 창 구조는
Windows 버전마다 다르다 — 실제로 개발 환경에서는 문서와 예제가 전제하는 "최상위 WorkerW 형제"가
존재하지 않았다. 두 가지 배치를 모두 다루고(§3.3), 부모 창 유효성 감시(§5.3)로 Explorer 재시작을
복구한다. 그래도 구조가 또 바뀌면 `workerw` 모듈을 고쳐야 한다. 이 리스크는 배경 레이어 표시를
선택한 대가로 받아들인다.

**`UpdateLayeredWindow`는 자식 창에서 조용히 실패한다.** `Ok`를 반환하고도 아무것도 그리지 않는다.
스파이크에서 확인했다. 혹시 나중에 누군가 "레이어드 창으로 하면 더 간단한데" 하고 되돌리려 한다면,
이 문장이 그 이유다.

**DirectComposition 디바이스는 소실될 수 있다.** GPU 드라이버 갱신이나 원격 데스크톱 전환 시
`DXGI_ERROR_DEVICE_REMOVED`가 난다. D3D11 디바이스부터 다시 만들어 타깃·비주얼·표면을 재구성한다.
§6의 재시도 정책이 이를 다룬다.

**다른 벽지 앱과 같은 부모를 공유한다.** Wallpaper Engine도 같은 WorkerW에 자식 창을 붙인다.
실측에서는 우리 창이 위에 남았지만 보장은 없으므로 매 갱신마다 최상단 자식인지 확인한다(§3.3).
