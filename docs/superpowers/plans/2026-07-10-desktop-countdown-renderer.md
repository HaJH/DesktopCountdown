# DesktopCountdown 렌더러 구현 계획 (계획 1/2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 바탕화면 배경 레이어(WorkerW)에 마감까지 남은 시간을 표시하고, 트레이 아이콘으로 제어하며, `config.toml` 저장 즉시 화면에 반영되는 Windows 네이티브 앱을 만든다.

**Architecture:** Direct2D의 WIC 비트맵 렌더 타깃에 DirectWrite로 글자를 그려 프리멀티플라이드 BGRA 버퍼를 얻고, 그걸 WorkerW의 자식 레이어드 창에 `UpdateLayeredWindow`로 올린다. Win32를 아는 모듈과 모르는 모듈을 갈라 후자를 전부 단위 테스트한다. 숨은 최상위 컨트롤러 창이 타이머와 시스템 메시지를 받는다.

**Tech Stack:** Rust 2021 / `windows` 0.62 (Win32, Direct2D, DirectWrite, WIC) / `jiff` 0.2 / `serde` + `toml` / `notify` 8.2 / `tray-icon` 0.24 / `tracing`

**설계 문서:** `docs/superpowers/specs/2026-07-10-desktop-countdown-design.md`

**범위:** 이 계획은 렌더러 프로세스만 만든다. egui 설정 창은 계획 2에서 다룬다. 계획 1이 끝나면 사용자는 `config.toml`을 메모장으로 편집해 앱을 완전히 사용할 수 있다.

## Global Constraints

- 대상 OS는 Windows 10 1809 이상. 다른 OS 지원은 시도하지 않는다.
- Rust edition 2021, rustc 1.92 이상.
- 크레이트 버전: `windows = "0.62"`, `jiff = "0.2"`, `serde = "1"`, `toml = "1"`, `notify = "8"`, `tray-icon = "0.24"`, `tracing = "0.1"`, `tracing-appender = "0.2"`, `thiserror = "2"`, `anyhow = "1"`.
- **`windows` 0.62의 정확한 함수 시그니처(특히 `Option<HWND>` 래핑, `Result` 반환 여부)는 버전마다 다르다. 이 문서의 Win32 코드는 로직 기준이며, 컴파일 에러가 나면 컴파일러가 요구하는 형태를 따른다.** 로직을 바꾸지 않는 한 시그니처 조정은 계획 이탈이 아니다.
- 텍스트 안티에일리어싱은 항상 `D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`. ClearType은 레이어드 창에서 알파를 망가뜨린다.
- 코드와 코드 주석은 영어. 커밋 메시지와 문서는 한국어.
- 커밋 메시지에 자동 생성 문구(`Co-Authored-By`, `Generated with` 등)를 넣지 않는다. 제목 한 줄 + 필요하면 불릿 몇 개로 충분하다.
- 각 태스크는 커밋으로 끝난다.

---

### Task 1: 프로젝트 스캐폴드 + 레이어드 자식 창 스파이크

설계 §8. **이 태스크가 실패하면 이후 모든 태스크의 렌더링 경로가 바뀐다.** 그래서 첫 번째다.

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Create: `src/bin/spike_layered.rs`

**Interfaces:**
- Consumes: 없음
- Produces: 없음 (스파이크는 검증 전용. 이후 태스크가 이 코드를 import하지 않는다.)

- [ ] **Step 1: `Cargo.toml` 작성**

```toml
[package]
name = "desktop-countdown"
version = "0.1.0"
edition = "2021"
rust-version = "1.92"

[lib]
name = "desktop_countdown"
path = "src/lib.rs"

[[bin]]
name = "desktop-countdown"
path = "src/main.rs"

[dependencies]
anyhow = "1"
thiserror = "2"
jiff = { version = "0.2", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
toml = "1"
notify = "8"
tray-icon = "0.24"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"

[dependencies.windows]
version = "0.62"
features = [
    "implement",
    "Win32_Foundation",
    "Win32_Graphics_Direct2D",
    "Win32_Graphics_Direct2D_Common",
    "Win32_Graphics_DirectWrite",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_Graphics_Imaging",
    "Win32_System_Com",
    "Win32_System_LibraryLoader",
    "Win32_System_Registry",
    "Win32_System_Threading",
    "Win32_UI_HiDpi",
    "Win32_UI_WindowsAndMessaging",
]
```

- [ ] **Step 2: 빈 `src/lib.rs`와 `src/main.rs` 작성**

`src/lib.rs`:

```rust
//! DesktopCountdown — draws a countdown onto the desktop wallpaper layer.
```

`src/main.rs`:

```rust
fn main() {
    println!("placeholder");
}
```

- [ ] **Step 3: 빌드가 되는지 확인**

Run: `cargo build`
Expected: 성공. 의존성이 전부 받아지고 컴파일된다. (첫 빌드는 몇 분 걸린다.)

- [ ] **Step 4: 스파이크 바이너리 작성**

`src/bin/spike_layered.rs`. WorkerW를 확보하고, 그 자식으로 레이어드 창을 만들어 좌→우로 알파가 0에서 255로 변하는 빨간 그라디언트 사각형(600×300)을 주 모니터 좌상단에서 (100, 100) 떨어진 곳에 올린다.

```rust
//! Spike: verify that a WS_EX_LAYERED child of WorkerW composites per-pixel alpha
//! over the desktop wallpaper. See spec section 8 for the success criteria.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{copy_nonoverlapping, null_mut};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowExW, FindWindowW,
    GetMessageW, RegisterClassW, SendMessageTimeoutW, SetWindowPos, TranslateMessage,
    UpdateLayeredWindow, AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, HWND_TOP, MSG, SMTO_NORMAL,
    SWP_NOACTIVATE, SWP_NOZORDER, ULW_ALPHA, WNDCLASSW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TRANSPARENT, WS_VISIBLE,
};

const W: i32 = 600;
const H: i32 = 300;

fn main() -> Result<()> {
    unsafe {
        let workerw = acquire_workerw()?;
        println!("WorkerW = {:?}", workerw);

        let hinst = GetModuleHandleW(None)?;
        let class = w!("SpikeLayeredChild");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            W,
            H,
            Some(workerw),
            None,
            Some(hinst.into()),
            None,
        )?;

        // Child coordinates are relative to the parent's client area.
        let mut origin = POINT { x: 100, y: 100 };
        windows::Win32::Graphics::Gdi::ScreenToClient(workerw, &mut origin).ok()?;
        SetWindowPos(hwnd, Some(HWND_TOP), origin.x, origin.y, W, H, SWP_NOACTIVATE | SWP_NOZORDER)?;

        push_gradient(hwnd)?;

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

/// Premultiplied BGRA: alpha ramps 0..255 left to right, colour is red.
fn gradient_pixels() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let a = (x * 255 / (W - 1)) as u8;
            let i = ((y * W + x) * 4) as usize;
            px[i] = 0; // B
            px[i + 1] = 0; // G
            px[i + 2] = a; // R, premultiplied by alpha
            px[i + 3] = a; // A
        }
    }
    px
}

unsafe fn push_gradient(hwnd: HWND) -> Result<()> {
    let pixels = gradient_pixels();
    let hdc_screen = GetDC(None);
    let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: W,
            biHeight: -H, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = null_mut();
    let hbmp = CreateDIBSection(Some(hdc_mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
    copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
    let old = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));

    let size = SIZE { cx: W, cy: H };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    UpdateLayeredWindow(
        hwnd,
        Some(hdc_screen),
        None, // position already set by SetWindowPos
        Some(&size),
        Some(hdc_mem),
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    )?;

    SelectObject(hdc_mem, old);
    let _ = DeleteObject(HGDIOBJ(hbmp.0));
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(None, hdc_screen);
    Ok(())
}

unsafe fn acquire_workerw() -> Result<HWND> {
    let progman = FindWindowW(w!("Progman"), None)?;
    let mut res = 0usize;

    SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
    if let Some(h) = find_workerw() {
        return Ok(h);
    }
    // Some Windows builds only spawn the WorkerW for this payload.
    SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut res));
    find_workerw().ok_or_else(|| windows::core::Error::from_win32())
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // The WorkerW we want is the sibling that follows the window owning SHELLDLL_DefView.
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
            *(lparam.0 as *mut HWND) = worker;
            return BOOL(0); // stop enumeration
        }
    }
    BOOL(1)
}

unsafe fn find_workerw() -> Option<HWND> {
    let mut out = HWND(null_mut());
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut out as *mut HWND as isize));
    if out.0.is_null() {
        None
    } else {
        Some(out)
    }
}
```

- [ ] **Step 5: 스파이크 실행 후 4가지 성공 기준을 눈으로 확인**

Run: `cargo run --bin spike_layered`

바탕화면을 보이게 한 뒤(`Win+D`) 다음을 확인한다. **네 항목 전부 통과해야 한다.**

1. 사각형의 왼쪽(알파 0)은 완전히 투명해 벽지가 그대로 보이고, 오른쪽으로 갈수록 빨갛게 진해진다.
2. 사각형 위치에 바탕화면 아이콘이 있으면 아이콘이 사각형 **위에** 그려진다. (없으면 아이콘을 그 자리로 잠깐 옮겨 확인한다.)
3. 아무 창이나 띄우면 사각형이 창에 가려진다.
4. `SetWindowPos`의 `origin`을 두 번째 모니터 좌표(예: `x: 2600, y: -400`)로 바꿔 다시 실행하면 그 모니터에 같은 사각형이 그려진다.

종료는 `Ctrl+C`.

- [ ] **Step 6: 결과 기록**

`docs/superpowers/plans/spike-result.md`에 네 항목 각각의 통과/실패와 스크린샷 경로를 적는다.

**넷 다 통과 → Task 2로 진행한다.**

**하나라도 실패 → 여기서 멈추고 사용자에게 보고한다.** 설계 §9(벽지 직접 합성)로 전환해야 하며, Task 8·10·11이 다시 쓰여야 한다. 임의로 진행하지 않는다.

- [ ] **Step 7: 커밋**

```bash
git add Cargo.toml Cargo.lock src/ docs/superpowers/plans/spike-result.md
git commit -m "스캐폴드 + WorkerW 레이어드 자식 창 스파이크

- Cargo 프로젝트 초기화, windows/jiff/serde 등 의존성 추가
- WorkerW 확보 후 자식 레이어드 창에 알파 그라디언트 표시
- 스파이크 검증 결과 기록"
```

---

### Task 2: countdown 모듈

설계 §2, §2.1. 순수 모듈. Win32 없음.

**Files:**
- Create: `src/countdown.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: 없음
- Produces:
  - `pub struct Breakdown { pub months: i64, pub weeks: i64, pub days: i64, pub total_hours: i64, pub minutes: i64, pub seconds: i64, pub expired: bool }`
  - `pub fn breakdown(now: &jiff::Zoned, target: &jiff::Zoned) -> Breakdown`
  - `pub fn format_main(b: &Breakdown) -> String`
  - `pub fn format_summary(b: &Breakdown) -> String`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/countdown.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use jiff::{civil::datetime, tz::{offset, TimeZone}, Zoned};

    fn z(y: i16, m: i8, d: i8, h: i8, mi: i8, s: i8) -> Zoned {
        datetime(y, m, d, h, mi, s, 0)
            .to_zoned(TimeZone::fixed(offset(9)))
            .unwrap()
    }

    #[test]
    fn one_second_before_target() {
        let b = breakdown(&z(2026, 10, 24, 8, 59, 59), &z(2026, 10, 24, 9, 0, 0));
        assert!(!b.expired);
        assert_eq!(format_main(&b), "00:00:01");
        assert_eq!(format_summary(&b), "0m 0w 0d");
    }

    #[test]
    fn exactly_at_target_is_expired() {
        let t = z(2026, 10, 24, 9, 0, 0);
        let b = breakdown(&t, &t);
        assert!(b.expired);
        assert_eq!(format_main(&b), "00:00:00");
        assert_eq!(format_summary(&b), "0m 0w 0d");
    }

    #[test]
    fn past_target_stays_at_zero() {
        let b = breakdown(&z(2026, 10, 25, 0, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert!(b.expired);
        assert_eq!(format_main(&b), "00:00:00");
    }

    #[test]
    fn hour_digits_grow_past_two() {
        // 4 days 4 hours = 100 hours exactly.
        let b = breakdown(&z(2026, 10, 20, 5, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_main(&b), "100:00:00");
    }

    #[test]
    fn hour_digits_shrink_to_two() {
        let b = breakdown(&z(2026, 10, 20, 5, 0, 1), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_main(&b), "99:59:59");
    }

    #[test]
    fn summary_splits_months_weeks_days() {
        // 2026-07-10 09:00 -> 2026-10-24 09:00 is 106 days = 3 months + 14 days.
        let b = breakdown(&z(2026, 7, 10, 9, 0, 0), &z(2026, 10, 24, 9, 0, 0));
        assert_eq!(format_summary(&b), "3m 2w 0d");
        assert_eq!(format_main(&b), "2544:00:00");
    }

    #[test]
    fn month_end_clamps_to_shorter_month() {
        // Jan 31 + 1 month clamps to Feb 28, leaving 1 day to Mar 1.
        let b = breakdown(&z(2026, 1, 31, 0, 0, 0), &z(2026, 3, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "1m 0w 1d");
    }

    #[test]
    fn leap_day_is_handled() {
        // 2028 is a leap year: Jan 31 + 1 month clamps to Feb 29, leaving 1 day.
        let b = breakdown(&z(2028, 1, 31, 0, 0, 0), &z(2028, 3, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "1m 0w 1d");
    }

    #[test]
    fn months_have_no_upper_bound() {
        let b = breakdown(&z(2026, 1, 1, 0, 0, 0), &z(2027, 7, 1, 0, 0, 0));
        assert_eq!(format_summary(&b), "18m 0w 0d");
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod countdown;` 한 줄을 추가한 뒤,

Run: `cargo test countdown`
Expected: 컴파일 실패. `cannot find function 'breakdown'`.

- [ ] **Step 3: 최소 구현 작성**

`src/countdown.rs` 상단에:

