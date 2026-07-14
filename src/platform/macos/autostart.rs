//! Launch at login, via a `~/Library/LaunchAgents` plist.
//!
//! **Not `SMAppService`**, the modern API. It refuses to register anything whose code
//! signature it cannot validate (`-67054`, "Static code signature check failed"), and this
//! app ships ad-hoc signed and unnotarized on purpose (design §D3). A LaunchAgent plist has
//! no such requirement, and it is what every unsigned menu-bar app on the platform uses.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

const LABEL: &str = "com.hajh.desktop-countdown";

/// Whether the launch agent is installed and points at *this* executable.
///
/// **Not a reliable answer to "will it actually start at login".** The user can turn the
/// item off in System Settings → General → Login Items, and macOS then leaves the plist in
/// place and simply declines to run it. There is no supported way to read that state back,
/// so the checkbox in the settings window reflects what we wrote, not what macOS will do.
pub fn is_enabled() -> Result<bool> {
    let path = plist_path()?;
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    // A plist left behind by a copy of the app that has since moved is not "enabled" for
    // the copy running now -- `set_enabled(true)` will rewrite it.
    Ok(contents.contains(&exe_path()?.to_string_lossy().to_string()))
}

pub fn set_enabled(on: bool) -> Result<()> {
    if on {
        enable()
    } else {
        disable()
    }
}

fn enable() -> Result<()> {
    let path = plist_path()?;
    let wanted = plist(&exe_path()?)?;

    // The absolute path of the executable is baked into the plist, so an app that has been
    // moved (or a `cargo run` after a `cargo install`) has a stale one. Rewrite whenever it
    // does not match, not just when it is missing.
    if std::fs::read_to_string(&path).ok().as_deref() == Some(wanted.as_str()) {
        return Ok(());
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, &wanted).with_context(|| format!("writing {}", path.display()))?;
    tracing::info!(path = %path.display(), "launch agent written");

    // The plist alone takes effect at the *next* login. `bootstrap` loads it now, so the
    // toggle does what the user just asked for rather than what they will get tomorrow.
    // A stale registration would make it fail, so drop that first and ignore the result:
    // "was not loaded" is the normal case, not an error.
    let _ = launchctl(&["bootout", &domain_target()]);
    launchctl(&["bootstrap", &gui_domain(), &path.to_string_lossy()])
}

fn disable() -> Result<()> {
    let path = plist_path()?;

    // Unload first: removing the file out from under launchd leaves it running until logout.
    let _ = launchctl(&["bootout", &domain_target()]);

    match std::fs::remove_file(&path) {
        Ok(()) => tracing::info!(path = %path.display(), "launch agent removed"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
    }
    Ok(())
}

fn launchctl(args: &[&str]) -> Result<()> {
    let out = Command::new("/bin/launchctl")
        .args(args)
        .output()
        .with_context(|| format!("running launchctl {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(anyhow!(
            "launchctl {} failed ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn gui_domain() -> String {
    // SAFETY: `getuid` is always safe; it reads a process attribute and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn domain_target() -> String {
    format!("{}/{LABEL}", gui_domain())
}

fn plist_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    let mut p = PathBuf::from(home);
    p.push("Library/LaunchAgents");
    p.push(format!("{LABEL}.plist"));
    Ok(p)
}

/// The executable launchd should run.
///
/// Inside a bundle this is `DesktopCountdown.app/Contents/MacOS/desktop-countdown`, and it
/// has to be: handing launchd the `.app` directory gets it nowhere, because a directory is
/// not something it can exec. `current_exe` already gives us the inner binary, which is why
/// there is nothing to unwrap here.
fn exe_path() -> Result<PathBuf> {
    std::env::current_exe().context("resolving the path of the running executable")
}

fn plist(exe: &std::path::Path) -> Result<String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| anyhow!("the executable path is not valid UTF-8: {}", exe.display()))?;
    // A path with an XML metacharacter in it would otherwise produce a plist launchd cannot
    // parse -- and `&` is perfectly legal in a macOS directory name.
    let exe = xml_escape(exe);

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<false/>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#
    ))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `KeepAlive` must be false, or the tray's "quit" would be meaningless: launchd would
    /// bring the app straight back up.
    #[test]
    fn the_plist_does_not_resurrect_a_quit_app() {
        let p = plist(std::path::Path::new("/Applications/X.app/Contents/MacOS/x")).unwrap();
        assert!(p.contains("<key>KeepAlive</key>\n\t<false/>"), "{p}");
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"), "{p}");
    }

    #[test]
    fn the_plist_names_the_executable_not_the_bundle() {
        let exe = "/Applications/DesktopCountdown.app/Contents/MacOS/desktop-countdown";
        let p = plist(std::path::Path::new(exe)).unwrap();
        assert!(p.contains(&format!("<string>{exe}</string>")), "{p}");
    }

    /// `&` is legal in a directory name and illegal in XML. An unescaped one produces a
    /// plist launchd cannot parse, and autostart silently never works.
    #[test]
    fn a_path_with_xml_metacharacters_is_escaped() {
        let p = plist(std::path::Path::new("/Users/a&b/App.app/Contents/MacOS/x")).unwrap();
        assert!(p.contains("/Users/a&amp;b/"), "{p}");
        assert!(!p.contains("/Users/a&b/"), "the raw & survived: {p}");
    }

    #[test]
    fn the_plist_lives_in_the_users_launch_agents() {
        let p = plist_path().unwrap();
        assert!(
            p.ends_with("Library/LaunchAgents/com.hajh.desktop-countdown.plist"),
            "{p:?}"
        );
    }

    /// Enable, read back, disable, read back. Touches the real `~/Library/LaunchAgents` and
    /// really calls `launchctl`, so it is `#[ignore]`d: `cargo test` must not install a
    /// launch agent on whoever runs it.
    ///
    /// Run with `cargo test autostart -- --ignored`.
    #[test]
    #[ignore]
    fn enabling_then_disabling_round_trips() {
        set_enabled(true).unwrap();
        assert!(is_enabled().unwrap());
        set_enabled(false).unwrap();
        assert!(!is_enabled().unwrap());
    }
}
