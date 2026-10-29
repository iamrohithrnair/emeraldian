//! The system clipboard.
//!
//! No clipboard crate: every desktop already ships a command that reads and
//! writes it, and piping through those is how helix and neovim reach it too.
//! When none of them works — most often over SSH — copying falls back to
//! OSC 52, which asks the terminal itself to set the clipboard on the machine
//! the user is actually sitting at.
//!
//! The last copy is also kept in-process, so copying and pasting inside the
//! app works even where the system clipboard can't be reached at all.
//!
//! A picture, a diagram copied as a PNG, goes the same way, through a tool
//! the system has: AppKit by way of `osascript` on macOS, `wl-copy` or
//! `xclip` on Linux, and PowerShell on Windows. OSC 52 carries only text, so
//! where none of those can reach the clipboard of the machine in front of
//! the user, as over SSH, the picture can't be copied and is saved instead.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

thread_local! {
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

/// What copying a diagram needs of the clipboard: a picture or text put on
/// it, and a folder to save a picture in when it can't take one.
///
/// A trait so tests can stand in for it: copying a diagram in a test never
/// touches the clipboard of the machine it runs on.
pub trait Clipboard {
    /// Puts a PNG on the clipboard.
    fn image(&mut self, png: &[u8]) -> Result<(), ImageError>;
    /// Puts text on the clipboard, as [`copy`] does.
    fn text(&mut self, text: &str);
    /// Where a picture is saved when the clipboard can't take it.
    fn folder(&self) -> Option<PathBuf>;
}

/// Why a picture didn't go on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// There is no clipboard here that takes pictures, and why not.
    Unreachable(&'static str),
    /// The tool that should have taken it failed, with what it said.
    Failed(String),
}

/// The system clipboard.
pub struct System;

impl Clipboard for System {
    fn image(&mut self, png: &[u8]) -> Result<(), ImageError> {
        system::copy_png(png)
    }

    fn text(&mut self, text: &str) {
        copy(text);
    }

    fn folder(&self) -> Option<PathBuf> {
        system::folder()
    }
}

/// A program that puts a PNG, given on its stdin, on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tool {
    program: String,
    args: &'static [&'static str],
    /// How long it may take. PowerShell can take seconds just to start.
    wait: Duration,
    /// Whether to read what it says when it fails. `wl-copy` and `xclip`
    /// stay behind to serve the clipboard, and can keep stderr open with
    /// them, so they are not asked.
    says: bool,
}

/// Which kind of system this is, as far as reaching its clipboard goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Mac,
    Windows,
    Unix,
}