```rust
//! Pure countdown arithmetic. No Win32, no I/O.

use jiff::{Unit, Zoned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakdown {
    /// Whole calendar months remaining.
    pub months: i64,
    /// Whole weeks in the remainder after `months`.
    pub weeks: i64,
    /// Whole days in the remainder after `weeks`.
    pub days: i64,
    /// Total hours remaining. Unbounded; not reduced by `months`/`weeks`/`days`.
    pub total_hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub expired: bool,
}

const EXPIRED: Breakdown = Breakdown {
    months: 0,
    weeks: 0,
    days: 0,
    total_hours: 0,
    minutes: 0,
    seconds: 0,
    expired: true,
};

pub fn breakdown(now: &Zoned, target: &Zoned) -> Breakdown {
    let secs = target.timestamp().as_second() - now.timestamp().as_second();
    if secs <= 0 {
        return EXPIRED;
    }

    // Calendar units for the summary line. `until` clamps month-end overflow
    // (Jan 31 + 1 month => Feb 28/29), which is what the spec asks for.
    let span = now
        .until((Unit::Month, target))
        .expect("calendar difference between two zoned datetimes");
    let rem_days = span.get_days();

    Breakdown {
        months: span.get_months(),
        weeks: rem_days / 7,
        days: rem_days % 7,
        total_hours: secs / 3600,
        minutes: (secs / 60) % 60,
        seconds: secs % 60,
        expired: false,
    }
}

/// `"2544:18:07"` — hours are zero-padded to at least two digits and grow freely.
pub fn format_main(b: &Breakdown) -> String {
    format!("{:02}:{:02}:{:02}", b.total_hours, b.minutes, b.seconds)
}

/// `"3m 2w 0d"` — auxiliary summary. Months are unbounded; years are never used.
pub fn format_summary(b: &Breakdown) -> String {
    format!("{}m {}w {}d", b.months, b.weeks, b.days)
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test countdown`
Expected: 9개 테스트 전부 PASS.

`month_end_clamps_to_shorter_month`나 `leap_day_is_handled`가 실패하면 `jiff`의 오버플로 정책이 예상과 다른 것이다. 실제 반환값을 출력해 확인하고, jiff가 옳으면 **테스트의 기댓값이 아니라 스펙 §2.1의 서술을 고친다.**

- [ ] **Step 5: 커밋**

```bash
git add src/countdown.rs src/lib.rs
git commit -m "countdown 모듈: 남은 총 시간과 개월/주/일 분해

- 만료 시 전부 0으로 클램프
- 시 자리는 최소 2자리, 상한 없음
- 월말 클램핑과 윤년 경계 테스트 포함"
```

---

### Task 3: color 파서

설계 §4. 순수 모듈.

**Files:**
- Create: `src/color.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: 없음
- Produces:
  - `pub struct Rgb { pub r: u8, pub g: u8, pub b: u8 }`
  - `pub fn parse_hex(s: &str) -> Option<Rgb>`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/color.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_hash() {
        assert_eq!(parse_hex("#FFFFFF"), Some(Rgb { r: 255, g: 255, b: 255 }));
    }

    #[test]
    fn parses_without_hash() {
        assert_eq!(parse_hex("1A2B3C"), Some(Rgb { r: 0x1A, g: 0x2B, b: 0x3C }));
    }

    #[test]
    fn is_case_insensitive() {
        assert_eq!(parse_hex("#abcdef"), parse_hex("#ABCDEF"));
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(parse_hex("#FFF"), None);
        assert_eq!(parse_hex("#FFFFFFFF"), None);
    }

    #[test]
    fn rejects_non_hex() {
        assert_eq!(parse_hex("#GGGGGG"), None);
        assert_eq!(parse_hex(""), None);
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod color;` 추가 후,

Run: `cargo test color`
Expected: 컴파일 실패. `cannot find type 'Rgb'`.

- [ ] **Step 3: 최소 구현 작성**

```rust
//! Hex colour parsing. No Win32, no I/O.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Accepts `#RRGGBB` and `RRGGBB`, case-insensitive.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let t = s.strip_prefix('#').unwrap_or(s);
    if t.len() != 6 || !t.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Rgb {
        r: u8::from_str_radix(&t[0..2], 16).ok()?,
        g: u8::from_str_radix(&t[2..4], 16).ok()?,
        b: u8::from_str_radix(&t[4..6], 16).ok()?,
    })
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test color`
Expected: 5개 PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/color.rs src/lib.rs
git commit -m "color 모듈: 16진 색상 파서"
```

---

### Task 4: config 스키마와 기본값, 검증

설계 §4. 순수 모듈.

**Files:**
- Create: `src/config/mod.rs`
- Create: `src/config/schema.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::color::parse_hex`
- Produces:
  - `pub enum DrawMode { Fill, Outline, Both }`
  - `pub enum Anchor { TopLeft, TopCenter, TopRight, MiddleLeft, Center, MiddleRight, BottomLeft, BottomCenter, BottomRight }`
  - `pub struct Style { font_family: String, font_weight: u16, size_px: f32, mode: DrawMode, color: String, outline_color: String, outline_width_px: f32, opacity: f32, letter_spacing_em: f32, shadow: bool, tabular_figures: bool, show_summary_line: bool }` (모든 필드 `pub`)
  - `pub struct Layout { pub anchor: Anchor, pub offset_px: [i32; 2] }`
  - `pub struct General { pub autostart: bool }`
  - `pub struct DisplayOverride { pub id: String, pub name: Option<String>, pub enabled: Option<bool>, pub anchor: Option<Anchor>, pub offset_px: Option<[i32;2]>, ... 각 Style 필드의 Option 판 }`
  - `pub struct Config { pub target: jiff::civil::DateTime, pub style: Style, pub layout: Layout, pub general: General, pub displays: Vec<DisplayOverride> }`
  - `pub enum ConfigError` (thiserror)
  - `pub fn validate(cfg: &Config) -> Result<(), ConfigError>`
  - `impl Default for Config`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/config/schema.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"target = "2026-10-24T09:00:00""#;

    #[test]
    fn minimal_config_fills_defaults() {
        let cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert_eq!(cfg.style.font_family, "Consolas");
        assert_eq!(cfg.style.size_px, 64.0);
        assert_eq!(cfg.style.mode, DrawMode::Fill);
        assert_eq!(cfg.style.opacity, 0.85);
        assert!(cfg.style.shadow);
        assert!(cfg.style.tabular_figures);
        assert!(cfg.style.show_summary_line);
        assert_eq!(cfg.layout.anchor, Anchor::Center);
        assert_eq!(cfg.layout.offset_px, [0, 0]);
        assert!(!cfg.general.autostart);
        assert!(cfg.displays.is_empty());
    }

    #[test]
    fn anchor_uses_kebab_case() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"
[layout]
anchor = "bottom-right"
offset_px = [-40, -80]
"#,
        )
        .unwrap();
        assert_eq!(cfg.layout.anchor, Anchor::BottomRight);
        assert_eq!(cfg.layout.offset_px, [-40, -80]);
    }

    #[test]
    fn display_overrides_are_parsed_flat() {
        let cfg: Config = toml::from_str(
            r#"
target = "2026-10-24T09:00:00"

[[display]]
id = "\\\\?\\DISPLAY#DEL41A8#1"
name = "DISPLAY1 (세로)"
enabled = true
anchor = "top-center"
size_px = 48.0
"#,
        )
        .unwrap();
        assert_eq!(cfg.displays.len(), 1);
        let d = &cfg.displays[0];
        assert_eq!(d.enabled, Some(true));
        assert_eq!(d.anchor, Some(Anchor::TopCenter));
        assert_eq!(d.size_px, Some(48.0));
        assert_eq!(d.font_family, None);
    }

    #[test]
    fn draw_mode_is_lowercase() {
        let cfg: Config =
            toml::from_str("target = \"2026-10-24T09:00:00\"\n[style]\nmode = \"outline\"").unwrap();
        assert_eq!(cfg.style.mode, DrawMode::Outline);
    }

    #[test]
    fn validate_accepts_defaults() {
        let cfg: Config = toml::from_str(MINIMAL).unwrap();
        assert!(validate(&cfg).is_ok());
    }

    #[test]
    fn validate_rejects_bad_colour() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.color = "not-a-colour".into();
        assert!(matches!(validate(&cfg), Err(ConfigError::Color(_))));
    }

    #[test]
    fn validate_rejects_out_of_range_opacity() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.opacity = 1.5;
        assert!(matches!(validate(&cfg), Err(ConfigError::Opacity(_))));
    }

    #[test]
    fn validate_rejects_nonpositive_size() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.size_px = 0.0;
        assert!(matches!(validate(&cfg), Err(ConfigError::Size(_))));
    }

    #[test]
    fn validate_rejects_bad_weight() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.style.font_weight = 50;
        assert!(matches!(validate(&cfg), Err(ConfigError::Weight(_))));
    }

    #[test]
    fn validate_checks_display_overrides_too() {
        let mut cfg: Config = toml::from_str(MINIMAL).unwrap();
        cfg.displays.push(DisplayOverride {
            id: "x".into(),
            opacity: Some(9.0),
            ..DisplayOverride::default()
        });
        assert!(matches!(validate(&cfg), Err(ConfigError::Opacity(_))));
    }

    #[test]
    fn default_config_round_trips_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod config;`, `src/config/mod.rs`에 `mod schema; pub use schema::*;` 추가 후,

Run: `cargo test config`
Expected: 컴파일 실패. `cannot find type 'Config'`.

- [ ] **Step 3: 최소 구현 작성**

`src/config/schema.rs`:

```rust
//! Config schema, defaults, and validation. No Win32, no I/O.

use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};

use crate::color::parse_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawMode {
    Fill,
    Outline,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    Center,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

fn d_font_family() -> String { "Consolas".to_string() }
fn d_font_weight() -> u16 { 400 }
fn d_size_px() -> f32 { 64.0 }
fn d_mode() -> DrawMode { DrawMode::Fill }
fn d_color() -> String { "#FFFFFF".to_string() }
fn d_outline_color() -> String { "#000000".to_string() }
fn d_outline_width() -> f32 { 1.5 }
fn d_opacity() -> f32 { 0.85 }
fn d_letter_spacing() -> f32 { 0.02 }
fn d_true() -> bool { true }
fn d_anchor() -> Anchor { Anchor::Center }
fn d_offset() -> [i32; 2] { [0, 0] }

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    #[serde(default = "d_font_family")]
    pub font_family: String,
    #[serde(default = "d_font_weight")]
    pub font_weight: u16,
    #[serde(default = "d_size_px")]
    pub size_px: f32,
    #[serde(default = "d_mode")]
    pub mode: DrawMode,
    #[serde(default = "d_color")]
    pub color: String,
    #[serde(default = "d_outline_color")]
    pub outline_color: String,
    #[serde(default = "d_outline_width")]
    pub outline_width_px: f32,
    #[serde(default = "d_opacity")]
    pub opacity: f32,
    #[serde(default = "d_letter_spacing")]
    pub letter_spacing_em: f32,
    #[serde(default = "d_true")]
    pub shadow: bool,
    #[serde(default = "d_true")]
    pub tabular_figures: bool,
    #[serde(default = "d_true")]
    pub show_summary_line: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font_family: d_font_family(),
            font_weight: d_font_weight(),
            size_px: d_size_px(),
            mode: d_mode(),
            color: d_color(),
            outline_color: d_outline_color(),
            outline_width_px: d_outline_width(),
            opacity: d_opacity(),
            letter_spacing_em: d_letter_spacing(),
            shadow: true,
            tabular_figures: true,
            show_summary_line: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default = "d_anchor")]
    pub anchor: Anchor,
    #[serde(default = "d_offset")]
    pub offset_px: [i32; 2],
}

impl Default for Layout {
    fn default() -> Self {
        Self { anchor: d_anchor(), offset_px: d_offset() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    #[serde(default)]
    pub autostart: bool,
}

/// Per-monitor overrides. Style fields sit at the same level as `enabled`,
/// not nested under a `[style]` table — see spec section 4.2.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayOverride {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_px: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_px: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DrawMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width_px: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_em: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabular_figures: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_summary_line: Option<bool>,
}

fn d_target() -> DateTime {
    jiff::civil::datetime(2026, 12, 31, 23, 59, 59, 0)
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "d_target")]
    pub target: DateTime,
    #[serde(default)]
    pub style: Style,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub general: General,
    #[serde(default, rename = "display", skip_serializing_if = "Vec::is_empty")]
    pub displays: Vec<DisplayOverride>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target: d_target(),
            style: Style::default(),
            layout: Layout::default(),
            general: General::default(),
            displays: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("invalid colour `{0}`, expected #RRGGBB")]
    Color(String),
    #[error("opacity must be within 0.0..=1.0, got {0}")]
    Opacity(f32),
    #[error("size_px must be greater than 0, got {0}")]
    Size(f32),
    #[error("font_weight must be within 100..=900, got {0}")]
    Weight(u16),
    #[error("outline_width_px must not be negative, got {0}")]
    OutlineWidth(f32),
}

fn check_color(s: &str) -> Result<(), ConfigError> {
    parse_hex(s).map(|_| ()).ok_or_else(|| ConfigError::Color(s.to_string()))
}

fn check_opacity(v: f32) -> Result<(), ConfigError> {
    if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(ConfigError::Opacity(v)) }
}

fn check_size(v: f32) -> Result<(), ConfigError> {
    if v > 0.0 { Ok(()) } else { Err(ConfigError::Size(v)) }
}

fn check_weight(v: u16) -> Result<(), ConfigError> {
    if (100..=900).contains(&v) { Ok(()) } else { Err(ConfigError::Weight(v)) }
}

fn check_outline_width(v: f32) -> Result<(), ConfigError> {
    if v >= 0.0 { Ok(()) } else { Err(ConfigError::OutlineWidth(v)) }
}

pub fn validate(cfg: &Config) -> Result<(), ConfigError> {
    let s = &cfg.style;
    check_color(&s.color)?;
    check_color(&s.outline_color)?;
    check_opacity(s.opacity)?;
    check_size(s.size_px)?;
    check_weight(s.font_weight)?;
    check_outline_width(s.outline_width_px)?;

    for d in &cfg.displays {
        if let Some(v) = &d.color { check_color(v)?; }
        if let Some(v) = &d.outline_color { check_color(v)?; }
        if let Some(v) = d.opacity { check_opacity(v)?; }
        if let Some(v) = d.size_px { check_size(v)?; }
        if let Some(v) = d.font_weight { check_weight(v)?; }
        if let Some(v) = d.outline_width_px { check_outline_width(v)?; }
    }
    Ok(())
}
```

`src/config/mod.rs`:

```rust
mod schema;
pub use schema::*;
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test config`
Expected: 11개 PASS.

`display_overrides_are_parsed_flat`가 `deny_unknown_fields` 때문에 실패하면 필드 이름 오타다. `default_config_round_trips_through_toml`이 실패하면 `skip_serializing_if`와 `default`가 어긋난 것이다.

- [ ] **Step 5: 커밋**

```bash
git add src/config/ src/lib.rs
git commit -m "config 스키마: 기본값, 모니터별 오버라이드, 검증

