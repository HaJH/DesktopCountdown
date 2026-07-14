# 스파이크 결과: macOS 데스크톱 레벨 창

- 실행일: 2026-07-14
- 환경: MacBook Pro (Apple M1 Max), **macOS 26.5.1 (25F80)**, 내장 레티나 1대 (1800×1169 @2x)
- 대상 커밋: `214eff3` (docs: macOS port design)
- 관련: 설계 문서 [§5.1](../specs/2026-07-14-macos-port-design.md) (창 설정), §8 (미확인 항목), §9 1단계

## 결론

**설계 §5.1의 창 설정이 그대로 동작한다. 설계 변경 없음.** 2단계(구조 리팩터)로 진행한다.

§8의 미확인 항목 중 **1(Stage Manager)·3(메뉴바 틈)·5(objc2 feature flag)가 해소**됐다.
2(클릭 시 데스크톱 표시)·4(폰트 weight 매핑)·6·7·8은 이 스파이크의 범위가 아니라 그대로 남는다.

> **주의:** 설계 문서가 인용한 근거(Übersicht #541의 메뉴바 틈, Sequoia Gatekeeper 변경)는 전부
> **macOS 15 Sequoia** 기준이다. 이 검증은 **macOS 26**에서 했다. 아래 결과는 26 기준이며,
> 구형 macOS(배포 타깃 11.0)에서 같다는 보장은 없다. 특히 메뉴바 틈은 OS 버전에 따라 재발할 수 있다.

## 검증 결과

| # | 항목 | 결과 |
|---|---|---|
| a | 데스크톱 아이콘 **아래**에 깔리는가 | ✅ 아이콘이 위에 그려진다 |
| b | 클릭이 통과하는가 | ✅ 아이콘 클릭·드래그, 메뉴바 전부 정상 |
| c | Space를 전환해도 살아있는가 | ✅ 새 Space에서도 동일하게 표시 |
| d | 화면 상단에 1~2px 틈이 생기는가 | ✅ **틈 없음** (아래 참조) |
| e | Mission Control / Dock / Cmd-Tab에 안 나오는가 | ✅ 안 나온다 |
| f | Stage Manager | ✅ 켠 상태에서도 그대로 보인다. 창 막대에도 나타나지 않는다 |

알파 합성도 확인됐다. `CALayer.backgroundColor`에 알파 0.30을 준 레이어 아래로 배경화면이 비치고,
불투명 서브레이어(가장자리 띠·중앙 사각형)는 불투명하게 나온다. 픽셀별 알파가 배경 레이어에서
정상 합성된다는 뜻이다 — Windows 스파이크에서 `UpdateLayeredWindow`가 자식 창에서 조용히 실패해
DirectComposition으로 선회해야 했던 것과 달리, macOS는 **첫 시도에서 바로 된다.**

## 발견 1 — 창 레벨 상수가 설계값과 정확히 일치

`objc2-core-graphics` 0.3.2가 컴파일 타임 상수로 그대로 노출한다 (런타임 `CGWindowLevelForKey` 불필요):

```
kCGDesktopWindowLevel      = -2147483623
kCGDesktopIconWindowLevel  = -2147483603
level we use (icon - 1)    = -2147483604
```

`setLevel` 후 `window.level()`로 되읽어도 `-2147483604`가 유지된다. AppKit이 값을 clamp하지 않는다.

## 발견 2 — 메뉴바 틈(§8-3)은 macOS 26에서 발생하지 않는다

요청한 프레임과 AppKit이 실제로 준 프레임이 **완전히 같다.**

```
requested frame:  x=0.0 y=0.0 w=1800.0 h=1169.0
actual   frame:   x=0.0 y=0.0 w=1800.0 h=1169.0
```

`constrainFrameRect(_:to:)`가 borderless + 데스크톱 레벨 창에는 관여하지 않는다.
→ **`objc2::define_class!`로 `NSWindow`를 서브클래싱할 필요가 없다.** 설계 §5.1이 "`define_class!`가
필요한 유일한 지점"이라고 했던 곳이 사라졌으므로, **macOS 백엔드에 `define_class!`는 아예 등장하지 않는다.**

육안으로도 화면 최상단 노란 띠 위로 배경화면이 비치지 않았다. 프레임 값과 육안이 일치한다.

단, 이건 macOS 26 결과다. 구형 OS에서 재발하면 그때 오버라이드를 넣는다 — 설계 문서에 방법이 적혀 있다.

## 발견 3 — 크레이트 feature flag 조합 (§8-5)

아래 조합으로 한 번에 컴파일된다. 설계 §6의 크레이트 목록이 맞다.

```toml
objc2 = "0.6"
objc2-foundation = { version = "0.3", features = ["NSGeometry", "NSString", "NSNotification", "NSThread"] }
objc2-app-kit = { version = "0.3", features = [
    "NSApplication", "NSWindow", "NSScreen", "NSView", "NSResponder",
    "NSColor", "NSGraphics", "NSRunningApplication",
] }
objc2-core-foundation = { version = "0.3", features = ["CFBase", "CFUUID"] }
objc2-core-graphics = { version = "0.3", features = ["CGColor", "CGColorSpace", "CGDirectDisplay", "CGWindowLevel"] }
objc2-quartz-core = { version = "0.3", features = ["CALayer", "objc2-core-foundation"] }
```

주의할 점 몇 가지:

- **`NSWindowStyleMask::Borderless`는 `NSWindow` feature, `NSBackingStoreType::Buffered`는 `NSGraphics`
  feature에 있다.** `initWithContentRect:styleMask:backing:defer:` 자체가 `#[cfg(feature = "NSGraphics")]`로
  가려져 있어서 `NSGraphics` 없이는 생성자가 아예 안 보인다.
- `setRestorable` / `disableSnapshotRestoration`은 `NSWindow.rs`가 아니라 **`NSWindowRestoration.rs`**에 있다.
  현재 feature 목록으로도 잡히지만, feature를 줄일 때 주의.
- `NSApplicationActivationPolicy`는 `NSApplication`이 아니라 **`NSRunningApplication` feature**에 정의돼 있다.
- `NSWindow::alloc(mtm)`을 쓰려면 `use objc2::MainThreadOnly;`가 스코프에 있어야 한다 (트레이트 메서드).
- **`setReleasedWhenClosed`만 `unsafe`다.** 나머지 setter(`setLevel`·`setCollectionBehavior`·
  `setIgnoresMouseEvents` 등)는 전부 safe `fn`이다. 우리가 넘기는 `false`는 창을 살려두는 방향이라
  안전하지만, `unsafe` 블록과 SAFETY 주석이 필요하다.
- `CGColor::new_srgb(r, g, b, a) -> CFRetained<CGColor>`. Deref 강제 변환으로 `Some(&color)`가
  `Option<&CGColor>`에 그대로 들어간다. **servo `core-graphics` 크레이트는 섞지 않았고, 섞을 이유도 없었다.**

## 발견 4 — 현재 리포는 macOS에서 `cargo check`조차 안 된다

예상된 실패지만, **깨지는 지점이 우리 코드가 아니다.** `Cargo.toml`의 `windows` 계열이 target-gate 없이
`[dependencies]`에 있어서, 의존성인 `windows-future` 0.3.2가 컴파일에 실패한다:

```
error[E0425]: cannot find function `submit` in crate `windows_threading`
  --> windows-future-0.3.2/src/async_spawn.rs:264:28
error: could not compile `windows-future` (lib) due to 16 previous errors
```

따라서 **스파이크를 리포 안의 `examples/`나 `src/bin/`에 둘 수 없다** — 그것들도 라이브러리 크레이트를
같이 빌드하기 때문이다. 이번 스파이크는 리포 밖 독립 크레이트로 만들었다.

핸드오프 문서가 "`examples/` 또는 `src/bin/`으로 시작하는 게 빠르다"고 적은 부분은 **틀렸다.**
설계 §6의 `[target.'cfg(windows)'.dependencies]` 이동이 2단계에서 **가장 먼저** 이뤄져야 한다.

## 발견 5 — 로컬 환경 제약: rustup이 없다

이 머신의 Rust는 **Homebrew 설치본**(rustc 1.92.0)이라 `rustup`이 없다. 즉 `rustup target add
x86_64-apple-darwin`이 불가능하고, **로컬에서 universal 바이너리를 만들 수 없다.**

- 1~7단계(호스트 arm64 개발·테스트)에는 영향 없다.
- 8단계(패키징)의 `lipo` universal 빌드는 **CI에서만 검증 가능**하다. CI는 `dtolnay/rust-toolchain`으로
  타깃을 받으므로 설계 §7.2 그대로 동작한다.
- 로컬에서 universal을 만져야 하면 그때 rustup을 설치한다.

## 후속 조치

설계 문서 수정 사항:

- **§5.1의 `constrainFrameRect` 오버라이드 문단** — macOS 26에서 불필요함이 확인됐다. "우선 오버라이드
  없이 띄워보고 틈이 확인되면 넣는다"는 지침대로, **넣지 않는다.** `define_class!` 사용처가 0이 된다.
- **§8 항목 1·3·5 해소.** 항목 2·4·6·7·8은 유지.
- **핸드오프 문서의 "스파이크는 examples/로"** 안내는 발견 4에 따라 무효.

2단계(구조 리팩터)의 첫 작업은 `Cargo.toml`의 windows 의존성 target-gating이다. 그래야 macOS에서
`cargo check`가 우리 코드에 도달한다.

## 스파이크 코드

리포에 커밋하지 않았다(버리는 코드). 재현이 필요하면 이 문서의 발견 3에 있는 `Cargo.toml`과
설계 §5.1의 창 설정 표로 30분이면 다시 만든다.
