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

use std::cell::RefCell;

thread_local! {
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
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
}

#[cfg(test)]
mod system {
    pub fn copy(_: &str) {}

    pub fn paste() -> Option<String> {
        None
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
}