- 스타일 필드는 [[display]] 안에서 평평하게 배치
- 색상/불투명도/크기/굵기 범위 검증
- 기본 설정의 TOML 왕복 테스트"
```

---

### Task 5: config 병합

설계 §4.2. 순수 모듈.

**Files:**
- Create: `src/config/merge.rs`
- Modify: `src/config/mod.rs`

**Interfaces:**
- Consumes: `Config`, `Style`, `Anchor`, `DisplayOverride` (Task 4)
- Produces:
  - `pub struct Effective { pub enabled: bool, pub anchor: Anchor, pub offset_px: [i32; 2], pub style: Style }`
  - `pub fn effective_for(cfg: &Config, monitor_id: &str) -> Effective`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/config/merge.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Anchor, Config, DisplayOverride, DrawMode};

    fn cfg_with(over: Vec<DisplayOverride>) -> Config {
        Config { displays: over, ..Config::default() }
    }

    #[test]
    fn no_override_yields_global_defaults() {
        let cfg = cfg_with(vec![]);
        let e = effective_for(&cfg, "MON-A");
        assert!(e.enabled);
        assert_eq!(e.anchor, Anchor::Center);
        assert_eq!(e.offset_px, [0, 0]);
        assert_eq!(e.style.size_px, 64.0);
        assert_eq!(e.style.font_family, "Consolas");
    }

    #[test]
    fn unrelated_override_is_ignored() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-B".into(),
            size_px: Some(120.0),
            ..DisplayOverride::default()
        }]);
        assert_eq!(effective_for(&cfg, "MON-A").style.size_px, 64.0);
    }

    #[test]
    fn partial_override_replaces_only_present_fields() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            anchor: Some(Anchor::TopCenter),
            size_px: Some(48.0),
            ..DisplayOverride::default()
        }]);
        let e = effective_for(&cfg, "MON-A");
        assert_eq!(e.anchor, Anchor::TopCenter);
        assert_eq!(e.style.size_px, 48.0);
        // untouched fields keep global values
        assert_eq!(e.style.font_family, "Consolas");
        assert_eq!(e.style.opacity, 0.85);
        assert_eq!(e.offset_px, [0, 0]);
        assert!(e.enabled);
    }

    #[test]
    fn enabled_false_is_respected() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            enabled: Some(false),
            ..DisplayOverride::default()
        }]);
        assert!(!effective_for(&cfg, "MON-A").enabled);
    }

    #[test]
    fn every_style_field_can_be_overridden() {
        let cfg = cfg_with(vec![DisplayOverride {
            id: "MON-A".into(),
            font_family: Some("Impact".into()),
            font_weight: Some(800),
            size_px: Some(10.0),
            mode: Some(DrawMode::Both),
            color: Some("#112233".into()),
            outline_color: Some("#445566".into()),
            outline_width_px: Some(3.0),
            opacity: Some(0.1),
            letter_spacing_em: Some(0.5),
            shadow: Some(false),
            tabular_figures: Some(false),
            show_summary_line: Some(false),
            ..DisplayOverride::default()
        }]);
        let s = effective_for(&cfg, "MON-A").style;
        assert_eq!(s.font_family, "Impact");
        assert_eq!(s.font_weight, 800);
        assert_eq!(s.size_px, 10.0);
        assert_eq!(s.mode, DrawMode::Both);
        assert_eq!(s.color, "#112233");
        assert_eq!(s.outline_color, "#445566");
        assert_eq!(s.outline_width_px, 3.0);
        assert_eq!(s.opacity, 0.1);
        assert_eq!(s.letter_spacing_em, 0.5);
        assert!(!s.shadow);
        assert!(!s.tabular_figures);
        assert!(!s.show_summary_line);
    }

    #[test]
    fn first_matching_override_wins() {
        let cfg = cfg_with(vec![
            DisplayOverride { id: "MON-A".into(), size_px: Some(10.0), ..Default::default() },
            DisplayOverride { id: "MON-A".into(), size_px: Some(20.0), ..Default::default() },
        ]);
        assert_eq!(effective_for(&cfg, "MON-A").style.size_px, 10.0);
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/config/mod.rs`에 `mod merge; pub use merge::*;` 추가 후,

Run: `cargo test merge`
Expected: 컴파일 실패. `cannot find function 'effective_for'`.

- [ ] **Step 3: 최소 구현 작성**

`src/config/merge.rs`:

```rust
//! Merges global defaults with per-monitor overrides. No Win32, no I/O.

use super::{Anchor, Config, Style};

/// The resolved settings for one monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    pub enabled: bool,
    pub anchor: Anchor,
    pub offset_px: [i32; 2],
    pub style: Style,
}

/// Only fields present in the matching `[[display]]` entry override the globals.
/// A monitor with no entry gets the globals and is enabled.
pub fn effective_for(cfg: &Config, monitor_id: &str) -> Effective {
    let mut e = Effective {
        enabled: true,
        anchor: cfg.layout.anchor,
        offset_px: cfg.layout.offset_px,
        style: cfg.style.clone(),
    };

    let Some(o) = cfg.displays.iter().find(|d| d.id == monitor_id) else {
        return e;
    };

    if let Some(v) = o.enabled { e.enabled = v; }
    if let Some(v) = o.anchor { e.anchor = v; }
    if let Some(v) = o.offset_px { e.offset_px = v; }

    if let Some(v) = &o.font_family { e.style.font_family = v.clone(); }
    if let Some(v) = o.font_weight { e.style.font_weight = v; }
    if let Some(v) = o.size_px { e.style.size_px = v; }
    if let Some(v) = o.mode { e.style.mode = v; }
    if let Some(v) = &o.color { e.style.color = v.clone(); }
    if let Some(v) = &o.outline_color { e.style.outline_color = v.clone(); }
    if let Some(v) = o.outline_width_px { e.style.outline_width_px = v; }
    if let Some(v) = o.opacity { e.style.opacity = v; }
    if let Some(v) = o.letter_spacing_em { e.style.letter_spacing_em = v; }
    if let Some(v) = o.shadow { e.style.shadow = v; }
    if let Some(v) = o.tabular_figures { e.style.tabular_figures = v; }
    if let Some(v) = o.show_summary_line { e.style.show_summary_line = v; }

    e
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test merge`
Expected: 6개 PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/config/
git commit -m "config 병합: 전역 기본값 + 모니터별 오버라이드"
```

---

### Task 6: layout 계산

설계 §4.3. 순수 모듈. 이 환경의 음수 좌표 모니터와 세로 모니터가 테스트에 들어간다.

**Files:**
- Create: `src/layout.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::config::Anchor`
- Produces:
  - `pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }`
  - `pub fn place(monitor: Rect, content_w: i32, content_h: i32, anchor: Anchor, offset: [i32; 2]) -> Rect`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/layout.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Anchor::*;

    /// The user's leftmost monitor sits at negative virtual-desktop coordinates.
    const NEG: Rect = Rect { x: -3840, y: -368, w: 2560, h: 1440 };
    /// The user's portrait monitor.
    const PORTRAIT: Rect = Rect { x: 2560, y: -556, w: 1440, h: 2560 };

    const CW: i32 = 800;
    const CH: i32 = 140;

    #[test]
    fn center_on_negative_origin_monitor() {
        let r = place(NEG, CW, CH, Center, [0, 0]);
        assert_eq!(r, Rect { x: -2960, y: 282, w: CW, h: CH });
    }

    #[test]
    fn top_left_is_the_monitor_origin() {
        assert_eq!(place(NEG, CW, CH, TopLeft, [0, 0]), Rect { x: -3840, y: -368, w: CW, h: CH });
    }

    #[test]
    fn bottom_right_hugs_the_far_corner() {
        assert_eq!(place(NEG, CW, CH, BottomRight, [0, 0]), Rect { x: -2080, y: 932, w: CW, h: CH });
    }

    #[test]
    fn offset_is_applied_after_the_anchor() {
        let r = place(NEG, CW, CH, BottomRight, [-40, -80]);
        assert_eq!(r, Rect { x: -2120, y: 852, w: CW, h: CH });
    }

    #[test]
    fn center_on_portrait_monitor() {
        let r = place(PORTRAIT, CW, CH, Center, [0, 0]);
        assert_eq!(r, Rect { x: 2880, y: 654, w: CW, h: CH });
    }

    #[test]
    fn top_center_and_middle_left_and_bottom_center() {
        assert_eq!(place(NEG, CW, CH, TopCenter, [0, 0]).x, -2960);
        assert_eq!(place(NEG, CW, CH, TopCenter, [0, 0]).y, -368);
        assert_eq!(place(NEG, CW, CH, MiddleLeft, [0, 0]).x, -3840);
        assert_eq!(place(NEG, CW, CH, MiddleLeft, [0, 0]).y, 282);
        assert_eq!(place(NEG, CW, CH, BottomCenter, [0, 0]).x, -2960);
        assert_eq!(place(NEG, CW, CH, BottomCenter, [0, 0]).y, 932);
    }

    #[test]
    fn top_right_and_middle_right_and_bottom_left() {
        assert_eq!(place(NEG, CW, CH, TopRight, [0, 0]), Rect { x: -2080, y: -368, w: CW, h: CH });
        assert_eq!(place(NEG, CW, CH, MiddleRight, [0, 0]), Rect { x: -2080, y: 282, w: CW, h: CH });
        assert_eq!(place(NEG, CW, CH, BottomLeft, [0, 0]), Rect { x: -3840, y: 932, w: CW, h: CH });
    }

    #[test]
    fn content_wider_than_monitor_overhangs_symmetrically() {
        let r = place(NEG, 3000, CH, Center, [0, 0]);
        assert_eq!(r.x, -4060); // -3840 + (2560 - 3000) / 2
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod layout;` 추가 후,

Run: `cargo test layout`
Expected: 컴파일 실패. `cannot find type 'Rect'`.

- [ ] **Step 3: 최소 구현 작성**

```rust
//! Anchor + offset arithmetic in virtual-desktop coordinates. No Win32, no I/O.

use crate::config::Anchor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Places a `content_w` x `content_h` box inside `monitor` (whose coordinates may be
/// negative), anchored per `anchor`, then shifts it by `offset` (+x right, +y down).
///
/// The anchor is relative to the monitor's full rectangle, not its work area, so
/// bottom anchors can land under the taskbar. Callers lift them with a negative y offset.
pub fn place(monitor: Rect, content_w: i32, content_h: i32, anchor: Anchor, offset: [i32; 2]) -> Rect {
    use Anchor::*;

    let x = match anchor {
        TopLeft | MiddleLeft | BottomLeft => monitor.x,
        TopCenter | Center | BottomCenter => monitor.x + (monitor.w - content_w) / 2,
        TopRight | MiddleRight | BottomRight => monitor.x + monitor.w - content_w,
    };
    let y = match anchor {
        TopLeft | TopCenter | TopRight => monitor.y,
        MiddleLeft | Center | MiddleRight => monitor.y + (monitor.h - content_h) / 2,
        BottomLeft | BottomCenter | BottomRight => monitor.y + monitor.h - content_h,
    };

    Rect { x: x + offset[0], y: y + offset[1], w: content_w, h: content_h }
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test layout`
Expected: 8개 PASS.

`content_wider_than_monitor_overhangs_symmetrically`가 실패하면 `(2560 - 3000) / 2`가 Rust에서 `-220`(0 방향 절단)이라는 점을 확인한다. 기댓값은 그 동작을 전제한다.

- [ ] **Step 5: 커밋**

```bash
git add src/layout.rs src/lib.rs
git commit -m "layout 모듈: 앵커/오프셋 배치 계산

- 가상 데스크톱 음수 좌표 모니터와 세로 모니터 테스트 포함"
```

---

### Task 7: 로깅, 단일 인스턴스, DPI 인식, 설정 파일 경로

설계 §5.1, §6. 여기서 처음으로 `main.rs`가 실제 동작을 한다.