impl Os {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// AppKit's pasteboard, from JavaScript for Automation: reads the PNG from
/// stdin and puts it on the clipboard as `public.png`, which AppleScript
/// calls `«class PNGf»`. What was on the clipboard is left alone unless a
/// picture arrived. (The bridge gives an `NSData`'s length as a string.)
const MAC_PNG: &str = "ObjC.import('AppKit');\
    var png = $.NSFileHandle.fileHandleWithStandardInput.readDataToEndOfFile;\
    if (Number(png.length) === 0) throw new Error('no picture arrived');\
    var board = $.NSPasteboard.generalPasteboard;\
    board.clearContents;\
    if (!board.setDataForType(png, $.NSPasteboardTypePNG))\
    throw new Error('the clipboard would not take the picture');";

/// The PNG from stdin on the Windows clipboard, both as PNG, which Office and
/// browsers paste with its colours exact, and as a bitmap for everything else.
const WINDOWS_PNG: &str = "$ErrorActionPreference = 'Stop'; \
    Add-Type -AssemblyName System.Windows.Forms; \
    Add-Type -AssemblyName System.Drawing; \
    $read = New-Object System.IO.MemoryStream; \
    [Console]::OpenStandardInput().CopyTo($read); \
    $read.Position = 0; \
    $png = New-Object System.IO.MemoryStream(, $read.ToArray()); \
    $data = New-Object System.Windows.Forms.DataObject; \
    $data.SetImage([System.Drawing.Image]::FromStream($read)); \
    $data.SetData('PNG', $false, $png); \
    [System.Windows.Forms.Clipboard]::SetDataObject($data, $true)";

/// The tools that can put a picture on the clipboard here, in the order to
/// try them, or why there are none. `var` reads the environment.
///
/// A session over SSH has the clipboard of the machine it runs on, not the
/// one the user sits at, so on a Mac or Windows it counts as none. On Linux
/// a display says there is a desktop to copy to: the user's own, or theirs
/// by way of X forwarding.
fn image_tools(os: Os, var: impl Fn(&str) -> Option<String>) -> Result<Vec<Tool>, &'static str> {
    let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    let remote = set("SSH_CONNECTION") || set("SSH_CLIENT") || set("SSH_TTY");
    const OVER_SSH: &str = "no clipboard for pictures over SSH";
    let tool = |program: &str, args, wait, says| Tool {
        program: program.to_string(),
        args,
        wait: Duration::from_secs(wait),
        says,
    };
    match os {
        _ if remote && os != Os::Unix => Err(OVER_SSH),
        Os::Mac => Ok(vec![tool(
            "/usr/bin/osascript",
            &["-l", "JavaScript", "-e", MAC_PNG],
            5,
            true,
        )]),
        Os::Windows => {
            // By its full path, so nothing else called PowerShell on the
            // path is run instead.
            let program = var("SystemRoot").map_or_else(
                || "powershell.exe".to_string(),
                |root| format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"),
            );
            Ok(vec![tool(
                &program,
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-STA",
                    "-Command",
                    WINDOWS_PNG,
                ],
                15,
                true,
            )])
        }
        Os::Unix => {
            let mut tools = Vec::new();
            if set("WAYLAND_DISPLAY") {
                tools.push(tool("wl-copy", &["--type", "image/png"], 5, false));
            }
            if set("DISPLAY") {
                tools.push(tool(
                    "xclip",
                    &["-selection", "clipboard", "-t", "image/png", "-i"],
                    5,
                    false,
                ));
            }
            match (tools.is_empty(), remote) {
                (false, _) => Ok(tools),
                (true, true) => Err(OVER_SSH),
                (true, false) => Err("no display to copy a picture to"),
            }
        }
    }
}

/// Puts `text` on the clipboard.
pub fn copy(text: &str) {
    LAST.with(|last| text.clone_into(&mut last.borrow_mut()));
    system::copy(text);
}

/// What is on the clipboard, with line endings made `\n`.
///
/// Falls back to the last copy made here when the system clipboard can't be
/// read.
pub fn paste() -> String {
    system::paste().map_or_else(
        || LAST.with(|last| last.borrow().clone()),
        |text| normalize(&text),
    )
}

/// Turns `\r\n` and lone `\r` into `\n`.
///
/// Windows copies with `\r\n`, and terminals deliver a bracketed paste with
/// bare `\r` for every newline — either would otherwise land in the note as a
/// stray character or as one very long line.
pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Standard base64, which is what OSC 52 carries.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, byte)| n | u32::from(*byte) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The real clipboard. Tests get a stand-in, so running them never touches
/// the clipboard of the machine they run on.
#[cfg(not(test))]
mod system {
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    const COPY: &[&[&str]] = &[&["pbcopy"]];
    #[cfg(target_os = "macos")]
    const PASTE: &[&[&str]] = &[&["pbpaste"]];

    // PowerShell rather than `clip.exe`, which reads its input in the console's
    // code page and so mangles anything outside ASCII.
    #[cfg(windows)]
    const COPY: &[&[&str]] = &[&[
        "powershell",
        "-NoProfile",
        "-Command",
        "[Console]::InputEncoding = [Text.Encoding]::UTF8; \
         Set-Clipboard -Value ([Console]::In.ReadToEnd())",
    ]];
    #[cfg(windows)]
    const PASTE: &[&[&str]] = &[&[
        "powershell",
        "-NoProfile",
        "-Command",
        "[Console]::OutputEncoding = [Text.Encoding]::UTF8; \
         [Console]::Out.Write((Get-Clipboard -Raw))",
    ]];

