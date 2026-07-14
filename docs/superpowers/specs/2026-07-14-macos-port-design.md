# macOS 포팅 설계

> 상태: 승인됨 (2026-07-14). 구현 미착수.
> 개발은 macOS 실기에서 진행한다. 이 문서는 그 세션을 위한 설계 원본이다.

## 0. 한 줄 요약

같은 리포 안에서 플랫폼 백엔드를 분리하고(`src/platform/{windows,macos}`), macOS 백엔드를
AppKit(`NSWindow` 데스크톱 레벨) + CoreText/CoreGraphics로 새로 쓴다. 도메인 로직·설정
창·config 포맷은 그대로 공유한다. Windows 렌더러는 손대지 않는다.

---

## 1. 확정된 결정사항

| # | 결정 | 근거 |
|---|---|---|
| D1 | **같은 리포, 플랫폼 백엔드 분리.** Windows 코드는 D2D/DirectWrite 그대로 유지, macOS는 새 네이티브 백엔드. | 도메인 로직(config·countdown·tokens·layout·color)과 egui 설정 창이 이미 순수 Rust라 그대로 재사용된다. Windows 릴리스에 회귀 위험 0. |
| D2 | **전체 기능 패리티.** 렌더링·멀티모니터·모니터별 override·트레이·설정 창·라이브 리로드·자동 시작·단일 인스턴스·폰트 피커 전부. | 공유 코드 덕에 축소해서 아끼는 양이 작고, 줄이면 "맥에서는 config를 손으로 편집" 같은 이상한 상태가 된다. |
| D3 | **미서명 배포.** ad-hoc 서명(`codesign -s -`)만 하고 Developer ID·공증(notarization)은 안 한다. | 연 $99 비용 회피. ad-hoc 서명은 **선택이 아니라 필수** — 이유는 §7.3. |
| D4 | **Universal 바이너리** (arm64 + x86_64를 `lipo`로 병합), `.app`을 `ditto` zip으로 릴리스에 첨부. | CI 비용 거의 0. `.app`은 디렉터리라 raw 업로드가 불가능하므로 압축이 필수. |
| D5 | **개발은 macOS 실기에서.** CI는 릴리스 빌드와 회귀 검증용. | 배경화면 레이어 동작은 육안 검증이 필요하고, 헤드리스 러너로는 확인할 수 없다. |

## 2. 비목표 (하지 않는 것)

- **통합 렌더러**(cosmic-text + tiny-skia로 양 OS 렌더러를 하나로) — 잘 도는 Windows 렌더러를
  버리는 회귀 위험이 이득보다 크다.
- **Developer ID 서명 / 공증 / .dmg / Homebrew cask.**
- **Linux 지원.**
- **데스크톱 위젯 인터랙션.** macOS의 데스크톱 레벨 창은 `ignoresMouseEvents`와 무관하게
  **애초에 마우스·키보드 이벤트를 받지 못한다**(플랫폼 제약). 이 앱은 클릭 통과가 요구사항이므로
  문제되지 않지만, 나중에 "위젯 클릭"류를 원하면 창 레벨을 올려야 하고 그건 별개 설계다.

---

## 3. 현재 코드의 경계

전체 7,151줄. `windows::`를 import하는 파일이 13개.

**그대로 공유되는 것 (수정 거의 없음)**
`config/` (schema·io·merge, 844+113+211줄) · `countdown.rs` · `tokens.rs` · `layout.rs` ·
`color.rs` · `logging.rs` · `settings/{app,lines,overrides,widgets}.rs` (egui, 1,690줄)

**플랫폼 백엔드로 내려가는 것**
| 파일 | 하는 일 | macOS 대응물 |
|---|---|---|
| `workerw.rs` (319) | WorkerW 탐색 + 자식 창 + 재획득 backoff | `NSWindow` (데스크톱 레벨). 탐색·backoff·재획득 **전부 불필요** |
| `dcomp.rs` (395) | DirectComposition 서피스 | `CALayer.contents` ← `CGImage` |
| `render/mod.rs` (858) | D2D 페인팅 | CoreGraphics `CGContext` |
| `render/text.rs` (107) | DirectWrite 레이아웃 | CoreText `CTLine` |
| `render/outline.rs` (164) | `IDWriteTextRenderer` COM 콜백으로 글리프 아웃라인 수집 | `CTFontCreatePathForGlyph` — **파일 통째로 소멸** (COM vtable/패닉 UB 우려도 함께) |
| `app.rs` (624) | Win32 메시지 루프 + 타이머 + 상태 | 상태는 공유(`AppCore`), 루프는 `NSApplication` + `NSTimer` |
| `monitors.rs` (160) | `EnumDisplayMonitors` | `NSScreen::screens` |
| `fonts.rs` (195) | DirectWrite 패밀리 열거 + 폰트 파일 경로 | `CTFontManagerCopyAvailableFontFamilyNames` + `kCTFontURLAttribute` |
| `autostart.rs` (97) | HKCU\...\Run | `~/Library/LaunchAgents/*.plist` |
| `single_instance.rs` (48) | 네임드 뮤텍스 | 락 파일 + `flock(LOCK_EX\|LOCK_NB)` |
| `paths.rs` (25) | `%APPDATA%` / `%LOCALAPPDATA%` | `~/Library/Application Support` / `~/Library/Logs` |
| `watch.rs` (183) | notify → `PostMessageW` | notify → 채널/런루프 소스 (notify 자체는 크로스플랫폼) |
| `tray.rs` (132) | exe 리소스에서 아이콘 로드 | 임베드된 PNG → RGBA |
| `backoff.rs` (79) | WorkerW 재시도 | Windows 전용. `platform/windows/`로 이동 |