**Files:**
- Create: `src/logging.rs`
- Create: `src/paths.rs`
- Create: `src/single_instance.rs`
- Create: `src/config/io.rs`
- Modify: `src/config/mod.rs`, `src/lib.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `Config`, `validate` (Task 4)
- Produces:
  - `pub fn paths::config_path() -> anyhow::Result<std::path::PathBuf>` — `%APPDATA%\DesktopCountdown\config.toml`
  - `pub fn paths::log_dir() -> anyhow::Result<std::path::PathBuf>` — `%LOCALAPPDATA%\DesktopCountdown`
  - `pub fn logging::init(dir: &std::path::Path) -> tracing_appender::non_blocking::WorkerGuard`
  - `pub fn single_instance::acquire() -> anyhow::Result<SingleInstance>` — 이미 실행 중이면 `Err`
  - `pub fn config::load_or_create(path: &std::path::Path) -> anyhow::Result<Config>`
  - `pub fn config::save(path: &std::path::Path, cfg: &Config) -> anyhow::Result<()>`

- [ ] **Step 1: config I/O 테스트 작성**

`src/config/io.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("dc-test-{name}"));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p.push("config.toml");
        p
    }

    #[test]
    fn creates_default_file_when_missing() {
        let p = tmp("create");
        assert!(!p.exists());
        let cfg = load_or_create(&p).unwrap();
        assert!(p.exists());
        assert_eq!(cfg, Config::default());
        // The written file must parse back to the same config.
        let text = fs::read_to_string(&p).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn reads_existing_file() {
        let p = tmp("read");
        fs::write(&p, "target = \"2030-01-01T00:00:00\"\n[style]\nsize_px = 99.0\n").unwrap();
        let cfg = load_or_create(&p).unwrap();
        assert_eq!(cfg.style.size_px, 99.0);
    }

    #[test]
    fn rejects_invalid_values() {
        let p = tmp("invalid");
        fs::write(&p, "target = \"2030-01-01T00:00:00\"\n[style]\nopacity = 3.0\n").unwrap();
        assert!(load_or_create(&p).is_err());
    }

    #[test]
    fn rejects_malformed_toml() {
        let p = tmp("malformed");
        fs::write(&p, "target = \"2030-01-01T00:00:00\"\n[style\n").unwrap();
        assert!(load_or_create(&p).is_err());
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run: `cargo test config::io`
Expected: 컴파일 실패. `cannot find function 'load_or_create'`.

- [ ] **Step 3: 구현 작성**

`src/paths.rs`:

```rust
//! Filesystem locations. Uses the standard Windows environment variables.

use std::path::PathBuf;

use anyhow::{anyhow, Result};

const APP_DIR: &str = "DesktopCountdown";

pub fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
    let mut p = PathBuf::from(base);
    p.push(APP_DIR);
    std::fs::create_dir_all(&p)?;
    p.push("config.toml");
    Ok(p)
}

pub fn log_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA").ok_or_else(|| anyhow!("LOCALAPPDATA is not set"))?;
    let mut p = PathBuf::from(base);
    p.push(APP_DIR);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}
```

`src/config/io.rs`:

```rust
//! Loading and saving `config.toml`.

use std::path::Path;

use anyhow::{Context, Result};

use super::{validate, Config};

/// Reads the config, creating it with defaults if it does not exist.
/// Returns `Err` on malformed TOML or values outside their allowed range;
/// callers keep their previous config in that case.
pub fn load_or_create(path: &Path) -> Result<Config> {
    if !path.exists() {
        let cfg = Config::default();
        save(path, &cfg)?;
        return Ok(cfg);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config = toml::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn save(path: &Path, cfg: &Config) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
```

`src/logging.rs`:

```rust
//! File-only logging. This app has no console, so the log file is the only diagnostic.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Keep the returned guard alive for the process lifetime; dropping it stops the writer.
pub fn init(dir: &Path) -> WorkerGuard {
    let appender = tracing_appender::rolling::never(dir, "log.txt");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));

    guard
}
```

`src/single_instance.rs`:

```rust
//! Named-mutex single instance guard, scoped to the current session.

use anyhow::{bail, Result};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

pub struct SingleInstance(HANDLE);

impl SingleInstance {
    /// Returns `Err` if another instance already holds the mutex.
    pub fn acquire() -> Result<Self> {
        unsafe {
            let handle = CreateMutexW(None, true, w!("Local\\DesktopCountdown"))?;
            if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                bail!("another instance is already running");
            }
            Ok(Self(handle))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
```

`src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use desktop_countdown::{config, logging, paths, single_instance::SingleInstance};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

fn main() -> Result<()> {
    // Must happen before any window or monitor query.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };

    let _guard = logging::init(&paths::log_dir()?);
    let _instance = SingleInstance::acquire()?;

    let cfg_path = paths::config_path()?;
    let cfg = config::load_or_create(&cfg_path)?;
    tracing::info!(?cfg_path, target = %cfg.target, "starting");

    Ok(())
}
```

`src/lib.rs`:

```rust
//! DesktopCountdown — draws a countdown onto the desktop wallpaper layer.

pub mod color;
pub mod config;
pub mod countdown;
pub mod layout;
pub mod logging;
pub mod paths;
pub mod single_instance;
```

`src/config/mod.rs`:

```rust
mod io;
mod merge;
mod schema;

pub use io::*;
pub use merge::*;
pub use schema::*;
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test`
Expected: 지금까지의 모든 테스트 PASS (countdown 9, color 5, config 11, merge 6, io 4, layout 8).

- [ ] **Step 5: 실제 실행 확인**

Run: `cargo run`
Expected: 즉시 종료된다. `%APPDATA%\DesktopCountdown\config.toml`이 생성되고, `%LOCALAPPDATA%\DesktopCountdown\log.txt`에 `starting` 줄이 남는다.

Run: 두 번째 인스턴스를 띄우기 위해 `cargo run`을 두 번 동시에 실행할 수는 없으므로, `main`의 `Ok(())` 앞에 임시로 `std::thread::sleep(std::time::Duration::from_secs(10));`을 넣고 두 터미널에서 실행해 두 번째가 `another instance is already running`으로 죽는지 확인한 뒤 다시 지운다.

- [ ] **Step 6: 커밋**

```bash
git add src/
git commit -m "로깅, 단일 인스턴스, DPI 인식, 설정 파일 입출력

- %APPDATA%에 config.toml 자동 생성
- %LOCALAPPDATA%에 log.txt, 패닉 훅 연결
- Per-Monitor V2 DPI 인식을 창 생성 전에 설정"
```

---

### Task 8: 모니터 열거

설계 §4.1. Win32 모듈. 안정적인 디바이스 ID를 얻는다.

**Files:**
- Create: `src/monitors.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::layout::Rect` (Task 6)
- Produces:
  - `pub struct MonitorInfo { pub id: String, pub name: String, pub rect: Rect, pub dpi: u32 }`
  - `pub fn enumerate() -> anyhow::Result<Vec<MonitorInfo>>`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/monitors.rs` 하단에. 실제 하드웨어에 의존하므로 스모크 테스트다.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn enumerates_at_least_one_monitor() {
        let ms = enumerate().unwrap();
        assert!(!ms.is_empty());
    }

    #[test]
    fn every_monitor_has_a_positive_size_and_dpi() {
        for m in enumerate().unwrap() {
            assert!(m.rect.w > 0, "{m:?}");
            assert!(m.rect.h > 0, "{m:?}");
            assert!(m.dpi >= 96, "{m:?}");
        }
    }

    #[test]
    fn ids_are_unique_and_nonempty() {
        let ms = enumerate().unwrap();
        let ids: HashSet<_> = ms.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), ms.len(), "duplicate monitor ids: {ms:#?}");
        assert!(ms.iter().all(|m| !m.id.is_empty()));
    }

    #[test]
    fn names_are_nonempty() {
        assert!(enumerate().unwrap().iter().all(|m| !m.name.is_empty()));
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod monitors;` 추가 후,

Run: `cargo test monitors`
Expected: 컴파일 실패. `cannot find function 'enumerate'`.

- [ ] **Step 3: 구현 작성**

```rust
//! Monitor enumeration with stable per-device identifiers.

use std::cell::RefCell;

use anyhow::Result;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
    EDD_GET_DEVICE_INTERFACE_NAME, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use crate::layout::Rect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Stable across reboots and cable swaps, e.g. `\\?\DISPLAY#DEL41A8#...`.
    pub id: String,
    /// Display only, e.g. `\\.\DISPLAY1 (2560x1440)`. Never used for identity.
    pub name: String,
    /// Virtual-desktop coordinates in physical pixels. May be negative.
    pub rect: Rect,
    pub dpi: u32,
}

thread_local! {
    static SINK: RefCell<Vec<MonitorInfo>> = const { RefCell::new(Vec::new()) };
}

pub fn enumerate() -> Result<Vec<MonitorInfo>> {
    SINK.with(|s| s.borrow_mut().clear());
    unsafe {
        EnumDisplayMonitors(None, None, Some(monitor_cb), LPARAM(0)).ok()?;
    }
    Ok(SINK.with(|s| s.borrow().clone()))
}

unsafe extern "system" fn monitor_cb(hmon: HMONITOR, _hdc: HDC, _rc: *mut RECT, _lp: LPARAM) -> BOOL {
    if let Some(info) = describe(hmon) {
        SINK.with(|s| s.borrow_mut().push(info));
    }
    TRUE
}

unsafe fn describe(hmon: HMONITOR) -> Option<MonitorInfo> {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    GetMonitorInfoW(hmon, &mut mi.monitorInfo as *mut _).ok().ok()?;

    let device = wide_to_string(&mi.szDevice);
    let r = mi.monitorInfo.rcMonitor;
    let rect = Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };

    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);

    // The device interface name survives reboots and port changes; szDevice does not.
    let mut dd = DISPLAY_DEVICEW::default();
    dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
    let device_wide: Vec<u16> = mi.szDevice.to_vec();
    let ok = EnumDisplayDevicesW(
        windows::core::PCWSTR(device_wide.as_ptr()),
        0,
        &mut dd,
        EDD_GET_DEVICE_INTERFACE_NAME,
    )
    .as_bool();

    let id = if ok && dd.DeviceID[0] != 0 {
        wide_to_string(&dd.DeviceID)
    } else {
        // Fall back to the unstable name rather than dropping the monitor entirely.
        device.clone()
    };

    Some(MonitorInfo {
        id,
        name: format!("{} ({}x{})", device, rect.w, rect.h),
        rect,
        dpi: dpi_x,
    })
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test monitors -- --nocapture`
Expected: 4개 PASS.

`ids_are_unique_and_nonempty`가 실패하면 `EDD_GET_DEVICE_INTERFACE_NAME` 호출이 빈 `DeviceID`를 돌려준 것이다. 이때는 `dd.DeviceKey`(레지스트리 키)를 대신 쓴다. 두 필드 모두 비면 `device` 폴백이 걸리며, 그 경우 두 모니터가 같은 ID를 가질 수는 없다(`szDevice`는 유일).

- [ ] **Step 5: 실제 값 눈으로 확인**

`src/main.rs`의 `Ok(())` 앞에 임시로 다음을 넣고 `cargo run`으로 4개 모니터가 전부 나오는지, 좌표가 음수인 것이 포함되는지 확인한 뒤 지운다.

```rust
for m in desktop_countdown::monitors::enumerate()? {
    println!("{m:#?}");
}
```

- [ ] **Step 6: 커밋**

```bash
git add src/monitors.rs src/lib.rs
git commit -m "monitors 모듈: 안정적인 디바이스 ID로 모니터 열거

- EnumDisplayDevices의 인터페이스 이름을 식별자로 사용
- 가상 데스크톱 물리 좌표와 모니터별 DPI 수집"
```

---

### Task 9: 렌더러 — fill 모드

설계 §3.2. Direct2D WIC 비트맵 렌더 타깃 + DirectWrite. 프리멀티플라이드 BGRA 버퍼를 내놓는다.

**Files:**
- Create: `src/render/mod.rs`
- Create: `src/render/text.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::config::{Style, DrawMode}` (Task 4), `crate::color::parse_hex` (Task 3)
- Produces:
  - `pub struct RenderedText { pub width: u32, pub height: u32, pub pixels: Vec<u8> }` — 프리멀티플라이드 BGRA, top-down, `pixels.len() == width * height * 4`
  - `pub struct Lines { pub summary: Option<String>, pub main: String }`
  - `pub struct Renderer` + `pub fn Renderer::new() -> anyhow::Result<Renderer>`
  - `pub fn Renderer::render(&self, lines: &Lines, style: &Style) -> anyhow::Result<RenderedText>`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/render/mod.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DrawMode, Style};

    fn lines() -> Lines {
        Lines { summary: Some("3m 2w 0d".into()), main: "2544:18:07".into() }
    }

    /// Fraction of pixels with non-zero alpha.
    fn coverage(r: &RenderedText) -> f64 {
        let opaque = r.pixels.chunks_exact(4).filter(|p| p[3] != 0).count();
        opaque as f64 / (r.width * r.height) as f64
    }

    /// Bounding box of non-zero alpha, as (min_x, min_y, max_x, max_y).
    fn ink_bbox(r: &RenderedText) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..r.height {
            for x in 0..r.width {
                let a = r.pixels[((y * r.width + x) * 4 + 3) as usize];
                if a != 0 {
                    x0 = x0.min(x); y0 = y0.min(y);
                    x1 = x1.max(x); y1 = y1.max(y);
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn renders_something() {
        let r = Renderer::new().unwrap();
        let img = r.render(&lines(), &Style::default()).unwrap();
        assert!(img.width > 0 && img.height > 0);
        assert_eq!(img.pixels.len(), (img.width * img.height * 4) as usize);
        assert!(coverage(&img) > 0.01, "nothing was drawn");
        assert!(coverage(&img) < 0.9, "canvas is almost fully opaque");
    }

    #[test]
    fn ink_stays_inside_the_canvas_with_padding() {
        let r = Renderer::new().unwrap();
        let img = r.render(&lines(), &Style::default()).unwrap();
        let (x0, y0, x1, y1) = ink_bbox(&img);
        assert!(x0 >= 1 && y0 >= 1, "ink touches the top-left edge");
        assert!(x1 < img.width - 1 && y1 < img.height - 1, "ink touches the bottom-right edge");
    }

    #[test]
    fn alpha_is_premultiplied() {
        let r = Renderer::new().unwrap();
        let img = r.render(&lines(), &Style::default()).unwrap();
        for p in img.pixels.chunks_exact(4) {
            let (b, g, r_, a) = (p[0], p[1], p[2], p[3]);
            assert!(b <= a && g <= a && r_ <= a, "channel exceeds alpha: {p:?}");
        }
    }

    #[test]
    fn hiding_the_summary_line_shrinks_the_canvas() {
        let r = Renderer::new().unwrap();
        let tall = r.render(&lines(), &Style::default()).unwrap();
        let short = r
            .render(&Lines { summary: None, main: "2544:18:07".into() }, &Style::default())
            .unwrap();
        assert!(short.height < tall.height);
    }

    #[test]
    fn bigger_font_yields_a_bigger_canvas() {
        let r = Renderer::new().unwrap();
        let small = r.render(&lines(), &Style { size_px: 32.0, ..Style::default() }).unwrap();
        let big = r.render(&lines(), &Style { size_px: 96.0, ..Style::default() }).unwrap();
        assert!(big.width > small.width && big.height > small.height);
    }

    #[test]
    fn missing_font_falls_back_instead_of_failing() {
        let r = Renderer::new().unwrap();
        let style = Style { font_family: "NoSuchFontFamily12345".into(), ..Style::default() };
        let img = r.render(&lines(), &style).unwrap();
        assert!(coverage(&img) > 0.01);
    }

    #[test]
    fn shadow_adds_ink() {
        let r = Renderer::new().unwrap();
        let without = r.render(&lines(), &Style { shadow: false, ..Style::default() }).unwrap();
        let with = r.render(&lines(), &Style { shadow: true, ..Style::default() }).unwrap();
        assert!(coverage(&with) > coverage(&without));
    }

    #[test]
    fn tabular_figures_keep_the_width_constant_across_digits() {
        let r = Renderer::new().unwrap();
        let style = Style { tabular_figures: true, show_summary_line: false, ..Style::default() };
        let a = r.render(&Lines { summary: None, main: "11:11:11".into() }, &style).unwrap();
        let b = r.render(&Lines { summary: None, main: "00:00:00".into() }, &style).unwrap();
        assert_eq!(a.width, b.width);
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod render;` 추가 후,

Run: `cargo test render`
Expected: 컴파일 실패. `cannot find type 'Renderer'`.

- [ ] **Step 3: 텍스트 레이아웃 헬퍼 작성**

`src/render/text.rs`:

```rust
//! DirectWrite text layout construction.

use anyhow::{anyhow, Result};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Graphics::DirectWrite::*;

use crate::config::Style;

/// Families tried, in order, when the configured one is not installed.
const FALLBACKS: [&str; 2] = ["Consolas", "Segoe UI"];

pub struct TextEngine {
    pub factory: IDWriteFactory,
}

impl TextEngine {
    pub fn new() -> Result<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(Self { factory })
    }

    fn family_exists(&self, family: &str) -> bool {
        unsafe {
            let mut coll = None;
            if self.factory.GetSystemFontCollection(&mut coll, false).is_err() {
                return false;
            }
            let Some(coll) = coll else { return false };
            let mut index = 0u32;
            let mut exists = windows::Win32::Foundation::BOOL(0);
            if coll.FindFamilyName(&HSTRING::from(family), &mut index, &mut exists).is_err() {
                return false;
            }
            exists.as_bool()
        }
    }

    /// Returns the configured family if installed, otherwise the first installed fallback.
    pub fn resolve_family(&self, family: &str) -> String {
        if self.family_exists(family) {
            return family.to_string();
        }
        tracing::warn!(family, "font family not installed, falling back");
        for f in FALLBACKS {
            if self.family_exists(f) {
                return f.to_string();
            }
        }
        family.to_string() // let DirectWrite do whatever it does
    }

    /// Builds a single-line layout. `size_px` is the em size in physical pixels.
    pub fn layout(&self, text: &str, family: &str, style: &Style, size_px: f32) -> Result<IDWriteTextLayout> {
        let utf16: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let format = self.factory.CreateTextFormat(
                &HSTRING::from(family),
                None,
                DWRITE_FONT_WEIGHT(style.font_weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                &HSTRING::from("ko-kr"),
            )?;

            let layout = self.factory.CreateTextLayout(&utf16, &format, 8192.0, 8192.0)?;
            let range = DWRITE_TEXT_RANGE { startPosition: 0, length: utf16.len() as u32 };

            if style.letter_spacing_em != 0.0 {
                let l1: IDWriteTextLayout1 = layout.cast()?;
                l1.SetCharacterSpacing(0.0, style.letter_spacing_em * size_px, 0.0, range)?;
            }

            if style.tabular_figures {
                let typo = self.factory.CreateTypography()?;
                typo.AddFontFeature(DWRITE_FONT_FEATURE {
                    nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                    parameter: 1,
                })?;
                layout.SetTypography(&typo, range)?;
            }

            Ok(layout)
        }
    }

    pub fn measure(layout: &IDWriteTextLayout) -> Result<(f32, f32)> {
        let m = unsafe { layout.GetMetrics()? };
        Ok((m.widthIncludingTrailingWhitespace, m.height))
    }
}
```

