# 스파이크 결과: 바탕화면 배경 레이어에 픽셀별 알파 그리기

- 실행일: 2026-07-10
- 환경: Windows 11 Pro build 26200, 모니터 4대, Wallpaper Engine 상시 사용

> **주의:** 아래에 적힌 창 좌표·크기는 진단 바이너리가 DPI 인식을 설정하지 않은 채 얻은 **가상화된
> (논리) 값**이다. 제품 코드는 Per-Monitor V2로 물리 픽셀을 본다. Task 8에서 DPI 인식 상태로 실측한
> 진짜 레이아웃은 DISPLAY2 2560x1440 @(0,0) / DISPLAY1 1440x2560 @(2560,-556) /
> DISPLAY3 3840x2160 @(-3840,-368) dpi 144 / DISPLAY4 2560x1440 @(0,1440) 이다.
> 창 트리의 부모-자식 관계와 클래스 이름은 DPI와 무관하므로 그대로 유효하다.
- 관련: 설계 문서 §8(스파이크), §9(폴백), §12(리스크)

## 결론

**설계 §8의 A안(`WS_EX_LAYERED` 자식 창 + `UpdateLayeredWindow`)은 실패했다. 폐기한다.**

**DirectComposition(설계에서 C안으로 언급한 것)이 동작한다. 이걸로 간다.**

부수적으로, 설계와 계획이 전제한 WorkerW 확보 방법과 창 생성 방법이 **둘 다 이 시스템에서 틀렸다.**

## 발견 1 — WorkerW 탐색 알고리즘이 틀렸다

계획의 `find_workerw()`는 최상위 창 중 `SHELLDLL_DefView`를 자식으로 가진 것을 찾고, 그 **다음 형제**
최상위 `WorkerW`를 잡는다. 이 시스템의 실제 구조는 다르다:

```
Progman (explorer.exe, 가상 데스크톱 전체 -3840,-556 .. 4000,2880)
├─ [z0] SHELLDLL_DefView          ← 아이콘. Progman의 직접 자식이다.
│    └─ SysListView32
└─ [z1] WorkerW  0xfe0fdc         ← 벽지 레이어. Progman의 자식이다.
     └─ WPEDesktopDX11Window × 4  ← Wallpaper Engine (wallpaper64.exe)
```

- `SHELLDLL_DefView`의 부모는 `WorkerW`가 아니라 `Progman` 자신이다.
- `Progman` 다음의 최상위 `WorkerW` 형제는 **존재하지 않는다.**
- `0x052C` 메시지는 `(0,0)`, `(0xD,1)`, `(0xD,0)` 셋 다 **무효**다. 보내기 전후 창 트리가 완전히 같고
  `SendMessageTimeoutW`의 결과값이 0이다. WorkerW가 이미 존재하기 때문이다.

따라서 올바른 탐색은 `FindWindowExW(Some(progman), None, "WorkerW", None)` — **Progman의 자식**이다.
고전 알고리즘을 먼저 시도하고 실패하면 이쪽으로 폴백해야 한다.

Wallpaper Engine이 자기 렌더 창을 정확히 이 `WorkerW` 아래에 붙여둔 것이 결정적 단서였다.

## 발견 2 — `CreateWindowEx`에 다른 프로세스의 창을 부모로 넘길 수 없다

`CreateWindowExW(..., WS_CHILD, ..., Some(workerw), ...)`는 `NULL`을 반환한다. `GetLastError()`가 0이라
에러 메시지가 "작업을 완료했습니다"라는 무의미한 문자열로 나온다.

올바른 방법(Lively, Wallpaper Engine이 쓰는 것):

1. 부모 없이 `WS_POPUP`으로 최상위 창을 만든다.
2. `SetParent(hwnd, workerw)`.
3. `SetWindowLongPtrW(hwnd, GWL_STYLE, WS_CHILD | WS_VISIBLE)` — `SetParent`는 스타일을 고치지 않는다.
4. `SetWindowPos(..., SWP_FRAMECHANGED)` + `ShowWindow(hwnd, SW_SHOW)`.

좌표는 `ScreenToClient(workerw, ..)`로 부모 클라이언트 좌표로 변환한다. 이 시스템에서 화면 (100,100)은
클라이언트 (3940, 656)이다.

## 발견 3 — 그리기 방식 4종 비교 (실측)

네 창을 동시에 띄워 눈으로 확인했다. Wallpaper Engine은 꺼진 상태.

| 창 | 종류 | 알파 방식 | 결과 |
|---|---|---|---|
| A | WorkerW의 평범한 자식 | 없음 (GDI `WM_PAINT`) | ❌ 안 보임 |
| B | WorkerW의 레이어드 자식 | `SetLayeredWindowAttributes` (균일) | ✅ 보임 |
| C | WorkerW의 레이어드 자식 | `UpdateLayeredWindow` (픽셀별) | ❌ 안 보임 |
| D | **최상위** 레이어드 창 | `UpdateLayeredWindow` (픽셀별) | ✅ 보임 |