이미 macOS-safe한 것: `build.rs`는 `CARGO_CFG_WINDOWS` 가드가 있고, `.cargo/config.toml`의
`crt-static`은 `[target.x86_64-pc-windows-msvc]`로 스코프되어 있다. **둘 다 수정 불필요.**

---

## 4. 목표 구조

### 4.1 모듈 트리

```
src/
  main.rs                # --settings 분기 → platform::init() → platform::run()
  lib.rs
  app.rs                 # AppCore: 플랫폼 중립 상태 + 로직 (reload/resolve/tick)
  color.rs  countdown.rs  layout.rs  logging.rs  tokens.rs
  config/                # 그대로
  settings/              # egui, 그대로 (플랫폼 호출은 platform:: 경유)
  paths.rs               # cfg 분기 (§5.9)
  platform/
    mod.rs               # 활성 백엔드 재수출 + 계약 + 공통 타입 MonitorInfo
    windows/
      mod.rs  backoff.rs  dcomp.rs  workerw.rs  monitors.rs  fonts.rs
      autostart.rs  single_instance.rs  tray.rs  watch.rs
      render/{mod,outline,text}.rs
    macos/
      mod.rs  desktop_window.rs  layer.rs  monitors.rs  fonts.rs
      autostart.rs  single_instance.rs  tray.rs  watch.rs
      render/{mod,text}.rs
```

`platform/mod.rs`:
```rust
#[cfg(windows)]           mod windows;
#[cfg(windows)]           pub use windows::*;
#[cfg(target_os = "macos")] mod macos;
#[cfg(target_os = "macos")] pub use macos::*;
```

**진짜 트레이트가 아니라 std의 `sys` 모듈처럼 "덕 타이핑 + 문서화된 계약"으로 간다.**
백엔드가 둘뿐인데 `Composed`가 플랫폼 타입(`IDWriteTextLayout` vs `CTLine`)이라 트레이트로
가면 연관 타입 + 제네릭 배관이 온 코드에 번지고 얻는 게 없다.

### 4.2 플랫폼 계약

각 백엔드는 아래 이름들을 **정확히 같은 시그니처로** 노출한다.

```rust
// --- 공통 타입 (platform/mod.rs에 정의, 두 백엔드가 채운다) ---
pub struct MonitorInfo {
    pub id: String,     // 재부팅·케이블 교체에도 안정적인 식별자. config override 키.
    pub name: String,   // 표시용. 절대 식별에 쓰지 않는다.
    pub rect: Rect,     // 가상 데스크톱 좌표, 물리 픽셀. 음수 가능.
    pub scale: f32,     // Windows: dpi/96.0, macOS: backingScaleFactor
}

// --- 초기화 / 이벤트 루프 ---
pub fn init() -> Result<()>;              // win: SetProcessDpiAwarenessContext
                                          // mac: NSApp activationPolicy = .accessory
pub fn run(core: AppCore) -> Result<()>;  // 블로킹 이벤트 루프. 1초 틱마다 core.tick().

// --- 렌더 ---
pub struct Painter;
impl Painter {
    pub fn new() -> Result<Self>;
    pub fn compose(&self, lines: &[Line], style: &Style) -> Result<Composed>;
}
pub struct Composed;
impl Composed { pub fn size(&self) -> (u32, u32); }

// --- 화면 표면 ---
/// `bool`이 아닌 이유: "계속 붙어 있음"과 "방금 새로 붙음"을 구분해야 한다. 둘 다 "붙었다"지만
/// **후자만 caller에게 rebuild 의무를 지운다** — 백엔드가 옛 서피스와 함께 패널을 버렸기 때문.
/// caller 쪽 `attached: bool` 플래그로는 안 된다: WorkerW는 한 tick 안에서 죽었다가 곧바로
/// 재획득될 수 있고, 그러면 caller는 "안 붙음" 상태를 **한 번도 관측하지 못한다.**
/// (2단계 구현 중 발견. 이대로 두면 WorkerW 재생성 시 화면이 영구히 빈다.)
pub enum Attach { Live, Fresh, Pending }

pub struct Panels;
impl Panels {
    /// `&Painter`를 받는다: Windows compositor의 D2D 디바이스와 painter의 device-independent
    /// 리소스가 **같은 팩토리**에서 나와야 함께 쓸 수 있다. macOS는 무시한다.
    pub fn new(painter: &Painter) -> Result<Self>;
    /// Windows: WorkerW 생존 확인/재획득(backoff 포함).
    /// macOS: 첫 호출에 `Fresh`, 이후 계속 `Live`.
    pub fn ensure_attached(&mut self) -> Result<Attach>;
    /// `wanted`(이미 enabled 필터링됨)에 맞춰 창/서피스를 재구성.
    pub fn rebuild(&mut self, wanted: &[MonitorInfo]) -> Result<()>;
    pub fn monitors(&self) -> &[MonitorInfo];
    /// 패널당 하나씩, 미리 compose된 스택을 그린다. panels와 frames는 lockstep.
    pub fn draw(&mut self, painter: &Painter, frames: &[Frame]) -> Result<()>;
    /// draw 실패 후 컴포지션 디바이스 재생성. caller가 뒤이어 `rebuild`를 부른다.
    /// Windows: Compositor 재생성. macOS: no-op.
    pub fn recover(&mut self) -> Result<()>;
}
pub struct Frame { pub composed: Composed, pub style: Style, pub rect: Rect }

// --- 시스템 ---
pub fn enumerate_monitors() -> Result<Vec<MonitorInfo>>;
pub mod autostart { pub fn is_enabled() -> Result<bool>; pub fn set_enabled(on: bool) -> Result<()>; }
pub mod fonts     { pub fn system_families() -> Result<Vec<String>>;
                    pub fn font_file(family: &str) -> Option<PathBuf>; }
pub struct SingleInstance;  impl SingleInstance { pub fn acquire(name: &str) -> Result<Self>; }
pub struct Tray;            impl Tray { pub fn new() -> Result<Self>;
                                        pub fn poll(&self) -> Option<TrayCommand>;
                                        pub fn set_warning(&self, on: bool) -> Result<()>; }
pub struct ConfigWatcher;   // notify는 공유, 알림 전달 방식만 플랫폼별
```