    // Wayland first, then the two X11 tools. Each fails fast when its display
    // isn't there, so trying them in turn costs nothing.
    #[cfg(not(any(target_os = "macos", windows)))]
    const COPY: &[&[&str]] = &[
        &["wl-copy"],
        &["xclip", "-selection", "clipboard"],
        &["xsel", "--clipboard", "--input"],
    ];
    #[cfg(not(any(target_os = "macos", windows)))]
    const PASTE: &[&[&str]] = &[
        &["wl-paste", "--no-newline"],
        &["xclip", "-selection", "clipboard", "-o"],
        &["xsel", "--clipboard", "--output"],
    ];

    pub fn copy(text: &str) {
        if !COPY.iter().any(|command| pipe_into(command, text)) {
            osc52(text);
        }
    }

    pub fn paste() -> Option<String> {
        PASTE.iter().find_map(|command| read_from(command))
    }

    fn pipe_into(command: &[&str], text: &str) -> bool {
        let child = Command::new(command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            return false;
        };
        // Taken so it is dropped — closing the pipe is what tells the tool the
        // text has ended.
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
        child.wait().is_ok_and(|status| status.success()) && written
    }

    fn read_from(command: &[&str]) -> Option<String> {
        let output = Command::new(command[0])
            .args(&command[1..])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout).ok()
    }

    /// Asks the terminal to set the clipboard. Written straight to stdout: it is
    /// an instruction to the terminal, not something to draw.
    fn osc52(text: &str) {
        let mut stdout = std::io::stdout();
        let _ = write!(stdout, "\x1b]52;c;{}\x07", super::base64(text.as_bytes()));
        let _ = stdout.flush();
    }

    pub fn copy_png(png: &[u8]) -> Result<(), super::ImageError> {
        use super::{ImageError, Os};

        let os = Os::current();
        let tools = super::image_tools(os, |name| std::env::var(name).ok())
            .map_err(ImageError::Unreachable)?;
        let mut failed = None;
        for tool in &tools {
            match run(tool, png) {
                Ok(()) => return Ok(()),
                // Not installed: the next one may be.
                Err(None) => {}
                Err(Some(said)) => failed = Some(said),
            }
        }
        let missing = if os == Os::Unix {
            "copying a picture needs wl-copy or xclip"
        } else {
            "the program that copies pictures couldn't be started"
        };
        Err(failed.map_or(ImageError::Unreachable(missing), ImageError::Failed))
    }

    /// Runs `tool` with `input` on its stdin. `Err(None)` when it isn't
    /// installed, and `Err(Some(what it said))` when it fails or hangs.
    ///
    /// The input is written from a thread of its own while this one waits,
    /// so a tool that stops reading can't stall the app past its time.
    fn run(tool: &super::Tool, input: &[u8]) -> Result<(), Option<String>> {
        use std::io::Read;
        use std::time::Instant;

        let name = std::path::Path::new(&tool.program).file_stem().map_or_else(
            || tool.program.clone(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let mut child = Command::new(&tool.program)
            .args(tool.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(if tool.says {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .spawn()
            .map_err(|_| None)?;
        let stdin = child.stdin.take();
        let stderr = child.stderr.take();
        let deadline = Instant::now() + tool.wait;
        let (status, said) = std::thread::scope(|scope| {
            scope.spawn(move || {
                if let Some(mut stdin) = stdin {
                    // Dropped at the end, which tells the tool that is all.
                    let _ = stdin.write_all(input);
                }
            });
            let said = scope.spawn(move || {
                let mut said = String::new();
                if let Some(stderr) = stderr {
                    let _ = stderr.take(4096).read_to_string(&mut said);
                }
                said
            });
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                }
            };
            (status, said.join().unwrap_or_default())
        });
        match status {
            Some(status) if status.success() => Ok(()),
            Some(status) => {
                let said = said.lines().map(str::trim).find(|line| !line.is_empty());
                Err(Some(match said {
                    Some(said) => format!("{name}: {said}"),
                    None => format!("{name} failed ({status})"),
                }))
            }
            None => Err(Some(format!("{name} took too long"))),
        }
    }

    pub fn folder() -> Option<std::path::PathBuf> {
        dirs::cache_dir().map(|dir| dir.join("emeraldian").join("diagrams"))
    }
}

#[cfg(test)]
mod system {
    pub fn copy(_: &str) {}

    pub fn paste() -> Option<String> {
        None
    }

    pub fn copy_png(_: &[u8]) -> Result<(), super::ImageError> {
        Err(super::ImageError::Unreachable("tests have no clipboard"))
    }

    pub fn folder() -> Option<std::path::PathBuf> {
        None
    }
}

/// A clipboard for tests, which keeps what it is given where the test can
/// see it.
#[cfg(test)]
pub mod stand_in {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::{Clipboard, ImageError};

    /// What was copied, shared with the test.
    #[derive(Debug, Default)]
    pub struct Copied {
        pub images: Vec<Vec<u8>>,
        pub texts: Vec<String>,
    }

    pub struct StandIn {
        pub copied: Rc<RefCell<Copied>>,
        /// Set to have pictures refused, as they would be over SSH.
        pub refuse: Option<ImageError>,
        pub folder: Option<PathBuf>,
    }

    impl StandIn {
        /// A clipboard that takes everything, and what it takes.
        pub fn new() -> (Box<Self>, Rc<RefCell<Copied>>) {
            let copied = Rc::new(RefCell::new(Copied::default()));
            let clipboard = Self {
                copied: Rc::clone(&copied),
                refuse: None,
                folder: None,
            };
            (Box::new(clipboard), copied)
        }
    }

    impl Clipboard for StandIn {
        fn image(&mut self, png: &[u8]) -> Result<(), ImageError> {
            if let Some(refused) = &self.refuse {
                return Err(refused.clone());
            }
            self.copied.borrow_mut().images.push(png.to_vec());
            Ok(())
        }

        fn text(&mut self, text: &str) {
            self.copied.borrow_mut().texts.push(text.to_string());
        }

        fn folder(&self) -> Option<PathBuf> {
            self.folder.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64("é\n".as_bytes()), "w6kK");
    }

    #[test]
    fn line_endings_come_back_as_newlines() {
        assert_eq!(normalize("a\r\nb\rc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn what_is_copied_can_be_pasted() {
        copy("- [ ] one\n- [ ] two\n");
        assert_eq!(paste(), "- [ ] one\n- [ ] two\n");
    }

    /// An environment holding just these variables.
    fn env(vars: &'static [(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn a_picture_goes_through_the_tool_each_system_has() {
        let mac = image_tools(Os::Mac, env(&[])).expect("a Mac has one");
        assert_eq!(mac[0].program, "/usr/bin/osascript");
        assert!(mac[0].args.contains(&"JavaScript") && mac[0].says);

        let windows = image_tools(Os::Windows, env(&[("SystemRoot", r"C:\Windows")]))
            .expect("so does Windows");
        assert_eq!(
            windows[0].program,
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        );
        assert!(windows[0].args.contains(&"-STA"), "the clipboard needs it");
        assert!(windows[0].wait > mac[0].wait, "PowerShell is slow to start");

        // Wayland first, then X11, and neither asked what it said, since
        // both stay behind to serve the clipboard.
        let both = env(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
        let linux = image_tools(Os::Unix, both).expect("two tools");
        let names: Vec<&str> = linux.iter().map(|tool| tool.program.as_str()).collect();
        assert_eq!(names, ["wl-copy", "xclip"]);
        assert!(
            linux
                .iter()
                .all(|tool| tool.args.contains(&"image/png") && !tool.says)
        );
        assert_eq!(
            image_tools(Os::Unix, env(&[])),
            Err("no display to copy a picture to")
        );

        // With nothing set, as on this machine's own desktop.
        let here = image_tools(Os::current(), env(&[]));
        assert_eq!(here.is_ok(), Os::current() != Os::Unix);
    }

    #[test]
    fn over_ssh_there_is_no_clipboard_for_pictures_unless_x_comes_too() {
        let ssh = &[("SSH_CONNECTION", "10.0.0.2 52000 10.0.0.1 22")];
        for os in [Os::Mac, Os::Windows, Os::Unix] {
            assert_eq!(
                image_tools(os, env(ssh)),
                Err("no clipboard for pictures over SSH"),
                "{os:?}"
            );
        }
        let forwarded = env(&[("SSH_TTY", "/dev/pts/1"), ("DISPLAY", "localhost:10.0")]);
        let tools = image_tools(Os::Unix, forwarded).expect("X forwarding");
        assert_eq!(tools[0].program, "xclip");
    }
}