`use` 목록은 실제로 쓰는 것만 남긴다: `anyhow::Result`, `windows::core::{Interface, HSTRING}`,
`windows::Win32::Graphics::DirectWrite::*`, `crate::config::Style`. `anyhow!`와 `PCWSTR`은 쓰지 않는다.

- [ ] **Step 4: 렌더러 본체 작성**

`src/render/mod.rs`:

```rust
//! Draws the countdown into a premultiplied BGRA buffer using Direct2D + DirectWrite.

mod text;

use anyhow::{anyhow, Result};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};

use crate::color::parse_hex;
use crate::config::{DrawMode, Style};
use text::TextEngine;

/// Gap between the summary line and the main line, as a fraction of `size_px`.
const LINE_GAP_RATIO: f32 = 0.12;
/// Summary line em size, as a fraction of `size_px`.
const SUMMARY_RATIO: f32 = 0.28;
/// Shadow offset in pixels.
const SHADOW_OFFSET: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    pub summary: Option<String>,
    pub main: String,
}

/// Premultiplied BGRA, top-down, tightly packed.
#[derive(Debug, Clone)]
pub struct RenderedText {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct Renderer {
    d2d: ID2D1Factory,
    wic: IWICImagingFactory,
    text: TextEngine,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        unsafe {
            // WIC needs COM. Ignore RPC_E_CHANGED_MODE if something already initialised it.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { d2d, wic, text: TextEngine::new()? })
        }
    }

    pub fn render(&self, lines: &Lines, style: &Style) -> Result<RenderedText> {
        let family = self.text.resolve_family(&style.font_family);

        let main = self.text.layout(&lines.main, &family, style, style.size_px)?;
        let (main_w, main_h) = TextEngine::measure(&main)?;

        let summary = match (&lines.summary, style.show_summary_line) {
            (Some(s), true) => {
                let l = self.text.layout(s, &family, style, style.size_px * SUMMARY_RATIO)?;
                let m = TextEngine::measure(&l)?;
                Some((l, m.0, m.1))
            }
            _ => None,
        };

        let gap = if summary.is_some() { style.size_px * LINE_GAP_RATIO } else { 0.0 };
        let sum_h = summary.as_ref().map(|s| s.2).unwrap_or(0.0);
        let sum_w = summary.as_ref().map(|s| s.1).unwrap_or(0.0);

        let pad = (style.outline_width_px.max(0.0) + 4.0
            + if style.shadow { SHADOW_OFFSET } else { 0.0 })
            .ceil();

        let content_w = main_w.max(sum_w);
        let content_h = sum_h + gap + main_h;
        let width = (content_w + pad * 2.0).ceil().max(1.0) as u32;
        let height = (content_h + pad * 2.0).ceil().max(1.0) as u32;

        let bitmap = unsafe {
            self.wic.CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?
        };

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };

        unsafe {
            let rt = self.d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
            // ClearType writes subpixel colour fringes that destroy the alpha channel
            // a layered window depends on. Grayscale is mandatory here.
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

            let ink = self.brush(&rt, &style.color, 1.0)?;
            let shadow = self.brush(&rt, "#000000", 0.55)?;

            let sum_x = pad + (content_w - sum_w) / 2.0;
            let main_x = pad + (content_w - main_w) / 2.0;
            let sum_y = pad;
            let main_y = pad + sum_h + gap;

            if style.shadow {
                if let Some((l, _, _)) = &summary {
                    rt.DrawTextLayout(
                        D2D_POINT_2F { x: sum_x + SHADOW_OFFSET, y: sum_y + SHADOW_OFFSET },
                        l, &shadow, D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                rt.DrawTextLayout(
                    D2D_POINT_2F { x: main_x + SHADOW_OFFSET, y: main_y + SHADOW_OFFSET },
                    &main, &shadow, D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }

            if matches!(style.mode, DrawMode::Fill | DrawMode::Both) {
                if let Some((l, _, _)) = &summary {
                    rt.DrawTextLayout(D2D_POINT_2F { x: sum_x, y: sum_y }, l, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
                rt.DrawTextLayout(D2D_POINT_2F { x: main_x, y: main_y }, &main, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }

            rt.EndDraw(None, None)?;
        }

        let pixels = unsafe { copy_out(&bitmap, width, height)? };
        Ok(RenderedText { width, height, pixels })
    }

    fn brush(&self, rt: &ID2D1RenderTarget, hex: &str, alpha: f32) -> Result<ID2D1SolidColorBrush> {
        let c = parse_hex(hex).ok_or_else(|| anyhow!("invalid colour {hex}"))?;
        let color = D2D1_COLOR_F {
            r: c.r as f32 / 255.0,
            g: c.g as f32 / 255.0,
            b: c.b as f32 / 255.0,
            a: alpha,
        };
        Ok(unsafe { rt.CreateSolidColorBrush(&color, None)? })
    }
}

/// Copies the WIC bitmap into a tightly packed top-down BGRA buffer.
unsafe fn copy_out(bitmap: &IWICBitmap, width: u32, height: u32) -> Result<Vec<u8>> {
    let rect = WICRect { X: 0, Y: 0, Width: width as i32, Height: height as i32 };
    let lock = bitmap.Lock(&rect, WICBitmapLockRead.0 as u32)?;

    let mut stride = 0u32;
    lock.GetStride(&mut stride)?;

    let mut size = 0u32;
    let mut ptr = std::ptr::null_mut();
    lock.GetDataPointer(&mut size, &mut ptr)?;

    let row_bytes = (width * 4) as usize;
    let mut out = vec![0u8; row_bytes * height as usize];
    for y in 0..height as usize {
        let src = ptr.add(y * stride as usize);
        std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(y * row_bytes), row_bytes);
    }
    Ok(out)
}
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test render -- --test-threads=1`
Expected: 8개 PASS. (`--test-threads=1`은 COM 아파트먼트 초기화가 스레드마다 반복되는 것을 피하기 위해서다.)

`alpha_is_premultiplied`가 실패하면 `SetTextAntialiasMode`가 GRAYSCALE로 설정되지 않은 것이다.
`tabular_figures_keep_the_width_constant_across_digits`가 Consolas에서 실패하면 이미 고정폭이라 `tnum`이 무의미한 경우이므로, 실패할 리가 없다 — 실패하면 `letter_spacing_em` 적용이 문자열 길이에 따라 달라지는지 확인한다.

- [ ] **Step 6: 커밋**

```bash
git add src/render/ src/lib.rs
git commit -m "renderer: fill 모드 텍스트 렌더링

- WIC 비트맵 렌더 타깃에 프리멀티플라이드 BGRA 출력
- 레이어드 창을 위해 텍스트 AA를 GRAYSCALE로 고정
- 폰트 폴백, tabular figures, 자간, 오프셋 그림자
"
```

---

### Task 10: 렌더러 — outline / both 모드

설계 §3.2. 커스텀 `IDWriteTextRenderer`로 글리프 아웃라인 지오메트리를 뽑아 stroke한다.

**Files:**
- Create: `src/render/outline.rs`
- Modify: `src/render/mod.rs`

**Interfaces:**
- Consumes: `ID2D1Factory`, `IDWriteTextLayout` (Task 9)
- Produces:
  - `pub(crate) fn collect_geometry(d2d: &ID2D1Factory, layout: &IDWriteTextLayout, origin_x: f32, origin_y: f32) -> anyhow::Result<Vec<ID2D1Geometry>>`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/render/mod.rs`의 `mod tests`에 추가:

```rust
    #[test]
    fn outline_mode_draws_less_ink_than_fill() {
        let r = Renderer::new().unwrap();
        let base = Style { shadow: false, ..Style::default() };
        let fill = r.render(&lines(), &Style { mode: DrawMode::Fill, ..base.clone() }).unwrap();
        let outline = r.render(&lines(), &Style { mode: DrawMode::Outline, ..base.clone() }).unwrap();
        assert!(coverage(&outline) > 0.005, "outline drew nothing");
        assert!(
            coverage(&outline) < coverage(&fill),
            "outline {} should be lighter than fill {}",
            coverage(&outline),
            coverage(&fill)
        );
    }

    #[test]
    fn both_mode_draws_more_ink_than_fill() {
        let r = Renderer::new().unwrap();
        let base = Style { shadow: false, outline_width_px: 3.0, ..Style::default() };
        let fill = r.render(&lines(), &Style { mode: DrawMode::Fill, ..base.clone() }).unwrap();
        let both = r.render(&lines(), &Style { mode: DrawMode::Both, ..base.clone() }).unwrap();
        assert!(coverage(&both) > coverage(&fill));
    }

    #[test]
    fn outline_mode_leaves_glyph_centres_transparent() {
        // A thin outline of a large '0' must leave its interior empty.
        let r = Renderer::new().unwrap();
        let style = Style {
            mode: DrawMode::Outline,
            shadow: false,
            show_summary_line: false,
            size_px: 200.0,
            outline_width_px: 1.5,
            ..Style::default()
        };
        let img = r.render(&Lines { summary: None, main: "0".into() }, &style).unwrap();
        let cx = img.width / 2;
        let cy = img.height / 2;
        let a = img.pixels[((cy * img.width + cx) * 4 + 3) as usize];
        assert_eq!(a, 0, "the centre of '0' should be transparent in outline mode");
    }
```

`Style`에 `Clone`이 이미 있으므로 `..base.clone()`이 동작한다.

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run: `cargo test render -- --test-threads=1`
Expected: `outline_mode_draws_less_ink_than_fill` FAIL — outline 모드가 아무것도 그리지 않아 `coverage` 가 0이다.

- [ ] **Step 3: 커스텀 텍스트 렌더러 구현**

`src/render/outline.rs`:

```rust
//! Extracts glyph outlines from a text layout so they can be stroked.
//!
//! DirectWrite hands glyph runs to an `IDWriteTextRenderer` implementation; we turn
//! each run into an `ID2D1PathGeometry` translated to its baseline position.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

use anyhow::Result;
use windows::core::{implement, Interface, Ref};
use windows::Win32::Foundation::{BOOL, TRUE};
use windows::Win32::Graphics::Direct2D::Common::D2D_MATRIX_3X2_F;
use windows::Win32::Graphics::Direct2D::{ID2D1Factory, ID2D1Geometry};
use windows::Win32::Graphics::DirectWrite::*;

/// The collected geometries live behind an `Rc` so `collect_geometry` can read them
/// after `Draw` returns, without reaching inside the COM object.
type Collected = Rc<RefCell<Vec<ID2D1Geometry>>>;

#[implement(IDWriteTextRenderer)]
struct OutlineCollector {
    d2d: ID2D1Factory,
    geoms: Collected,
}

#[allow(non_snake_case)]
impl IDWritePixelSnapping_Impl for OutlineCollector_Impl {
    fn IsPixelSnappingDisabled(&self, _ctx: *const c_void) -> windows::core::Result<BOOL> {
        Ok(TRUE)
    }
    fn GetCurrentTransform(&self, _ctx: *const c_void, transform: *mut DWRITE_MATRIX) -> windows::core::Result<()> {
        unsafe {
            *transform = DWRITE_MATRIX { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };
        }
        Ok(())
    }
    fn GetPixelsPerDip(&self, _ctx: *const c_void) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

#[allow(non_snake_case)]
impl IDWriteTextRenderer_Impl for OutlineCollector_Impl {
    fn DrawGlyphRun(
        &self,
        _ctx: *const c_void,
        baseline_x: f32,
        baseline_y: f32,
        _mode: DWRITE_MEASURING_MODE,
        glyph_run: *const DWRITE_GLYPH_RUN,
        _desc: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _effect: Ref<windows::core::IUnknown>,
    ) -> windows::core::Result<()> {
        unsafe {
            let run = &*glyph_run;
            let Some(face) = run.fontFace.as_ref() else { return Ok(()) };

            let path = self.d2d.CreatePathGeometry()?;
            let sink = path.Open()?;

            let indices = std::slice::from_raw_parts(run.glyphIndices, run.glyphCount as usize);
            let advances = std::slice::from_raw_parts(run.glyphAdvances, run.glyphCount as usize);
            let offsets = std::slice::from_raw_parts(run.glyphOffsets, run.glyphCount as usize);

            face.GetGlyphRunOutline(
                run.fontEmSize,
                indices.as_ptr(),
                Some(advances.as_ptr()),
                Some(offsets.as_ptr()),
                run.glyphCount,
                run.isSideways.as_bool(),
                run.bidiLevel % 2 == 1,
                &sink,
            )?;
            sink.Close()?;

            // GetGlyphRunOutline emits coordinates relative to the baseline origin.
            let translate = D2D_MATRIX_3X2_F {
                Anonymous: windows::Win32::Graphics::Direct2D::Common::D2D_MATRIX_3X2_F_0 {
                    Anonymous: windows::Win32::Graphics::Direct2D::Common::D2D_MATRIX_3X2_F_0_0 {
                        m11: 1.0, m12: 0.0,
                        m21: 0.0, m22: 1.0,
                        m31: baseline_x, m32: baseline_y,
                    },
                },
            };
            let moved = self.d2d.CreateTransformedGeometry(&path, &translate)?;
            self.geoms.borrow_mut().push(moved.cast()?);
        }
        Ok(())
    }

