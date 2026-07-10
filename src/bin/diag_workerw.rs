//! Throwaway diagnostic: dump the desktop window tree before and after the
//! undocumented 0x052C message, so we can see what Explorer actually creates.
//! Not part of the product. Delete once the WorkerW question is settled.

use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindow,
    GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SendMessageTimeoutW,
    GW_CHILD, GW_HWNDNEXT, SMTO_NORMAL,
};

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let v = &mut *(lparam.0 as *mut Vec<HWND>);
    v.push(hwnd);
    BOOL(1)
}

fn top_levels() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

fn children(parent: HWND) -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(
            Some(parent),
            Some(collect),
            LPARAM(&mut v as *mut Vec<HWND> as isize),
        );
    }
    v
}

fn has_defview(hwnd: HWND) -> bool {
    unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None).is_ok() }
}

fn rect_of(hwnd: HWND) -> String {
    let mut r = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
        format!(
            "({},{})-({},{}) {}x{}",
            r.left,
            r.top,
            r.right,
            r.bottom,
            r.right - r.left,
            r.bottom - r.top
        )
    } else {
        "<no rect>".into()
    }
}

/// Prints only the windows that matter, plus their position in enumeration order.
fn dump(label: &str) {
    println!("\n===== {label} =====");
    let tops = top_levels();
    println!("total top-level windows: {}", tops.len());
    for (i, hwnd) in tops.iter().enumerate() {
        let c = class_of(*hwnd);
        if c != "Progman" && c != "WorkerW" {
            continue;
        }
        let visible = unsafe { IsWindowVisible(*hwnd) }.as_bool();
        println!(
            "  [{i:3}] {c:<10} hwnd={:?} visible={visible} defview_child={} rect={} title={:?}",
            hwnd.0,
            has_defview(*hwnd),
            rect_of(*hwnd),
            title_of(*hwnd)
        );
        // What is the very next top-level window? That's what the classic trick grabs.
        if let Some(next) = tops.get(i + 1) {
            println!("         next sibling: {} hwnd={:?}", class_of(*next), next.0);
        }
        // Direct children.
        let kids = children(*hwnd);
        let kid_classes: Vec<String> = kids.iter().map(|k| class_of(*k)).collect();
        println!("         descendants ({}): {:?}", kids.len(), kid_classes);
    }

    // The two lookups the product code might use.
    unsafe {
        if let Ok(progman) = FindWindowW(w!("Progman"), None) {
            match FindWindowExW(Some(progman), None, w!("WorkerW"), None) {
                Ok(h) => println!("  WorkerW as CHILD of Progman: {:?} rect={}", h.0, rect_of(h)),
                Err(_) => println!("  WorkerW as CHILD of Progman: none"),
            }
            match FindWindowExW(None, Some(progman), w!("WorkerW"), None) {
                Ok(h) => println!("  WorkerW as SIBLING after Progman: {:?} rect={}", h.0, rect_of(h)),
                Err(_) => println!("  WorkerW as SIBLING after Progman: none"),
            }
        }
    }
}

fn send(progman: HWND, wp: usize, lp: isize) {
    let mut res = 0usize;
    unsafe {
        SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(wp),
            LPARAM(lp),
            SMTO_NORMAL,
            1000,
            Some(&mut res),
        );
    }
    println!("\n>>> sent 0x052C wparam={wp:#x} lparam={lp:#x}, result={res}");
}

/// Walks direct children in z-order (top first) via GW_CHILD + GW_HWNDNEXT.
fn direct_children(parent: HWND) -> Vec<HWND> {
    let mut out = Vec::new();
    unsafe {
        let Ok(mut h) = GetWindow(parent, GW_CHILD) else { return out };
        loop {
            out.push(h);
            match GetWindow(h, GW_HWNDNEXT) {
                Ok(next) => h = next,
                Err(_) => break,
            }
        }
    }
    out
}

fn process_of(hwnd: HWND) -> String {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return format!("pid {pid} <no access>");
    };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    unsafe { let _ = CloseHandle(h); }
    if ok {
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let base = full.rsplit('\\').next().unwrap_or(full.as_str()).to_string();
        format!("pid {pid} {base}")
    } else {
        format!("pid {pid} <unknown>")
    }
}

fn tree(label: &str, parent: HWND, depth: usize) {
    let pad = "  ".repeat(depth);
    println!("{pad}{label} {:?} class={} rect={} visible={} proc={}",
        parent.0, class_of(parent), rect_of(parent),
        unsafe { IsWindowVisible(parent) }.as_bool(), process_of(parent));
    for (i, c) in direct_children(parent).iter().enumerate() {
        tree(&format!("[z{i}]"), *c, depth + 1);
    }
}

fn main() {
    let progman = unsafe { FindWindowW(w!("Progman"), None) }.expect("Progman not found");
    println!("Progman = {:?}", progman.0);

    dump("BEFORE any message");

    println!("\n===== DIRECT CHILD TREE OF PROGMAN (z-order, top first) =====");
    tree("Progman", progman, 0);

    send(progman, 0, 0);
    dump("AFTER 0x052C (0, 0)");

    send(progman, 0xD, 0x1);
    dump("AFTER 0x052C (0xD, 0x1)");

    send(progman, 0xD, 0x0);
    dump("AFTER 0x052C (0xD, 0x0)");
}