**D가 보이므로 ULW 호출 코드와 프리멀티플라이드 BGRA 비트맵 자체는 정상이다.** C만 안 보인다는 것은
`UpdateLayeredWindow`가 **자식 창에서는 조용히 아무것도 하지 않는다**는 뜻이다 — 호출은 `Ok(())`를
반환한다. 이것이 설계 §8 A안이 실패한 이유다.

A가 안 보이는 이유: `WorkerW`에 `WS_CLIPCHILDREN`이 없어서 Explorer가 벽지를 다시 칠할 때 평범한 자식
창의 픽셀을 덮는다. Wallpaper Engine의 DX11 창이 살아남는 것은 초당 수십 번 present하기 때문이다.
**즉 배경 레이어에서 안정적으로 합성되는 자식 창은 별도 DWM 비주얼을 갖는 것뿐이다.**

D 방식(최상위 레이어드 창)은 요구사항을 만족할 수 없다. 최상위 창은 `Progman` 아래로 내릴 수 없고
(내리면 벽지에 완전히 가려진다), 위에 두면 아이콘과 다른 창을 덮으며 바탕화면 드래그를 막는다.
실제로 D를 띄웠을 때 그 두 증상이 그대로 나타났다.

## 발견 4 — DirectComposition은 동작한다

WorkerW의 **평범한** 자식 창(레이어드 아님, `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`)에:

```
D3D11CreateDevice(HARDWARE, BGRA_SUPPORT)        -> ok
DCompositionCreateDevice(dxgi_device)            -> ok
device.CreateTargetForHwnd(child_hwnd, true)     -> ok    <- 관문
device.CreateVisual() / CreateSurface(B8G8R8A8_UNORM, PREMULTIPLIED)
surface.BeginDraw() -> IDXGISurface -> ID2D1DeviceContext로 그리기
visual.SetContent(surface); target.SetRoot(visual); device.Commit()
```

결과: 불투명 자홍색 **원**(사각형이 아님 → 원 주변이 투명해 벽지가 비침), 알파 0.5 흰 막대(벽지가
반쯤 비침). **바탕화면 새로 고침(F5) 후에도 유지된다.**

부수 확인: 불투명 레이어드 자식 창(`SetLayeredWindowAttributes`, alpha 255) + GDI 페인트도 보이고
F5 후 유지된다. 즉 설계 §9 폴백도 여전히 유효하다 — 단 평범한 자식 창이 아니라 **레이어드** 자식
창이어야 한다는 수정이 필요하다.

## 발견 5 — `windows` 0.62 API 차이

- `implement` 피처가 없다. `#[implement]` 매크로는 `windows::core::implement`로 무조건 노출된다.
- `DefWindowProcW`는 Rust ABI 함수라 `WNDCLASSW::lpfnWndProc`에 직접 대입할 수 없다.
  `extern "system"` 트램폴린이 필요하다.
- `D2D1_ELLIPSE::point`와 `DrawTextLayout(origin)`의 타입은 `D2D_POINT_2F`가 아니라 별도 크레이트의
  `windows_numerics::Vector2`(필드 `X`, `Y`)다. `windows-numerics = "0.3"`을 직접 의존성으로 넣어야 한다.
- `SendMessageTimeoutW`의 `wparam`/`lparam`은 `Option`으로 감싸지 않는다.

## 미확인 — 남은 검증 항목

- [x] Wallpaper Engine이 **켜진** 상태에서 DirectComposition 자식 창이 WE 표면 위에 유지되는가.
      **확인됨.** WE를 다시 켜자 WE의 새 렌더 창들이 우리 창 아래로 들어갔고 우리 내용이 그 위에 그대로 보인다.
      다만 이 순서가 보장된 것은 아니므로, 매 갱신 시 우리 창이 부모의 최상단 자식인지 확인하고
      아니면 `SetWindowPos(HWND_TOP)`으로 올리는 값싼 안전장치를 둔다.
- [x] 바탕화면 아이콘이 DirectComposition 자식 창 **위에** 그려지는가 — **확인됨.**
- [x] 다른 창을 띄우면 가려지는가 — 확인됨. 스파이크를 보려면 `Win+D`가 필요했다.
남은 항목은 구현 태스크에서 다룬다.

- [ ] 음수 좌표 모니터(DISPLAY3, x=-3840)와 세로 모니터에서의 배치.
- [ ] Explorer 재시작 후 재부착.
- [ ] 모니터 연결/해제, DPI 변경.
- [ ] DirectComposition 디바이스 소실 후 재생성.

## 후속 조치

설계 문서 §3.3, §8, §9, §11, §12와 구현 계획의 Task 1·9·10·11을 DirectComposition 방식으로 다시 쓴다.
`UpdateLayeredWindow`, DIB 섹션, 프리멀티플라이드 CPU 버퍼는 전부 사라진다.