`ensure_attached`가 WorkerW/backoff를 Windows 백엔드 안에 완전히 가둔다 — `AppCore`는
"붙었는가 / 방금 새로 붙었는가 / 아직 못 붙었는가"만 알면 되고, macOS는 그런 개념이 아예 없다.

`ConfigWatcher::new(path)`는 양쪽 시그니처가 같다. Windows가 쓰는 메시지 ID(`WM_CONFIG_DIRTY`)는
백엔드 내부 상수다. `notify_window(hwnd)`는 Windows 백엔드에만 있고 그 백엔드의 `run()`만 부른다.

`SingleInstance::acquire(name)`는 이름을 받는다. 렌더러(`DesktopCountdown`)와 설정 창
(`DesktopCountdown-Settings`)이 같은 타입을 쓰고, `settings/mod.rs`에 복붙돼 있던 두 번째
뮤텍스는 사라진다.

### 4.3 `AppCore` (플랫폼 중립)

지금 `app.rs`의 `App`에서 Win32 메시지 루프·`wndproc`·`SetTimer`를 걷어낸 나머지:

```rust
pub struct AppCore {
    cfg_path: PathBuf, cfg: Config, target: Zoned,
    painter: Painter, panels: Panels, tray: Tray, watcher: ConfigWatcher,
    last_lines: Option<Vec<Vec<Line>>>,
}
impl AppCore {
    pub fn new(cfg_path: PathBuf) -> Result<Self>;
    pub fn tick(&mut self) -> Result<()>;          // 트레이 폴링 → ensure_attached → render
    pub fn on_config_dirty(&mut self);             // reload + 즉시 render
    pub fn on_display_change(&mut self);           // rebuild_panels
    pub fn wants_quit(&self) -> bool;
}
```

`wndproc`의 `catch_unwind` / `IN_WNDPROC` 재진입 가드는 Win32 콜백 경계 고유의 문제이므로
`platform/windows/mod.rs`에 그대로 남긴다. macOS 루프는 Rust `NSTimer` 콜백에서 도는
`objc2` 클로저이므로 같은 UB 문제가 없다(단, objc 콜백 경계로 패닉이 새면 안 되는 건 동일 —
`catch_unwind`로 감싼다).

### 4.4 `MonitorInfo.dpi` → `scale`

현재 `MonitorInfo.dpi`는 **아무 데서도 읽히지 않는다**(렌더러는 96 DPI 고정, 크기는 물리 px).
macOS Retina에서는 `backingScaleFactor`가 없으면 텍스트가 뭉개지므로, 공통 타입은 `scale: f32`로
정규화하고 macOS 백엔드가 이를 `CALayer.contentsScale`과 비트맵 크기에 반영한다.
Windows 백엔드는 `dpi as f32 / 96.0`을 채워두되 지금처럼 안 써도 된다.

---

## 5. macOS 백엔드 설계

### 5.1 배경화면 창 (`desktop_window.rs`) — `workerw.rs` 대응

**창 레벨 (숫자 확정, `objc2-core-graphics` 0.3.2의 생성 바인딩 = CoreGraphics 헤더)**

| 상수 | 값 |
|---|---|
| `kCGDesktopWindowLevel` | −2147483623 |
| **`kCGDesktopIconWindowLevel`** = desktop + 20 | **−2147483603** ← Finder 아이콘이 여기 |
| `kCGNormalWindowLevel` | 0 |

**`level < kCGDesktopIconWindowLevel`이면 전부 아이콘 아래.** 안전 구간은
`[kCGDesktopWindowLevel − 1, kCGDesktopIconWindowLevel − 1]`.
→ **`kCGDesktopIconWindowLevel - 1`을 쓴다.** (Übersicht는 `kCGDesktopWindowLevel`,
Electron `type:'desktop'`과 `tauri-plugin-desktop-underlay`는 `CGWindowLevelForKey(2) - 1`.
전부 유효하다.)

**`NSWindow` 설정 (Übersicht `UBWindow.m` + Loopaper `DesktopWindow.swift` 교차 검증)**

```
styleMask:            Borderless
backing:              Buffered, defer: false
level:                kCGDesktopIconWindowLevel - 1   (NSWindowLevel = isize로 캐스팅)
collectionBehavior:   CanJoinAllSpaces | Stationary | IgnoresCycle | FullScreenNone
ignoresMouseEvents:   true
isOpaque:             false
backgroundColor:      NSColor::clearColor()
hasShadow:            false
releasedWhenClosed:   false   ← Retained를 우리가 들고 있으므로 필수
canHide:              false
movable:              false
excludedFromWindowsMenu: true
animationBehavior:    None
restorable:           false + disableSnapshotRestoration()
표시:                 orderFrontRegardless()   ← makeKeyAndOrderFront 절대 금지
```