    fn DrawUnderline(&self, _c: *const c_void, _x: f32, _y: f32, _u: *const DWRITE_UNDERLINE, _e: Ref<windows::core::IUnknown>) -> windows::core::Result<()> {
        Ok(())
    }
    fn DrawStrikethrough(&self, _c: *const c_void, _x: f32, _y: f32, _s: *const DWRITE_STRIKETHROUGH, _e: Ref<windows::core::IUnknown>) -> windows::core::Result<()> {
        Ok(())
    }
    fn DrawInlineObject(&self, _c: *const c_void, _x: f32, _y: f32, _o: Ref<IDWriteInlineObject>, _s: BOOL, _r: BOOL, _e: Ref<windows::core::IUnknown>) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Returns one geometry per glyph run in `layout`, positioned as if the layout were
/// drawn at (`origin_x`, `origin_y`).
pub(crate) fn collect_geometry(
    d2d: &ID2D1Factory,
    layout: &IDWriteTextLayout,
    origin_x: f32,
    origin_y: f32,
) -> Result<Vec<ID2D1Geometry>> {
    let geoms: Collected = Rc::new(RefCell::new(Vec::new()));
    let collector = OutlineCollector { d2d: d2d.clone(), geoms: Rc::clone(&geoms) };
    let renderer: IDWriteTextRenderer = collector.into();

    unsafe { layout.Draw(None, &renderer, origin_x, origin_y)? };
    drop(renderer); // release the COM object's Rc handle

    let out = geoms.borrow().clone();
    Ok(out)
}
```

`DrawGlyphRun` 안에서는 `self.geoms.borrow_mut().push(...)`가 그대로 동작한다 — `Rc<RefCell<..>>`도
`RefCell`과 같은 인터페이스다.

- [ ] **Step 4: 렌더러에 outline 경로 연결**

`src/render/mod.rs`의 `mod outline;`을 추가하고, `render()` 안의 그리기 블록을 아래로 바꾼다. `stroke_layout` 헬퍼를 `impl Renderer`에 추가한다.

```rust
    fn stroke_layout(
        &self,
        rt: &ID2D1RenderTarget,
        layout: &IDWriteTextLayout,
        x: f32,
        y: f32,
        brush: &ID2D1SolidColorBrush,
        width: f32,
    ) -> Result<()> {
        for geom in outline::collect_geometry(&self.d2d, layout, x, y)? {
            unsafe { rt.DrawGeometry(&geom, brush, width, None) };
        }
        Ok(())
    }
```

그리기 블록:

```rust
            let ink = self.brush(&rt, &style.color, 1.0)?;
            let line = self.brush(&rt, &style.outline_color, 1.0)?;
            let shadow = self.brush(&rt, "#000000", 0.55)?;

            let sum_x = pad + (content_w - sum_w) / 2.0;
            let main_x = pad + (content_w - main_w) / 2.0;
            let sum_y = pad;
            let main_y = pad + sum_h + gap;

            let draw_pass = |dx: f32, dy: f32, fill_brush: &ID2D1SolidColorBrush, stroke_brush: &ID2D1SolidColorBrush| -> Result<()> {
                if matches!(style.mode, DrawMode::Fill | DrawMode::Both) {
                    if let Some((l, _, _)) = &summary {
                        unsafe { rt.DrawTextLayout(D2D_POINT_2F { x: sum_x + dx, y: sum_y + dy }, l, fill_brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
                    }
                    unsafe { rt.DrawTextLayout(D2D_POINT_2F { x: main_x + dx, y: main_y + dy }, &main, fill_brush, D2D1_DRAW_TEXT_OPTIONS_NONE) };
                }
                if matches!(style.mode, DrawMode::Outline | DrawMode::Both) {
                    if let Some((l, _, _)) = &summary {
                        self.stroke_layout(&rt, l, sum_x + dx, sum_y + dy, stroke_brush, style.outline_width_px)?;
                    }
                    self.stroke_layout(&rt, &main, main_x + dx, main_y + dy, stroke_brush, style.outline_width_px)?;
                }
                Ok(())
            };

            if style.shadow {
                draw_pass(SHADOW_OFFSET, SHADOW_OFFSET, &shadow, &shadow)?;
            }
            draw_pass(0.0, 0.0, &ink, &line)?;
```

`use` 목록에 `windows::Win32::Graphics::DirectWrite::IDWriteTextLayout`를 추가한다.

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test render -- --test-threads=1`
Expected: 11개 PASS.

`outline_mode_leaves_glyph_centres_transparent`가 실패하면 `stroke_layout`이 아니라 `FillGeometry`를 부르고 있거나, `DrawMode::Outline`에서도 `DrawTextLayout`이 호출되고 있는 것이다.

- [ ] **Step 6: 커밋**

```bash
git add src/render/
git commit -m "renderer: outline/both 모드

- 커스텀 IDWriteTextRenderer로 글리프 런 아웃라인 지오메트리 수집
- outline은 stroke만, both는 fill + stroke
- 아웃라인 내부가 투명하게 남는지 검증"
```

---

### Task 11: WorkerW 확보와 레이어드 자식 창

설계 §3.3, §5.3. Task 1 스파이크의 코드를 제품 코드로 옮긴다.

**Files:**
- Create: `src/wallpaper_window.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::layout::Rect` (Task 6), `crate::render::RenderedText` (Task 9)
- Produces:
  - `pub fn acquire_workerw() -> anyhow::Result<HWND>`
  - `pub fn workerw_is_alive(hwnd: HWND) -> bool`
  - `pub struct CountdownWindow`
  - `pub fn CountdownWindow::create(parent: HWND) -> anyhow::Result<CountdownWindow>`
  - `pub fn CountdownWindow::update(&self, parent: HWND, rect: Rect, img: &RenderedText, opacity: f32) -> anyhow::Result<()>`
  - `impl Drop for CountdownWindow` — `DestroyWindow`

- [ ] **Step 1: 스모크 테스트 작성**

`src/wallpaper_window.rs` 하단에. Explorer가 없는 환경(CI)에서는 이 테스트가 의미 없으므로 `#[ignore]`를 붙이고 로컬에서만 돌린다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a live Explorer desktop; run with --ignored"]
    fn acquires_workerw_and_reports_it_alive() {
        let hwnd = acquire_workerw().unwrap();
        assert!(workerw_is_alive(hwnd));
    }

    #[test]
    fn a_null_handle_is_not_alive() {
        assert!(!workerw_is_alive(HWND(std::ptr::null_mut())));
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod wallpaper_window;` 추가 후,

Run: `cargo test wallpaper_window`
Expected: 컴파일 실패. `cannot find function 'workerw_is_alive'`.

- [ ] **Step 3: 구현 작성**

Task 1 스파이크의 `acquire_workerw` / `find_workerw` / `enum_cb`를 그대로 옮기고, 창 관리를 추가한다.

```rust
//! Owns the WorkerW parent and the layered child windows drawn onto it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::Once;

use anyhow::{anyhow, Result};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, ScreenToClient,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::layout::Rect;
use crate::render::RenderedText;

const CHILD_CLASS: PCWSTR = w!("DesktopCountdownChild");
static REGISTER: Once = Once::new();

pub fn acquire_workerw() -> Result<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), None)?;
        let mut res = 0usize;

        SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
        if let Some(h) = find_workerw() {
            return Ok(h);
        }
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(0x1), SMTO_NORMAL, 1000, Some(&mut res));
        find_workerw().ok_or_else(|| anyhow!("no WorkerW spawned behind the desktop icons"))
    }
}

