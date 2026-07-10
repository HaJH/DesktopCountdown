//! Throwaway diagnostic: inspect the live spike window's real state.
//! Run while spike_layered is still running. Not part of the product.

use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, FindWindowExW, FindWindowW, GetClassNameW, GetParent, GetWindow,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, GW_CHILD,
    GW_HWNDNEXT,
};

const WS_CHILD: isize = 0x4000_0000;
const WS_POPUP: isize = 0x8000_0000u32 as isize;
const WS_VISIBLE: isize = 0x1000_0000;
const WS_DISABLED: isize = 0x0800_0000;
const WS_EX_LAYERED: isize = 0x0008_0000;
const WS_EX_TRANSPARENT: isize = 0x0000_0020;
const WS_EX_NOACTIVATE: isize = 0x0800_0000;
const WS_EX_TOOLWINDOW: isize = 0x0000_0080;

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn rect_of(hwnd: HWND) -> String {
    let mut r = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
        format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom)
    } else {
        "<none>".into()
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let v = &mut *(lparam.0 as *mut Vec<HWND>);
    v.push(hwnd);
    BOOL(1)
}

fn describe(label: &str, hwnd: HWND) {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let mut s = Vec::new();
    if style & WS_CHILD != 0 { s.push("WS_CHILD"); }
    if style & WS_POPUP != 0 { s.push("WS_POPUP"); }
    if style & WS_VISIBLE != 0 { s.push("WS_VISIBLE"); }
    if style & WS_DISABLED != 0 { s.push("WS_DISABLED"); }
    let mut e = Vec::new();
    if ex & WS_EX_LAYERED != 0 { e.push("LAYERED"); }
    if ex & WS_EX_TRANSPARENT != 0 { e.push("TRANSPARENT"); }
    if ex & WS_EX_NOACTIVATE != 0 { e.push("NOACTIVATE"); }
    if ex & WS_EX_TOOLWINDOW != 0 { e.push("TOOLWINDOW"); }

    println!(
        "{label}: hwnd={:?} class={} rect={} IsWindowVisible={} \n    style=0x{style:x} [{}] \n    exstyle=0x{ex:x} [{}]",
        hwnd.0,
        class_of(hwnd),
        rect_of(hwnd),
        unsafe { IsWindowVisible(hwnd) }.as_bool(),
        s.join("|"),
        e.join("|"),
    );
}

fn main() {
    let Ok(progman) = (unsafe { FindWindowW(w!("Progman"), None) }) else {
        println!("no Progman");
        return;
    };
    describe("Progman", progman);

    match unsafe { FindWindowExW(Some(progman), None, w!("WorkerW"), None) } {
        Ok(workerw) => {
            describe("Progman>WorkerW", workerw);
            println!("  z-order of its children:");
            let mut h = unsafe { GetWindow(workerw, GW_CHILD) }.ok();
            let mut i = 0;
            while let Some(cur) = h {
                println!("    [z{i}] {:?} {} visible={} rect={}", cur.0, class_of(cur),
                    unsafe { IsWindowVisible(cur) }.as_bool(), rect_of(cur));
                h = unsafe { GetWindow(cur, GW_HWNDNEXT) }.ok();
                i += 1;
            }
        }
        Err(_) => println!("Progman>WorkerW: GONE"),
    }

    // Find our spike window wherever it now lives.
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(Some(progman), Some(collect), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    let mine: Vec<HWND> = all.into_iter().filter(|h| class_of(*h) == "SpikeLayeredChild").collect();
    if mine.is_empty() {
        println!("\nSpikeLayeredChild: NOT FOUND under Progman");
    }
    for h in mine {
        println!();
        describe("SpikeLayeredChild", h);
        let mut p = unsafe { GetParent(h) }.ok();
        let mut depth = 1;
        while let Some(cur) = p {
            describe(&format!("  ancestor[{depth}]"), cur);
            p = unsafe { GetParent(cur) }.ok();
            depth += 1;
        }
    }
}