`canBecomeKey`/`canBecomeMain`은 borderless의 AppKit 기본값이 이미 `false`라 서브클래싱 불필요.

**⚠️ 알려진 함정: 상단 메뉴바 틈.** AppKit이 창 프레임을 메뉴바 아래로 constrain해서 화면 상단에
1~2px 틈이 생긴다(Übersicht #541, Sequoia). 해결: `objc2::define_class!`로 `NSWindow`를 서브클래싱해
`constrainFrameRect(_:to:)`가 `screen.frame`을 그대로 반환하게 오버라이드. **`define_class!`가
필요한 유일한 지점이다.** 앵커가 top-* 가 아니면 실제로는 안 보일 수도 있으니, 우선 오버라이드
없이 띄워보고 틈이 확인되면 넣는다.

**Space 처리.** `canJoinAllSpaces`는 창 **하나**를 모든 Space에 공유한다. Space 전환 시 재생성
불필요 — 그게 `canJoinAllSpaces + stationary`의 목적이다. Space별로 다른 배경화면이 깔려 있어도
같은 카운트다운이 그 위에 그려진다.

**권한 불필요.** 데스크톱 레벨 창 생성·그리기에 Accessibility도 Screen Recording도 필요 없다.

### 5.2 렌더러 (`render/`) — `render/` + `dcomp.rs` 대응

**API 매핑 (검증됨)**

| 현재 (Windows) | macOS |
|---|---|
| `IDWriteTextLayout` (한 줄) | `CTLine` ← `CTLineCreateWithAttributedString` |
| `DWRITE_TEXT_METRICS.widthIncludingTrailingWhitespace` | `CTLineGetTypographicBounds()` 반환값 |
| `GetGlyphRunOutline` → `ID2D1PathGeometry` → `GetBounds` (= `ink_span`) | `CTLineGetGlyphRuns` → `CTFontCreatePathForGlyph` → `CGMutablePath` → **`CGPathGetPathBoundingBox`** |
| `DrawTextLayout` (fill) | `CGContextAddPath` + `CGContextDrawPath(Fill)` |
| `DrawGeometry(geom, brush, w)` (stroke) | `CGContextSetLineWidth(px)` + `CGContextDrawPath(Stroke)` |
| `DrawMode::{Fill,Outline,Both}` | `CGPathDrawingMode::{Fill,Stroke,FillStroke}` — **1:1** |
| `SetCharacterSpacing(0, s, 0)` | `kCTTrackingAttributeName` (pt 단위, 값은 그대로 `letter_spacing_em * size_px`) |
| `tabular_figures` | `kCTFontFeatureSettingsAttribute`로 `tnum` |
| `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE` | path 렌더링이면 **무관** (font smoothing이 적용되지 않음) |
| DComp 서피스 (B8G8R8A8 premul) | `CGBitmapContext` (`PremultipliedFirst \| ByteOrder32Little` = BGRA premul) → `CGImage` → `CALayer.contents` |

**`ink_span`은 그대로 살아난다.** `CGPathGetPathBoundingBox`(control point가 아닌 실제 곡선
bounds)가 현재 D2D geometry-bounds 트릭과 **의미가 완전히 동일**하다. 어차피 스트로크용
`CGPath`를 만들므로 추가 비용도 없다. (`CTLineGetBoundsWithOptions(kCTLineBoundsUseGlyphPathBounds)`도
같은 값을 주지만 tracking의 trailing whitespace 포함 여부가 문서에 없어 모호하므로 path bounds를 쓴다.)

**⚠️ 좌표계.** CoreText/CGBitmapContext는 **y-up**(원점 좌하단, ink rect는 baseline 기준).
현재 `ink_top`(line box 상단 기준, y-down)과 기준점이 다르니 변환이 필요하다.
**flip하지 말고 y-up으로 baseline을 계산하는 게 가장 단순하다** (flip하면 text matrix까지 뒤집어야 함).
`CGBitmapContextCreateImage`로 뽑은 `CGImage`는 정상 방향으로 나온다.

**⚠️ 스트로크 방식은 path로.** `kCTStrokeWidthAttributeName`은 단위가 **폰트 크기의 %**라서
`outline_width_px`(절대 px)와 안 맞고 CoreText가 context의 text drawing mode를 덮어쓴다.
path + `CGContextSetLineWidth`가 D2D `DrawGeometry`와 픽셀 의미가 같다.

**그림자.** `CGContextSetShadowWithColor`는 fill/stroke를 따로 그리면 두 번 합성되어 알파가
진해진다. **지금처럼 오프셋 준 별도 패스로 직접 그린다** (SHADOW_OFFSET 2.0px, alpha 0.55, blur 0).
y-up이므로 아래로 내리려면 dy가 **음수**다.

**폰트 weight.** config는 DWrite식 100–900(`u16`), CoreText는 `kCTFontWeightTrait`가 −1.0~1.0.
AppKit `NSFont.Weight` 실측 앵커(ultraLight −0.8, thin −0.6, light −0.4, regular 0.0, medium 0.23,
semibold 0.3, bold 0.4, heavy 0.56, black 0.62)로 piecewise-linear 매핑한다.
**이 수치는 Apple 공식 문서값이 아니다 — 실제 폰트로 검증할 것.**

**패밀리 fallback.** CoreText는 없는 패밀리를 주면 조용히 다른 폰트를 준다. 현재
`resolve_family`처럼 명시적으로 존재 확인 후 fallback한다.
Windows `["Consolas", "Segoe UI"]` → macOS `["SF Mono", "Menlo", "Helvetica Neue"]`.

**화면에 올리기.** `NSWindow.contentView.wantsLayer = true` → `CALayer.setContents(cgimage)`.
`contentsScale = backingScaleFactor`, 비트맵은 `size * scale`로 그린다.
**초당 1회 갱신은 전혀 비싸지 않다** — 수백 KB 텍스처 업로드 1회 + 레이어 리컴포짓.
`drawRect:`로 매번 CoreGraphics를 다시 태우는 것보다 싸고, DComp 흐름과도 가장 비슷하다.

### 5.3 이벤트 루프 (`mod.rs`)

`NSApplication::sharedApplication` → `setActivationPolicy(.accessory)` → `NSTimer`(다음 초 경계까지
남은 시간으로 재무장, `ms_to_next_second` 로직 재사용) → `app.run()`.
`tray-icon`이 `NSApplication` 런루프를 요구하므로 이 구조가 트레이와도 자연히 맞는다.
콜백 안에서 `catch_unwind`로 감싸 objc 경계로 패닉이 새지 않게 한다.

### 5.4 모니터 (`monitors.rs`)

- 열거: `NSScreen::screens(mtm)` → `frame()`, `backingScaleFactor()`.
- **식별자**: `CGDisplayCreateUUIDFromDisplayID`로 얻은 **UUID 문자열**.
  `CGDirectDisplayID`는 재연결/재부팅 시 재할당되므로 **영속 키로 쓰면 안 된다**
  (Windows의 `\\?\DISPLAY#DEL41A8#...`와 같은 역할).
- 변경 감지: `NSApplicationDidChangeScreenParametersNotification`.
  **연달아 여러 번 터진다** → 코얼레스 필요(다음 런루프로 미루거나 짧은 타이머로 debounce).
  재구성 시 AppKit이 **새 `NSScreen` 인스턴스**를 주므로 저장해둔 참조를 갱신해야 한다.
- config의 모니터별 override 키는 플랫폼마다 다른 형식이 된다(Windows 장치 인터페이스 이름 vs
  macOS 디스플레이 UUID). **의도된 것** — 같은 config.toml을 두 OS에서 공유하면 `[style]`·`[[line]]`
  같은 전역 설정은 그대로 먹고, 모니터별 override만 해당 OS에서 매칭된다.

### 5.5 트레이 (`tray.rs`)

`tray-icon` 크레이트가 macOS `NSStatusItem`을 지원한다. `LSUIElement=true`(액세서리 앱) + 메뉴바
아이콘은 Übersicht/Plash의 표준 구성이라 충돌 없다.
Windows는 exe 리소스에서 아이콘을 읽지만(`Icon::from_resource`) macOS는 그게 없으므로
`include_bytes!("../../../assets/icon.png")` → PNG 디코드 → `Icon::from_rgba`.
(`png` 크레이트를 macOS 전용 의존성으로 추가. 메뉴바 아이콘은 18~22px 단색 템플릿 이미지가 관례.)

### 5.6 설정 창 (`settings/`)

egui/eframe는 그대로 돈다. **단 하나의 macOS 고유 처리**: `Info.plist`의 `LSUIElement=true` 때문에
프로세스가 액세서리 정책으로 시작하므로, `--settings` 프로세스에서는 창이 앞으로 나오지 않는다.
→ eframe `NativeOptions`의 `event_loop_builder` 훅에서 winit의
`EventLoopBuilderExtMacOS::with_activation_policy(Regular)`를 부르고, 시작 시
`activate_ignoring_other_apps`. 트레이의 "설정 열기"는 지금처럼
`Contents/MacOS/desktop-countdown --settings`를 spawn한다.

### 5.7 자동 시작 (`autostart.rs`)

**`~/Library/LaunchAgents/com.hajh.desktop-countdown.plist`를 직접 쓴다.**
**SMAppService(macOS 13+)는 쓰지 않는다** — 제대로 된 코드 서명을 요구하고(`-67054`
"Static code signature check failed"), ad-hoc 서명 앱에서 동작한다는 근거가 없다. D3와 충돌.

```xml
<key>Label</key>                 <string>com.hajh.desktop-countdown</string>
<key>ProgramArguments</key>      <array>
  <string>/Applications/DesktopCountdown.app/Contents/MacOS/desktop-countdown</string>
</array>
<key>RunAtLoad</key>             <true/>
<key>KeepAlive</key>             <false/>   <!-- true면 트레이 "종료"가 무의미해짐 -->
<key>LimitLoadToSessionType</key><string>Aqua</string>
<key>ProcessType</key>           <string>Interactive</string>
```

함정:
- plist는 `.app` **안의 실행 파일**(`Contents/MacOS/<CFBundleExecutable>`)을 가리켜야 한다.
  `.app` 디렉터리를 넣으면 launchd가 실행하지 못한다.
- **절대 경로가 박힌다.** `std::env::current_exe()`로 런타임에 생성하고, 앱 시작 시 plist 안의
  경로가 현재 경로와 다르면 다시 쓴다(사용자가 앱을 옮긴 경우).
- plist를 두기만 해도 **다음 로그인부터 자동 로드**된다. 즉시 반영하려면
  `launchctl bootstrap gui/$UID <plist>` / 해제는 `bootout gui/$UID/<Label>`.
  `bootstrap`은 이미 로드돼 있으면 실패하므로 에러를 무시하거나 먼저 `bootout`.
- 사용자가 시스템 설정 → 일반 → 로그인 항목에서 끌 수 있고 그러면 plist는 남아도 실행되지 않는다.
  **앱 내 토글의 신뢰 가능한 상태 소스로 삼지 말 것.**

### 5.8 단일 인스턴스 (`single_instance.rs`)

락 파일 + `flock(fd, LOCK_EX | LOCK_NB)`. 이름 두 개(렌더러 / 설정 창)를 인자로 받아
Windows의 `Local\DesktopCountdown` / `Local\DesktopCountdown-Settings`에 대응.
프로세스가 죽으면 커널이 락을 자동 해제하므로 stale 락 문제가 없다.
(현재 `settings/mod.rs`에 복붙되어 있는 두 번째 뮤텍스도 이 참에 `SingleInstance::acquire(name)`으로 통합.)

### 5.9 경로 (`paths.rs`)

| | Windows | macOS |
|---|---|---|
| config | `%APPDATA%\DesktopCountdown\config.toml` | `~/Library/Application Support/DesktopCountdown/config.toml` |
| 로그 | `%LOCALAPPDATA%\DesktopCountdown\` | `~/Library/Logs/DesktopCountdown/` |

### 5.10 폰트 (`fonts.rs`)

- 열거: `CTFontManagerCopyAvailableFontFamilyNames()` — "UI 표시용으로 정렬된 가시 패밀리 이름".
- **`font_file(family)`도 반드시 구현해야 한다.** 설정 창이 각 패밀리 이름을 그 폰트로 그리려고
  실제 폰트 파일을 egui에 로드한다(`settings/app.rs:102`, `settings/mod.rs:113`).
  → `CTFontDescriptor`의 `kCTFontURLAttribute`로 파일 경로를 얻는다.
  기존 `skrifa` 사전 검증 로직은 그대로 재사용된다.

### 5.11 config 감시 (`watch.rs`)

`notify` 크레이트는 크로스플랫폼(macOS는 FSEvents). 바뀌는 건 **알림 전달 방식**뿐:
Windows는 `PostMessageW(hwnd, WM_CONFIG_DIRTY)`, macOS는 `AtomicBool`/채널을 세우고 다음 틱
또는 짧은 `NSTimer`에서 집어간다. 80ms 디바운스 의미는 동일하게 유지.

---

## 6. Cargo.toml

```toml
[target.'cfg(windows)'.dependencies]
windows          = { version = "0.62", features = [ ... 기존 그대로 ... ] }
windows-core     = "0.62"
windows-numerics = "0.3"

[target.'cfg(windows)'.build-dependencies]
winresource = "0.1.31"   # ⚠ 아래 주의

[target.'cfg(target_os = "macos")'.dependencies]
objc2                 = "0.6"    # 0.6.4
objc2-foundation      = "0.3"    # 0.3.2
objc2-app-kit         = "0.3"
objc2-core-foundation = "0.3"
objc2-core-graphics   = "0.3"
objc2-core-text       = "0.3"
objc2-quartz-core     = "0.3"    # CALayer
libc                  = "0.2"    # flock, getuid
png                   = "0.17"   # 트레이 아이콘 디코드
```

**⚠ `build-dependencies`의 `cfg(windows)`는 타깃이 아니라 *호스트*를 본다** (빌드 스크립트는
호스트용으로 컴파일되니까). 맥에서 `--target x86_64-pc-windows-msvc`로 크로스 컴파일하면
`winresource`가 **없다.** 그래서 `build.rs`의 사용부도 `#[cfg(windows)]`로 갈라야 한다
(2단계에서 반영됨). 크로스 빌드 결과물은 아이콘이 안 박히지만, 릴리스는 Windows 러너에서
빌드하므로 실제로는 문제되지 않는다 — 대신 경고를 찍어 조용히 넘어가지 않게 한다.

**맥에서 Windows 코드를 타입체크할 수 있다.** `cargo check --target x86_64-pc-windows-msvc
--all-targets`가 (링크 없이) 통과한다 — 32초. `clippy`/`fmt`도 마찬가지. 2단계처럼 Windows 코드를
대거 옮기는 작업의 **유일한 로컬 안전망**이므로, macOS 세션에서는 rustup으로 이 타깃을 반드시 깔 것.
(테스트 **실행**은 여전히 불가능하다 — D2D/WorkerW/실제 데스크톱이 필요하다. CI에 맡긴다.)

**레거시 `cocoa` / `core-graphics` / `core-text`(servo) 크레이트는 쓰지 않는다.**
`objc2-app-kit`이 넘겨주는 `CGContext`/`CGImage` 타입이 `objc2-core-graphics`의 것이라
servo 크레이트와 섞으면 타입이 충돌한다. objc2 계열로 통일.
(servo `core-foundation-rs` 메인테이너 본인이 objc2로의 대체를 공지했다.)

---

## 7. CI / 릴리스

### 7.1 CI (`ci.yml`)

`runs-on`을 매트릭스로: `windows-latest` + `macos-15`. 양쪽에서 `fmt --check`, `clippy -D warnings`, `test`.

### 7.2 릴리스 (`release.yml`)

Windows 잡이 `gh release create`, macOS 잡이 `needs: windows` 후 `gh release upload`.
(양쪽에서 `create`를 부르면 충돌한다.)

```yaml
  macos:
    needs: release          # Windows 잡이 릴리스를 먼저 만든다
    runs-on: macos-15
    env:
      MACOSX_DEPLOYMENT_TARGET: "11.0"   # LSMinimumSystemVersion과 반드시 일치
      APP: DesktopCountdown.app
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: x86_64-apple-darwin,aarch64-apple-darwin }
      - uses: Swatinem/rust-cache@v2
      - run: cargo test                    # 호스트(arm64)에서만
      - run: |
          cargo build --release --target aarch64-apple-darwin
          cargo build --release --target x86_64-apple-darwin
      - name: Assemble the .app bundle
        run: |
          mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
          lipo -create -output "$APP/Contents/MacOS/desktop-countdown" \
            target/aarch64-apple-darwin/release/desktop-countdown \
            target/x86_64-apple-darwin/release/desktop-countdown
          chmod +x "$APP/Contents/MacOS/desktop-countdown"
          cp macos/Info.plist "$APP/Contents/Info.plist"
          /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${GITHUB_REF_NAME#v}" \
            "$APP/Contents/Info.plist"
          printf 'APPL????' > "$APP/Contents/PkgInfo"
          ICONSET="$(mktemp -d)/AppIcon.iconset"; mkdir -p "$ICONSET"
          for s in 16 32 128 256 512; do
            sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
            sips -z $((s*2)) $((s*2)) assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
          done
          iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
      - name: Ad-hoc sign          # 필수. §7.3 참조. 공증이 아니다.
        run: |
          codesign --force --deep --sign - "$APP"
          codesign --verify --deep --strict --verbose=2 "$APP"
      - name: Zip                  # zip이 아니라 ditto
        run: ditto -c -k --sequesterRsrc --keepParent "$APP" DesktopCountdown-macos-universal.zip
      - run: gh release upload "$GITHUB_REF_NAME" DesktopCountdown-macos-universal.zip
        env: { GH_TOKEN: "${{ github.token }}" }
```

- **plist 수정은 서명을 깨뜨린다** → `PlistBuddy`는 반드시 `codesign` **전에**.
- `--target`을 주면 출력이 `target/<triple>/release/`로 간다(기존 `target/release/` 가정과 다름).
- `MACOSX_DEPLOYMENT_TARGET`을 명시하지 않으면 러너 SDK 기본값(최신)이 잡혀 구형 macOS에서 안 뜬다.
- 잡을 나눠 `upload-artifact`로 넘기면 **실행 권한 비트가 날아간다**. 한 잡에서 빌드→번들→업로드까지 끝낸다.

### 7.3 ad-hoc 서명이 필수인 이유

1. Apple 공식: **Apple 실리콘은 유효한 서명이 없으면 네이티브 arm64 코드의 실행을 허용하지 않는다.**
   (ad-hoc으로 충분하다.) 링커가 자동으로 붙여주지만 —
2. **`lipo`로 슬라이스를 합치면 번들 서명이 깨진다** → 재서명 필요.
3. macOS는 서명 없는 바이너리를 첫 실행 시 자동 ad-hoc 서명해주지만 **`com.apple.quarantine`가
   안 붙어 있을 때만** 그렇다. 다운로드된 앱은 quarantine이 붙으므로 이 구제가 적용되지 않고
   → **"앱이 손상되었습니다. 휴지통으로 옮기세요"** 라는 최악의 메시지가 뜬다.

**Gatekeeper 정책 통과에는 도움이 안 된다**(공증이 없으면 `spctl`은 여전히 reject).
ad-hoc 서명이 하는 일은 ① arm64에서 실행 자체를 가능하게 하고 ② "damaged" 대신
**Open Anyway 경로가 열려 있는** 다이얼로그를 띄우는 것. 이 차이가 UX상 결정적이다.

### 7.4 README에 넣을 설치 안내

Sequoia(15)부터 Control-클릭 → 열기 우회가 **제거**되었다. 사용자 절차:
`.zip` 압축 해제 → `/Applications`로 이동 → 더블클릭(차단됨) → **시스템 설정 → 개인정보 보호 및
보안 → 아래로 스크롤 → "확인 없이 열기"** → 관리자 인증.

더 빠른 길 두 가지도 함께 적는다:
```bash
xattr -dr com.apple.quarantine /Applications/DesktopCountdown.app
```
또는 애초에 `curl -L`로 받으면 quarantine이 안 붙어 **경고 없이 바로 실행된다**.

### 7.5 헤드리스 CI에서 테스트 가능한 것

`CGBitmapContext`(순수 CPU 버퍼)와 CoreText는 **WindowServer 없이 동작한다.** 지금 Windows에서
D2D/DirectWrite를 WIC 오프스크린 비트맵에 그려 테스트하는 것과 정확히 대응된다.
`render/mod.rs`의 픽셀 테스트(`alpha_is_premultiplied` 등)는 포맷이 같아서
(`GUID_WICPixelFormat32bppPBGRA` ↔ `PremultipliedFirst | ByteOrder32Little`) 거의 그대로 이식된다.

**CI에서 절대 건드리면 안 되는 것**: `NSApplication` 초기화, `NSWindow`/`NSScreen`, `CGMainDisplayID`,
`tray-icon`, eframe/winit 이벤트 루프. **렌더러를 AppKit 초기화와 분리해서 테스트 경계를
"CGContext(비트맵) + CoreText → 픽셀 버퍼"까지만 둔다.**

---

## 8. 실기 검증이 필요한 항목 (구현 중 확인)

1. **Stage Manager** 켠 상태에서 데스크톱 레벨 창의 가시성. (권위 있는 자료 없음)
2. **Sonoma+ "배경화면 클릭 시 데스크톱 표시"**(`EnableStandardClickToShowDesktop`). 우리 창은 클릭
   통과라서 클릭이 Finder에 도달 → 사용자 창들이 쓸려나간다. **앱 코드로 못 막는다.** 우리 창 자체가
   함께 쓸려나가는지는 미확인 → 실기 확인 후, 필요하면 README에 "Only in Stage Manager로 바꾸세요" 안내.
3. **메뉴바 상단 1~2px 틈** 발생 여부 (§5.1) → 발생하면 `constrainFrameRect` 오버라이드.
4. **DWrite weight 100–900 → CT weight −1.0~1.0 매핑 앵커값** (비공식 수치).
5. `objc2-*` 크레이트의 **feature flag 이름**과 CF 타입 캐스팅 방식 (docs.rs 확인).
6. `CGColorSpace`를 **DeviceRGB로 할지 sRGB로 할지** — 색 일치 문제. Windows 결과와 눈으로 비교.
7. `CTRunGetGlyphsPtr`/`CTRunGetPositionsPtr`는 **NULL을 반환할 수 있다** → 복사 버전
   (`CTRunGetGlyphs(range, buf)`) fallback을 반드시 둘 것.
8. GH arm64 러너의 **Rosetta 2 사전 설치 여부** (x86_64 슬라이스 테스트를 하려면. 안 하는 게 낫다).

---

## 9. 구현 순서

가장 불확실한 것을 앞으로 당긴다. 1단계가 틀리면 나머지 설계가 무의미해진다.

1. **스파이크 — 배경화면 창.** `NSWindow` 하나에 빨간 사각형만. 확인: 아이콘 **아래**에 깔리는가 /
   클릭이 통과하는가 / Space를 전환해도 살아있는가 / 상단에 틈이 없는가 / Mission Control·Dock·Cmd-Tab에
   안 나오는가. **여기서 §8의 1·2·3이 판가름난다.**
2. **구조 리팩터.** `src/platform/{windows,macos}` 도입, 기존 Windows 코드를 그대로 이동,
   `AppCore` 추출, `MonitorInfo`/`Panels` 계약 확정. **Windows 빌드·테스트가 그대로 통과하는지 확인.**
   (macOS 코드 없이 이 단계만으로 커밋 가능 — 회귀 검증 지점)
3. **macOS 렌더러.** `CTLine` + `CGPath` + `CGBitmapContext`. 오프스크린 픽셀 테스트를 Windows 쪽에서
   그대로 이식. **아직 화면에 안 띄운다.**
4. **렌더러 → 창 연결.** `CALayer.contents`, `contentsScale`. 1초 틱. 여기서 처음으로 카운트다운이 보인다.
5. **모니터.** `NSScreen` 열거 + UUID 식별자 + `DidChangeScreenParameters` 코얼레싱.
6. **시스템 통합.** paths / single_instance / watch / tray / autostart / fonts.
7. **설정 창.** activation policy 승격, 폰트 피커 파일 경로.
8. **패키징.** `macos/Info.plist`, .icns 생성, `release.yml` macOS 잡, README 설치 안내.

---

## 10. 참고 자료

- [Übersicht `UBWindow.m`](https://github.com/felixhageloh/uebersicht/blob/master/Uebersicht/UBWindow.m) · [`UBScreensController.m`](https://github.com/felixhageloh/uebersicht/blob/master/Uebersicht/UBScreensController.m) — 데스크톱 레벨 창의 레퍼런스 구현
- [Loopaper `DesktopWindow.swift`](https://github.com/jeongsk/Loopaper/blob/main/Loopaper/Desktop/DesktopWindow.swift) · [`ScreenIdentity.swift`](https://github.com/jeongsk/Loopaper/blob/main/Loopaper/Screens/ScreenIdentity.swift) — 디스플레이 UUID 식별
- [tauri-plugin-desktop-underlay](https://github.com/Charlie-XIAO/tauri-plugin-desktop-underlay) — Rust로 된 데스크톱 레벨 창(macOS 구현부 40줄)
- [Jim Fisher — What is the order of NSWindow levels?](https://jameshfisher.com/2020/08/03/what-is-the-order-of-nswindow-levels/)
- [objc2-core-text 0.3.2](https://docs.rs/objc2-core-text/0.3.2/objc2_core_text/) · [objc2-core-graphics `CGContext`](https://docs.rs/objc2-core-graphics/0.3.2/objc2_core_graphics/struct.CGContext.html) · [objc2-app-kit `NSWindow`](https://docs.rs/objc2-app-kit/latest/objc2_app_kit/struct.NSWindow.html)
- [WWDC 2012 Session 226 — Core Text and Fonts](https://asciiwwdc.com/2012/sessions/226) — glyph path bounds의 의미
- [Apple — Writing ARM64 code for Apple platforms](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms) — arm64 서명 필수
- [Apple Developer Forums — Manually lipoing and codesigning (Quinn)](https://developer.apple.com/forums/thread/708552)
- [Eclectic Light — Gatekeeper and notarization in Sequoia](https://eclecticlight.co/2024/08/10/gatekeeper-and-notarization-in-sequoia/)
- [scriptingosx — LaunchD](https://scriptingosx.com/2024/07/building-a-launchd-installer-pkg-for-desktoppr-and-other-tools/) · [launchd.info](https://www.launchd.info/)