pub fn workerw_is_alive(hwnd: HWND) -> bool {
    !hwnd.0.is_null() && unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() {
        if let Ok(worker) = FindWindowExW(None, Some(hwnd), w!("WorkerW"), None) {
            *(lparam.0 as *mut HWND) = worker;
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe fn find_workerw() -> Option<HWND> {
    let mut out = HWND(null_mut());
    let _ = EnumWindows(Some(enum_cb), LPARAM(&mut out as *mut HWND as isize));
    (!out.0.is_null()).then_some(out)
}

pub struct CountdownWindow {
    hwnd: HWND,
}

impl CountdownWindow {
    pub fn create(parent: HWND) -> Result<Self> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            REGISTER.call_once(|| {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(DefWindowProcW),
                    hInstance: hinst.into(),
                    lpszClassName: CHILD_CLASS,
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                CHILD_CLASS,
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE,
                0, 0, 1, 1,
                Some(parent),
                None,
                Some(hinst.into()),
                None,
            )?;
            Ok(Self { hwnd })
        }
    }

    /// `rect` is in virtual-desktop screen coordinates; it is converted to the
    /// parent's client space before positioning.
    pub fn update(&self, parent: HWND, rect: Rect, img: &RenderedText, opacity: f32) -> Result<()> {
        unsafe {
            let mut origin = POINT { x: rect.x, y: rect.y };
            ScreenToClient(parent, &mut origin).ok()?;
            SetWindowPos(
                self.hwnd,
                Some(HWND_TOP),
                origin.x, origin.y,
                img.width as i32, img.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )?;

            let hdc_screen = GetDC(None);
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));

            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: img.width as i32,
                    biHeight: -(img.height as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let hbmp = CreateDIBSection(Some(hdc_mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            copy_nonoverlapping(img.pixels.as_ptr(), bits as *mut u8, img.pixels.len());
            let old = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));

            let size = SIZE { cx: img.width as i32, cy: img.height as i32 };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };

            let r = UpdateLayeredWindow(
                self.hwnd,
                Some(hdc_screen),
                None, // SetWindowPos already placed it
                Some(&size),
                Some(hdc_mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );

            SelectObject(hdc_mem, old);
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(None, hdc_screen);
            r?;
            Ok(())
        }
    }
}

impl Drop for CountdownWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test wallpaper_window`
Expected: `a_null_handle_is_not_alive` PASS, 나머지 1개 ignored.

Run: `cargo test wallpaper_window -- --ignored`
Expected: `acquires_workerw_and_reports_it_alive` PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/wallpaper_window.rs src/lib.rs
git commit -m "wallpaper_window: WorkerW 확보와 레이어드 자식 창

- 스크린 좌표를 부모 클라이언트 좌표로 변환해 배치
- UpdateLayeredWindow로 프리멀티플라이드 BGRA 업로드
- opacity는 BLENDFUNCTION의 SourceConstantAlpha로 적용"
```

---

### Task 12: 앱 오케스트레이션 — 컨트롤러 창과 틱 루프

설계 §5.1, §5.2. 처음으로 화면에 카운트다운이 뜬다.

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: 지금까지의 모든 모듈
- Produces:
  - `pub struct App`
  - `pub fn App::run(cfg_path: std::path::PathBuf) -> anyhow::Result<()>` — 메시지 루프를 돌며 블록한다

- [ ] **Step 1: 초 경계 계산 테스트 작성**

`src/app.rs` 하단에. 이것만 순수 함수라 테스트한다.

```rust
#[cfg(test)]
mod tests {
    use super::ms_to_next_second;

    #[test]
    fn just_after_a_boundary_waits_almost_a_full_second() {
        assert_eq!(ms_to_next_second(1_000_000), 999); // 1ms past the boundary
    }

    #[test]
    fn just_before_a_boundary_waits_the_minimum() {
        // 999.9ms past the boundary would round to 0; we clamp to MIN_TIMER_MS.
        assert_eq!(ms_to_next_second(999_900_000), 20);
    }

    #[test]
    fn exactly_on_a_boundary_waits_a_full_second() {
        assert_eq!(ms_to_next_second(0), 1000);
    }

    #[test]
    fn never_exceeds_one_second() {
        for ns in [0, 1, 500_000_000, 999_999_999] {
            assert!(ms_to_next_second(ns) <= 1000);
            assert!(ms_to_next_second(ns) >= 20);
        }
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod app;` 추가 후,

Run: `cargo test app`
Expected: 컴파일 실패. `cannot find function 'ms_to_next_second'`.

- [ ] **Step 3: 구현 작성**

`src/app.rs`:

```rust
//! Ties everything together: a hidden top-level controller window owns the timer,
//! receives system messages, and drives the layered child windows.

use std::path::PathBuf;

use anyhow::Result;
use jiff::Zoned;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::{effective_for, Config};
use crate::countdown::{breakdown, format_main, format_summary};
use crate::layout::place;
use crate::monitors::{self, MonitorInfo};
use crate::render::{Lines, RenderedText, Renderer};
use crate::wallpaper_window::{acquire_workerw, workerw_is_alive, CountdownWindow};

const CTRL_CLASS: PCWSTR = w!("DesktopCountdownController");
const TIMER_ID: usize = 1;
const MIN_TIMER_MS: u32 = 20;

/// Milliseconds until the next whole second, clamped so the timer never fires
/// immediately (which would spin) nor sleeps past a tick.
fn ms_to_next_second(subsec_nanos: u32) -> u32 {
    let remaining_ms = 1000u32.saturating_sub(subsec_nanos / 1_000_000);
    remaining_ms.clamp(MIN_TIMER_MS, 1000)
}

struct Surface {
    monitor: MonitorInfo,
    window: CountdownWindow,
}

pub struct App {
    cfg_path: PathBuf,
    cfg: Config,
    target: Zoned,
    renderer: Renderer,
    workerw: HWND,
    surfaces: Vec<Surface>,
    last_lines: Option<Lines>,
    ticks_since_health_check: u32,
}

impl App {
    pub fn run(cfg_path: PathBuf) -> Result<()> {
        let cfg = crate::config::load_or_create(&cfg_path)?;
        let target = cfg.target.to_zoned(jiff::tz::TimeZone::system())?;

        let mut app = App {
            cfg_path,
            cfg,
            target,
            renderer: Renderer::new()?,
            workerw: acquire_workerw()?,
            surfaces: Vec::new(),
            last_lines: None,
            ticks_since_health_check: 0,
        };
        app.rebuild_surfaces()?;

        let hwnd = create_controller_window(&mut app)?;
        unsafe { SetTimer(Some(hwnd), TIMER_ID, 100, None) };

        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Ok(())
    }

    fn rebuild_surfaces(&mut self) -> Result<()> {
        self.surfaces.clear();
        self.last_lines = None;

        for m in monitors::enumerate()? {
            let eff = effective_for(&self.cfg, &m.id);
            if !eff.enabled {
                tracing::info!(monitor = %m.name, "disabled by config");
                continue;
            }
            let window = CountdownWindow::create(self.workerw)?;
            self.surfaces.push(Surface { monitor: m, window });
        }
        tracing::info!(count = self.surfaces.len(), "surfaces built");
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        self.ticks_since_health_check += 1;
        if self.ticks_since_health_check >= 2 {
            self.ticks_since_health_check = 0;
            if !workerw_is_alive(self.workerw) {
                tracing::warn!("WorkerW vanished (Explorer restart?), reattaching");
                self.workerw = acquire_workerw()?;
                self.rebuild_surfaces()?;
            }
        }

        let now = Zoned::now();
        let b = breakdown(&now, &self.target);
        let lines = Lines {
            summary: Some(format_summary(&b)),
            main: format_main(&b),
        };

        if self.last_lines.as_ref() == Some(&lines) {
            return Ok(());
        }

        for s in &self.surfaces {
            let eff = effective_for(&self.cfg, &s.monitor.id);
            let img: RenderedText = self.renderer.render(&lines, &eff.style)?;
            let rect = place(
                s.monitor.rect,
                img.width as i32,
                img.height as i32,
                eff.anchor,
                eff.offset_px,
            );
            s.window.update(self.workerw, rect, &img, eff.style.opacity)?;
        }
        self.last_lines = Some(lines);
        Ok(())
    }

    fn on_display_change(&mut self) {
        tracing::info!("display configuration changed");
        if let Err(e) = self.rebuild_surfaces() {
            tracing::error!("rebuilding surfaces failed: {e:#}");
        }
    }
}

fn create_controller_window(app: &mut App) -> Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: CTRL_CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CTRL_CLASS,
            w!("DesktopCountdown"),
            WS_POPUP, // never shown
            0, 0, 0, 0,
            None, None, Some(hinst.into()), None,
        )?;

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, app as *mut App as isize);
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let app = &mut *ptr;

    match msg {
        WM_TIMER => {
            if let Err(e) = app.tick() {
                tracing::error!("tick failed: {e:#}");
            }
            let next = ms_to_next_second(Zoned::now().subsec_nanosecond() as u32);
            SetTimer(Some(hwnd), TIMER_ID, next, None);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            app.on_display_change();
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
```

`src/main.rs`의 `main`을 아래로 바꾼다.

```rust
fn main() -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)? };

    let _guard = logging::init(&paths::log_dir()?);
    let _instance = SingleInstance::acquire()?;

    let cfg_path = paths::config_path()?;
    tracing::info!(?cfg_path, "starting");

    if let Err(e) = desktop_countdown::app::App::run(cfg_path) {
        tracing::error!("fatal: {e:#}");
        return Err(e);
    }
    Ok(())
}
```

`src/main.rs`의 `use`에서 `config`를 지운다(더 이상 직접 쓰지 않는다).

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test app`
Expected: 4개 PASS.

- [ ] **Step 5: 실제로 화면에 뜨는지 확인**

`%APPDATA%\DesktopCountdown\config.toml`의 `target`을 지금으로부터 며칠 뒤로 바꾼다.

Run: `cargo run`

`Win+D`로 바탕화면을 보면 4개 모니터 각각의 가운데에 카운트다운이 뜨고, 초가 1초마다 갱신되어야 한다. 종료는 `Ctrl+C`.

**확인 항목:**
- 초가 건너뛰지 않고 1씩 줄어든다.
- 바탕화면 아이콘이 글자 위에 그려진다.
- 창을 띄우면 가려진다.
- 세로 모니터에서도 가운데 정렬이 맞다.

- [ ] **Step 6: 커밋**

```bash
git add src/app.rs src/main.rs src/lib.rs
git commit -m "app: 컨트롤러 창과 틱 루프

- 다음 초 경계에 맞춘 타이머로 드리프트 방지
- 문자열이 같으면 렌더 스킵
- WorkerW 소실 감지 후 재부착, WM_DISPLAYCHANGE 시 창 재생성"
```

---

### Task 13: 설정 파일 감시와 핫 리로드

설계 §5.3, §6. 저장 즉시 반영. 잘못된 TOML이면 이전 설정을 유지한다.

**Files:**
- Create: `src/watch.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `notify`, `crate::paths`
- Produces:
  - `pub struct ConfigWatcher`
  - `pub fn ConfigWatcher::new(path: &std::path::Path) -> anyhow::Result<ConfigWatcher>`
  - `pub fn ConfigWatcher::changed(&mut self) -> bool` — 디바운스를 통과한 변경이 있으면 `true`를 한 번 돌려준다

- [ ] **Step 1: 디바운스 테스트 작성**

`src/watch.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn pending_change_is_not_reported_before_the_debounce_elapses() {
        let now = Instant::now();
        assert!(!should_fire(Some(now), now, DEBOUNCE));
    }

    #[test]
    fn pending_change_fires_after_the_debounce() {
        let now = Instant::now();
        let later = now + DEBOUNCE + Duration::from_millis(1);
        assert!(should_fire(Some(now), later, DEBOUNCE));
    }

    #[test]
    fn no_pending_change_never_fires() {
        assert!(!should_fire(None, Instant::now(), DEBOUNCE));
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod watch;` 추가 후,

Run: `cargo test watch`
Expected: 컴파일 실패. `cannot find function 'should_fire'`.

- [ ] **Step 3: 구현 작성**

```rust
//! Watches `config.toml` and reports debounced changes.

use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub const DEBOUNCE: Duration = Duration::from_millis(200);

/// Editors often write a file in several steps; wait for the writes to settle.
fn should_fire(pending_since: Option<Instant>, now: Instant, debounce: Duration) -> bool {
    match pending_since {
        Some(t) => now.duration_since(t) >= debounce,
        None => false,
    }
}

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<()>,
    pending_since: Option<Instant>,
}

impl ConfigWatcher {
    pub fn new(path: &Path) -> Result<Self> {
        let (tx, rx) = channel();
        let file_name = path.file_name().map(|s| s.to_owned());

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let touches_config = event.paths.iter().any(|p| p.file_name().map(|s| s.to_owned()) == file_name);
            if touches_config {
                let _ = tx.send(());
            }
        })?;

        // Watch the directory, not the file: editors replace the inode on save.
        let dir = path.parent().unwrap_or(Path::new("."));
        watcher.watch(dir, RecursiveMode::NonRecursive)?;

        Ok(Self { _watcher: watcher, rx, pending_since: None })
    }

    /// Call this every tick. Returns `true` exactly once per settled change.
    pub fn changed(&mut self) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(()) => self.pending_since = Some(Instant::now()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if should_fire(self.pending_since, Instant::now(), DEBOUNCE) {
            self.pending_since = None;
            return true;
        }
        false
    }
}
```

- [ ] **Step 4: App에 연결**

`App` 구조체에 `watcher: ConfigWatcher` 필드를 추가하고, `reload()` 메서드를 추가한다.

```rust
    fn reload(&mut self) {
        match crate::config::load_or_create(&self.cfg_path) {
            Ok(new_cfg) => {
                let displays_changed = new_cfg.displays != self.cfg.displays
                    || new_cfg.layout != self.cfg.layout;
                let target_changed = new_cfg.target != self.cfg.target;

                self.cfg = new_cfg;
                if target_changed {
                    match self.cfg.target.to_zoned(jiff::tz::TimeZone::system()) {
                        Ok(z) => self.target = z,
                        Err(e) => tracing::error!("bad target: {e:#}"),
                    }
                }
                if displays_changed {
                    if let Err(e) = self.rebuild_surfaces() {
                        tracing::error!("rebuilding surfaces failed: {e:#}");
                    }
                }
                self.last_lines = None; // force a redraw with the new style
                tracing::info!("config reloaded");
            }
            // Keeping the last valid config beats blanking the screen.
            Err(e) => tracing::error!("config reload rejected, keeping previous: {e:#}"),
        }
    }
```

`tick()`의 맨 앞에 추가:

```rust
        if self.watcher.changed() {
            self.reload();
        }
```

`App::run`의 앞부분을 아래로 바꾼다. `ConfigWatcher::new`는 `cfg_path`를 빌리므로 `App`을 짓기 전에 만든다.

```rust
    pub fn run(cfg_path: PathBuf) -> Result<()> {
        let cfg = crate::config::load_or_create(&cfg_path)?;
        let target = cfg.target.to_zoned(jiff::tz::TimeZone::system())?;
        let watcher = ConfigWatcher::new(&cfg_path)?;

        let mut app = App {
            cfg_path,
            cfg,
            target,
            watcher,
            renderer: Renderer::new()?,
            workerw: acquire_workerw()?,
            surfaces: Vec::new(),
            last_lines: None,
            ticks_since_health_check: 0,
        };
        app.rebuild_surfaces()?;
        // ... 이하 동일
```

`use crate::watch::ConfigWatcher;`를 추가한다.

`tick()`은 초 경계에서만 불리므로 디바운스 해상도가 1초다. 저장 후 최대 1.2초 뒤에 반영된다. 그 정도면 충분하다.

- [ ] **Step 5: 테스트와 수동 확인**

Run: `cargo test watch`
Expected: 3개 PASS.

Run: `cargo run` 후 `config.toml`에서 `size_px`를 `120.0`으로 바꿔 저장.
Expected: 1초쯤 뒤 글자가 커진다. `color`를 `"#FF0000"`으로 바꾸면 빨개진다.

`opacity = 3.0`으로 저장.
Expected: 화면이 그대로 유지되고, `log.txt`에 `config reload rejected, keeping previous`가 남는다.

- [ ] **Step 6: 커밋**

```bash
git add src/watch.rs src/app.rs src/lib.rs
git commit -m "설정 파일 감시와 핫 리로드

- 디렉터리를 감시해 에디터의 파일 교체를 잡는다
- 200ms 디바운스
- 파싱/검증 실패 시 이전 설정 유지"
```

---

### Task 14: 트레이 아이콘

설계 §3.2, §6. 종료할 수단을 준다.

**Files:**
- Create: `src/tray.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `tray-icon`, `crate::paths::config_path`
- Produces:
  - `pub enum TrayCommand { OpenConfig, Reload, Quit }`
  - `pub struct Tray`
  - `pub fn Tray::new() -> anyhow::Result<Tray>`
  - `pub fn Tray::poll(&self) -> Option<TrayCommand>`
  - `pub fn Tray::set_warning(&self, on: bool) -> anyhow::Result<()>` — 툴팁에 경고 표시

- [ ] **Step 1: 아이콘 생성 테스트 작성**

`src/tray.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_pixels_have_the_expected_length() {
        let px = icon_rgba(ICON_SIZE);
        assert_eq!(px.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn icon_centre_is_opaque_and_corners_are_transparent() {
        let s = ICON_SIZE;
        let px = icon_rgba(s);
        let at = |x: u32, y: u32| px[((y * s + x) * 4 + 3) as usize];
        assert_eq!(at(0, 0), 0, "corner should be transparent");
        assert_eq!(at(s - 1, s - 1), 0, "corner should be transparent");
        assert!(at(s / 2, s / 2) > 0, "centre should be drawn");
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod tray;` 추가 후,

Run: `cargo test tray`
Expected: 컴파일 실패. `cannot find function 'icon_rgba'`.

- [ ] **Step 3: 구현 작성**

```rust
//! System tray icon and menu. The only way to quit a wallpaper-layer app.

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const ICON_SIZE: u32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenConfig,
    Reload,
    Quit,
}

/// A filled disc, so we do not need to ship a binary .ico asset.
fn icon_rgba(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = (size as f32 - 1.0) / 2.0;
    let r = c - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            if (dx * dx + dy * dy).sqrt() <= r {
                let i = ((y * size + x) * 4) as usize;
                px[i] = 0xE8;
                px[i + 1] = 0xEE;
                px[i + 2] = 0xF7;
                px[i + 3] = 0xFF;
            }
        }
    }
    px
}

pub struct Tray {
    icon: TrayIcon,
    open_id: MenuId,
    reload_id: MenuId,
    quit_id: MenuId,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let open = MenuItem::new("설정 파일 열기", true, None);
        let reload = MenuItem::new("다시 불러오기", true, None);
        let quit = MenuItem::new("종료", true, None);

        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &reload, &quit])?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("DesktopCountdown")
            .with_icon(Icon::from_rgba(icon_rgba(ICON_SIZE), ICON_SIZE, ICON_SIZE)?)
            .build()?;

        Ok(Self {
            icon,
            open_id: open.id().clone(),
            reload_id: reload.id().clone(),
            quit_id: quit.id().clone(),
        })
    }

    pub fn poll(&self) -> Option<TrayCommand> {
        let ev = MenuEvent::receiver().try_recv().ok()?;
        if ev.id == self.open_id {
            Some(TrayCommand::OpenConfig)
        } else if ev.id == self.reload_id {
            Some(TrayCommand::Reload)
        } else if ev.id == self.quit_id {
            Some(TrayCommand::Quit)
        } else {
            None
        }
    }

    pub fn set_warning(&self, on: bool) -> Result<()> {
        let tip = if on {
            "DesktopCountdown — 설정 오류 (log.txt 확인)"
        } else {
            "DesktopCountdown"
        };
        self.icon.set_tooltip(Some(tip))?;
        Ok(())
    }
}
```

- [ ] **Step 4: App에 연결**

`App` 구조체에 `tray: Tray` 필드를 추가하고(`App::run`에서 `tray: Tray::new()?`), `tick()`의
`self.watcher.changed()` 처리 **바로 앞에** 트레이 명령 처리를 넣는다.

```rust
        match self.tray.poll() {
            Some(TrayCommand::Quit) => {
                unsafe { PostQuitMessage(0) };
                return Ok(());
            }
            Some(TrayCommand::Reload) => self.reload(),
            Some(TrayCommand::OpenConfig) => {
                if let Err(e) = std::process::Command::new("notepad.exe").arg(&self.cfg_path).spawn() {
                    tracing::error!("opening the config failed: {e:#}");
                }
            }
            None => {}
        }
```

`use crate::tray::{Tray, TrayCommand};`를 추가한다. `PostQuitMessage`는 이미
`windows::Win32::UI::WindowsAndMessaging::*`로 들어와 있다.

`reload()`의 두 갈래에서 툴팁 경고를 갱신한다. `Ok` 갈래의 `tracing::info!("config reloaded");`
바로 앞에 다음을 넣는다.

```rust
                let _ = self.tray.set_warning(false);
```

`Err` 갈래를 아래로 바꾼다.

```rust
            // Keeping the last valid config beats blanking the screen.
            Err(e) => {
                tracing::error!("config reload rejected, keeping previous: {e:#}");
                let _ = self.tray.set_warning(true);
            }
```

- [ ] **Step 5: 테스트와 수동 확인**

Run: `cargo test tray`
Expected: 2개 PASS.

Run: `cargo run`
Expected: 트레이에 흰 원 아이콘이 뜬다. 우클릭 → "설정 파일 열기"로 메모장이 열리고, "종료"로 앱이 끝난다. `opacity = 3.0`을 저장하면 툴팁이 경고로 바뀐다.

- [ ] **Step 6: 커밋**

```bash
git add src/tray.rs src/app.rs src/lib.rs
git commit -m "트레이 아이콘과 메뉴

- 설정 파일 열기 / 다시 불러오기 / 종료
- 아이콘은 코드로 생성(바이너리 에셋 없음)
- 설정 오류 시 툴팁에 경고 표시"
```

---

### Task 15: 복원력 — WorkerW 백오프와 렌더러 재생성

설계 §6. 지금까지는 WorkerW를 못 찾으면 앱이 그냥 죽고, 렌더 실패도 매 틱 반복된다.

**Files:**
- Create: `src/backoff.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: 없음
- Produces:
  - `pub struct Backoff { .. }`
  - `pub fn Backoff::new(base_ms: u64, cap_ms: u64, give_up_after_ms: u64) -> Backoff`
  - `pub fn Backoff::next_delay_ms(&mut self) -> Option<u64>` — 포기 시점을 넘으면 `None`
  - `pub fn Backoff::reset(&mut self)`

- [ ] **Step 1: 실패하는 테스트 작성**

`src/backoff.rs` 하단에:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_until_the_cap() {
        let mut b = Backoff::new(500, 4_000, 60_000);
        assert_eq!(b.next_delay_ms(), Some(500));
        assert_eq!(b.next_delay_ms(), Some(1_000));
        assert_eq!(b.next_delay_ms(), Some(2_000));
        assert_eq!(b.next_delay_ms(), Some(4_000));
        assert_eq!(b.next_delay_ms(), Some(4_000));
    }

    #[test]
    fn gives_up_once_the_total_wait_exceeds_the_budget() {
        let mut b = Backoff::new(1_000, 1_000, 2_500);
        assert_eq!(b.next_delay_ms(), Some(1_000)); // total 1000
        assert_eq!(b.next_delay_ms(), Some(1_000)); // total 2000
        assert_eq!(b.next_delay_ms(), None); // would exceed 2500
    }

    #[test]
    fn reset_starts_over() {
        let mut b = Backoff::new(500, 4_000, 60_000);
        b.next_delay_ms();
        b.next_delay_ms();
        b.reset();
        assert_eq!(b.next_delay_ms(), Some(500));
    }

    #[test]
    fn a_reset_backoff_can_give_up_again() {
        let mut b = Backoff::new(1_000, 1_000, 1_000);
        assert_eq!(b.next_delay_ms(), Some(1_000));
        assert_eq!(b.next_delay_ms(), None);
        b.reset();
        assert_eq!(b.next_delay_ms(), Some(1_000));
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod backoff;` 추가 후,

Run: `cargo test backoff`
Expected: 컴파일 실패. `cannot find type 'Backoff'`.

- [ ] **Step 3: 최소 구현 작성**

```rust
//! Exponential backoff with a total-wait budget. No Win32, no I/O, no clock.

#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    cap_ms: u64,
    give_up_after_ms: u64,
    next_ms: u64,
    elapsed_ms: u64,
}

impl Backoff {
    pub fn new(base_ms: u64, cap_ms: u64, give_up_after_ms: u64) -> Self {
        Self { base_ms, cap_ms, give_up_after_ms, next_ms: base_ms, elapsed_ms: 0 }
    }

    /// `None` means the caller should stop retrying.
    pub fn next_delay_ms(&mut self) -> Option<u64> {
        let delay = self.next_ms.min(self.cap_ms);
        if self.elapsed_ms + delay > self.give_up_after_ms {
            return None;
        }
        self.elapsed_ms += delay;
        self.next_ms = (self.next_ms * 2).min(self.cap_ms);
        Some(delay)
    }

    pub fn reset(&mut self) {
        self.next_ms = self.base_ms;
        self.elapsed_ms = 0;
    }
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test backoff`
Expected: 4개 PASS.

- [ ] **Step 5: App에 연결**

세 곳을 고친다.

**(1) 시작 시 WorkerW를 못 찾아도 죽지 않는다.** `App::run`의 `workerw: acquire_workerw()?`를
`workerw: HWND(std::ptr::null_mut())`로 바꾸고, `rebuild_surfaces()` 호출을 지운다. 첫 `tick()`이
확보를 시도한다. `App`에 `workerw_backoff: Backoff::new(500, 8_000, 60_000)`와
`retry_at: Option<std::time::Instant>` 필드를 추가한다.

**(2) `tick()`의 헬스 체크를 백오프로 감싼다.** 기존 헬스 체크 블록을 아래로 교체한다.

```rust
        if !workerw_is_alive(self.workerw) {
            if let Some(at) = self.retry_at {
                if std::time::Instant::now() < at {
                    return Ok(()); // still waiting out the backoff
                }
            }
            match acquire_workerw() {
                Ok(hwnd) => {
                    tracing::info!("attached to WorkerW");
                    self.workerw = hwnd;
                    self.workerw_backoff.reset();
                    self.retry_at = None;
                    self.rebuild_surfaces()?;
                    let _ = self.tray.set_warning(false);
                }
                Err(e) => {
                    let _ = self.tray.set_warning(true);
                    return match self.workerw_backoff.next_delay_ms() {
                        Some(ms) => {
                            tracing::warn!("WorkerW not available ({e:#}), retrying in {ms}ms");
                            self.retry_at =
                                Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                            Ok(())
                        }
                        None => {
                            tracing::error!("giving up on WorkerW after the retry budget: {e:#}");
                            Ok(()) // keep the tray alive so the user can quit
                        }
                    };
                }
            }
        }
```

`ticks_since_health_check` 필드와 그 로직은 지운다. 매 틱 `IsWindow`를 부르는 비용은 무시할 수준이고,
Explorer 재시작을 최대 1초 안에 복구하게 된다.

**(3) 렌더 실패 시 렌더러를 한 번 재생성한다.** `tick()`의 그리기 루프를 아래로 감싼다.

```rust
        if let Err(e) = self.draw(&lines) {
            tracing::warn!("draw failed, recreating the renderer: {e:#}");
            self.renderer = Renderer::new()?;
            self.draw(&lines)?; // one retry; a second failure propagates
        }
        self.last_lines = Some(lines);
```

`draw`는 기존 루프를 그대로 옮긴 메서드다.

```rust
    fn draw(&self, lines: &Lines) -> Result<()> {
        for s in &self.surfaces {
            let eff = effective_for(&self.cfg, &s.monitor.id);
            let img: RenderedText = self.renderer.render(lines, &eff.style)?;
            let rect = place(
                s.monitor.rect,
                img.width as i32,
                img.height as i32,
                eff.anchor,
                eff.offset_px,
            );
            s.window.update(self.workerw, rect, &img, eff.style.opacity)?;
        }
        Ok(())
    }
```

`tick()`이 `self.draw(&lines)?`를 부른 뒤 `Err`를 리턴하면 `wndproc`이 로그만 남기고 다음 틱을
계속 돌린다. 앱은 죽지 않는다.

- [ ] **Step 6: 수동 확인**

Run: `cargo run`

작업 관리자에서 `Windows 탐색기`를 **다시 시작**한다.
Expected: 카운트다운이 잠깐 사라졌다가 1~2초 안에 다시 나타난다. `log.txt`에
`WorkerW vanished` 또는 `attached to WorkerW`가 남는다.

- [ ] **Step 7: 커밋**

```bash
git add src/backoff.rs src/app.rs src/lib.rs
git commit -m "복원력: WorkerW 지수 백오프와 렌더러 재생성

- 시작 시 WorkerW가 없어도 앱이 죽지 않고 재시도
- 재시도 예산(60초) 소진 시 트레이만 남겨 종료 가능하게 유지
- 렌더 실패 시 렌더러를 한 번 재생성 후 재시도"
```

---

### Task 16: 자동 시작 등록

설계 §4, §3.2. `[general] autostart`를 레지스트리에 반영한다.

**Files:**
- Create: `src/autostart.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `windows::Win32::System::Registry`
- Produces:
  - `pub fn is_enabled() -> anyhow::Result<bool>`
  - `pub fn set_enabled(on: bool) -> anyhow::Result<()>`

- [ ] **Step 1: 왕복 테스트 작성**

`src/autostart.rs` 하단에. `HKCU`에 쓰므로 관리자 권한이 필요 없다. 테스트는 원래 값을 복원한다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_then_disable_round_trips() {
        let original = is_enabled().unwrap();

        set_enabled(true).unwrap();
        assert!(is_enabled().unwrap());

        set_enabled(false).unwrap();
        assert!(!is_enabled().unwrap());

        set_enabled(original).unwrap();
        assert_eq!(is_enabled().unwrap(), original);
    }

    #[test]
    fn disabling_when_absent_is_not_an_error() {
        let original = is_enabled().unwrap();
        set_enabled(false).unwrap();
        assert!(set_enabled(false).is_ok());
        set_enabled(original).unwrap();
    }
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

`src/lib.rs`에 `pub mod autostart;` 추가 후,

Run: `cargo test autostart -- --test-threads=1`
Expected: 컴파일 실패. `cannot find function 'is_enabled'`.

`--test-threads=1`이 필수다. 두 테스트가 같은 레지스트리 값을 건드린다.

- [ ] **Step 3: 구현 작성**

```rust
//! Registers the executable under HKCU\...\Run so it survives a reboot.

use anyhow::{Context, Result};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE_NAME: PCWSTR = w!("DesktopCountdown");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), access, &mut key).ok()? };
    Ok(key)
}

pub fn is_enabled() -> Result<bool> {
    let key = open(KEY_READ)?;
    let mut size = 0u32;
    let status = unsafe { RegQueryValueExW(key, VALUE_NAME, None, None, None, Some(&mut size)) };
    unsafe { let _ = RegCloseKey(key); }
    Ok(status.is_ok())
}

pub fn set_enabled(on: bool) -> Result<()> {
    let key = open(KEY_WRITE | KEY_READ)?;
    let result = if on {
        let exe = std::env::current_exe().context("current_exe")?;
        let quoted = format!("\"{}\"", exe.display());
        // REG_SZ data must include the terminating NUL, and `as_wide()` omits it.
        let mut wide: Vec<u16> = HSTRING::from(quoted).as_wide().to_vec();
        wide.push(0);
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        unsafe { RegSetValueExW(key, VALUE_NAME, Some(0), REG_SZ, Some(bytes)) }.ok()
    } else {
        let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
        // Deleting an absent value is success from the caller's point of view.
        if status == ERROR_FILE_NOT_FOUND { Ok(()) } else { status.ok() }
    };
    unsafe { let _ = RegCloseKey(key); }
    result.context("updating the Run key")?;
    Ok(())
}
```

- [ ] **Step 4: App에 연결**

`App::run`의 설정 로드 직후와 `reload()` 끝에서:

```rust
        if let Err(e) = crate::autostart::set_enabled(self.cfg.general.autostart) {
            tracing::error!("autostart update failed: {e:#}");
        }
```

- [ ] **Step 5: 테스트와 수동 확인**

Run: `cargo test autostart -- --test-threads=1`
Expected: 2개 PASS.

`config.toml`에 `[general]\nautostart = true`를 넣고 `cargo run` 후, `regedit`에서 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`에 `DesktopCountdown` 값이 생겼는지 확인한다. `false`로 바꾸면 사라진다.

- [ ] **Step 6: 커밋**

```bash
git add src/autostart.rs src/app.rs src/lib.rs
git commit -m "자동 시작 레지스트리 등록

- HKCU Run 키에 실행 파일 경로 등록/해제
- 설정 로드와 리로드 시 반영"
```

---

### Task 17: 릴리스 빌드와 README

**Files:**
- Modify: `Cargo.toml`
- Create: `README.md`

**Interfaces:**
- Consumes: 없음
- Produces: 없음

- [ ] **Step 1: 릴리스 프로파일 추가**

`Cargo.toml` 끝에:

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

- [ ] **Step 2: 릴리스 빌드 확인**

Run: `cargo build --release`
Expected: 성공. `target\release\desktop-countdown.exe`가 생긴다.

Run: `target\release\desktop-countdown.exe`
Expected: 콘솔 창이 뜨지 않고, 트레이 아이콘만 나타나며, 바탕화면에 카운트다운이 뜬다.

- [ ] **Step 3: README 작성**

`README.md`:

````markdown
# DesktopCountdown

바탕화면 배경 레이어에 마감까지 남은 시간을 표시하는 Windows 앱.

```
3m 2w 0d
2544:18:07
```

아랫줄이 남은 총 시간, 윗줄은 개월/주/일 요약입니다. 데스크톱 아이콘 아래에 그려지므로
다른 창을 가리지 않습니다.

## 설치

```
cargo build --release
```

`target\release\desktop-countdown.exe`를 실행하면 트레이 아이콘이 나타납니다.

## 설정

`%APPDATA%\DesktopCountdown\config.toml`을 편집하면 **저장 즉시** 화면에 반영됩니다.
트레이 아이콘 우클릭 → "설정 파일 열기"로 메모장에서 열 수 있습니다.

주요 항목은 `target`(마감 시각), `[style]`의 `font_family`·`size_px`·`color`·`mode`,
`[layout]`의 `anchor`·`offset_px`입니다. 모니터별로 다르게 하려면 `[[display]]` 블록을
추가합니다. 전체 스키마는 설계 문서 §4를 보세요.

설정이 잘못되면 이전 설정을 유지하고 트레이 툴팁에 경고를 띄웁니다.
이유는 `%LOCALAPPDATA%\DesktopCountdown\log.txt`에 남습니다.

## 알려진 제약

- 바탕화면 배경 레이어에 그리므로 마우스로 만질 수 없습니다. 모든 조작은 트레이와 설정 파일로 합니다.
- 배경 레이어 부착은 Windows의 비공식 동작(`Progman`에 `0x052C` 전송)에 기댑니다.
  Explorer가 재시작되면 자동으로 다시 붙지만, Windows 업데이트로 깨질 가능성이 있습니다.
- 목표 시각에 도달하면 `00:00:00`에서 멈춥니다. 알림은 띄우지 않습니다.

## 문서

- 설계: `docs/superpowers/specs/2026-07-10-desktop-countdown-design.md`
- 구현 계획: `docs/superpowers/plans/`
````

- [ ] **Step 4: 전체 테스트 재확인**

Run: `cargo test -- --test-threads=1`
Expected: 전부 PASS (ignored 1개 제외).

Run: `cargo test -- --ignored`
Expected: `acquires_workerw_and_reports_it_alive` PASS.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 경고 없음. 안 쓰는 `use`가 남아 있으면 여기서 지운다.

- [ ] **Step 5: 커밋**

```bash
git add Cargo.toml README.md
git commit -m "릴리스 프로파일과 README"
```

---

## 완료 조건

- `cargo test -- --test-threads=1`이 전부 통과한다.
- `cargo clippy --all-targets -- -D warnings`가 깨끗하다.
- 릴리스 실행 파일이 콘솔 없이 뜨고, 4개 모니터에 카운트다운이 그려진다.
- `config.toml`을 저장하면 1초 안에 반영되고, 깨진 설정은 화면을 건드리지 않는다.
- 트레이에서 종료할 수 있다.

## 다음 계획

계획 2는 `desktop-countdown.exe --settings`로 뜨는 egui 설정 창을 만든다. 폰트 목록은
DirectWrite의 `GetSystemFontCollection`으로 열거하고, 모니터 목록은 `monitors::enumerate()`를
재사용한다. 설정 창은 `config.toml`을 저장할 뿐이며, 렌더러는 이미 그 파일을 감시하고 있으므로
새 IPC는 필요 없다.
