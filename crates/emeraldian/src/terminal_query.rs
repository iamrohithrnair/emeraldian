//! Asking the terminal a question at start-up, without waiting forever for
//! the answer.
//!
//! Two things are asked before the first frame: whether the terminal speaks
//! the Kitty keyboard protocol, and what pictures it can draw. Both are
//! answered on stdin, and the libraries' own ways of asking can each wait
//! forever:
//!
//! * `ratatui_image`'s `Picker::from_query_stdio` reads the answer on a thread
//!   and gives up on it after two seconds, but the thread itself carries on,
//!   blocked in a `read` of stdin. Inside tmux its question is wrapped for the
//!   outer terminal, so a detached session never answers, and that thread
//!   then takes the next bytes that do arrive: a keypress, or another query's
//!   answer. It also puts the terminal back in cooked mode when it finishes.
//! * crossterm's `supports_keyboard_enhancement` waits for readiness with a
//!   two-second timeout, but then reads stdin in blocking mode until it has a
//!   whole event. When something else took the bytes it was woken for (that
//!   thread above, in tmux on Linux), or the answer stops halfway through a
//!   sequence, the `read` never returns and nothing is ever drawn.
//!
//! So both questions are asked here instead, the same way: switch the input to
//! a mode where a `read` returns at once, wait for bytes with `poll` against a
//! hard deadline, and stop at a reply every terminal sends last. Nothing is
//! left running afterwards, and an answer cut off at the deadline is given a
//! moment to finish and then thrown away, so its tail can't turn up later as
//! keypresses.

use std::time::Duration;

/// How long a terminal gets to answer. A local one takes a millisecond or
/// two; this leaves room for a slow SSH link, and is all a terminal that
/// never answers costs.
pub const WAIT: Duration = Duration::from_secs(1);

/// Extra time for an answer that had started arriving when [`WAIT`] ran out.
#[cfg(unix)]
const GRACE: Duration = Duration::from_millis(250);

/// Whether this runs inside a terminal multiplexer, which answers queries
/// itself and says nothing about the Kitty keyboard protocol.
///
/// tmux sets `$TMUX` in its panes, and both tmux and GNU screen give them a
/// `TERM` starting with their own name. Neither passes the protocol through,
/// so asking only costs time.
pub fn in_multiplexer(tmux: Option<&str>, term: Option<&str>) -> bool {
    tmux.is_some_and(|value| !value.is_empty())
        || term.is_some_and(|term| term.starts_with("tmux") || term.starts_with("screen"))
}

/// Where the reply to a primary device attributes query (`ESC [ ? … c`) ends,
/// if `reply` holds a whole one.
///
/// Every terminal answers that query, multiplexers included, so it goes last
/// and its reply marks the end of everything asked before it.
pub fn device_attributes_end(reply: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = find(&reply[from..], b"\x1b[?") {
        let start = from + at + 3;
        let params = reply[start..]
            .iter()
            .take_while(|b| b.is_ascii_digit() || **b == b';')
            .count();
        if reply.get(start + params) == Some(&b'c') {
            return Some(start + params + 1);
        }
        from = start;
    }
    None
}

/// Whether `reply` reports the Kitty keyboard protocol's flags
/// (`ESC [ ? flags u`) before the device attributes that close it.
pub fn reports_keyboard_flags(reply: &[u8]) -> bool {
    let end = device_attributes_end(reply).unwrap_or(reply.len());
    let reply = &reply[..end];
    let mut from = 0;
    while let Some(at) = find(&reply[from..], b"\x1b[?") {
        let start = from + at + 3;
        let digits = reply[start..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits > 0 && reply.get(start + digits) == Some(&b'u') {
            return true;
        }
        from = start;
    }
    false
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Whether the terminal speaks the Kitty keyboard protocol.
///
/// Asked only outside a multiplexer, and never for longer than [`WAIT`].
pub fn supports_keyboard_enhancement() -> bool {
    let tmux = std::env::var("TMUX").ok();
    let term = std::env::var("TERM").ok();
    if in_multiplexer(tmux.as_deref(), term.as_deref()) {
        return false;
    }
    ask(b"\x1b[?u\x1b[c", WAIT, |reply| {
        device_attributes_end(reply).is_some()
    })
    .is_some_and(|reply| reports_keyboard_flags(&reply))
}

/// Writes `request` to stdout and collects the terminal's reply from its
/// input until `done` says the reply is whole.
///
/// Returns `None` when the terminal didn't finish answering in time, and
/// always on Windows, whose console has no such replies.
pub fn ask(request: &[u8], wait: Duration, done: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    sys::ask(request, wait, done)
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io::{self, Write};
    use std::os::fd::{AsFd, BorrowedFd};
    use std::time::{Duration, Instant};

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use rustix::termios::{
        OptionalActions, QueueSelector, SpecialCodeIndex, Termios, isatty, tcflush, tcgetattr,
        tcsetattr,
    };

    /// Puts the terminal's input back the way it was found, however the
    /// question ends.
    struct Restore<'fd> {
        fd: BorrowedFd<'fd>,
        saved: Termios,
    }

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = tcsetattr(self.fd, OptionalActions::Now, &self.saved);
        }
    }

    pub fn ask(request: &[u8], wait: Duration, done: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        // The same input crossterm reads keys from: stdin when it is a
        // terminal, the controlling terminal otherwise.
        let stdin = io::stdin();
        if isatty(stdin.as_fd()) {
            ask_on(stdin.as_fd(), &mut io::stdout(), request, wait, done)
        } else {
            let tty = File::options()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .ok()?;
            ask_on(tty.as_fd(), &mut io::stdout(), request, wait, done)
        }
    }

    /// [`ask`] on an explicit input and output, so it can be tested against
    /// a pseudo-terminal.
    pub fn ask_on(
        fd: BorrowedFd<'_>,
        out: &mut impl Write,
        request: &[u8],
        wait: Duration,
        done: impl Fn(&[u8]) -> bool,
    ) -> Option<Vec<u8>> {
        let saved = tcgetattr(fd).ok()?;
        // Raw, so the reply is neither echoed nor held back for a newline,
        // and with VMIN and VTIME at zero so that a `read` returns at once
        // with whatever has arrived, never waiting for more.
        let mut raw = saved.clone();
        raw.make_raw();
        raw.special_codes[SpecialCodeIndex::VMIN] = 0;
        raw.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(fd, OptionalActions::Now, &raw).ok()?;
        let _restore = Restore { fd, saved };

        out.write_all(request).ok()?;
        out.flush().ok()?;

        let mut reply = Vec::new();
        let mut deadline = Instant::now() + wait;
        let mut extended = false;
        let mut buf = [0u8; 1024];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                // An answer already under way gets a moment to finish, so
                // its tail isn't left behind to be read as keys.
                if reply.is_empty() || extended {
                    break;
                }
                extended = true;
                deadline = Instant::now() + super::GRACE;
                continue;
            }
            let Ok(timeout) = Timespec::try_from(left) else {
                break;
            };
            let mut fds = [PollFd::from_borrowed_fd(fd, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(Errno::INTR) => continue,
                Ok(_) => {}
                Err(_) => break,
            }
            let revents = fds[0].revents();
            if !revents.contains(PollFlags::IN) {
                // Hung up or invalid: nothing more is coming.
                break;
            }
            // With VMIN and VTIME at zero this returns at once, even if
            // something else took the bytes `poll` saw.
            match rustix::io::read(fd, &mut buf) {
                Ok(0) if revents.contains(PollFlags::HUP) => break,
                // The bytes went elsewhere; wait a little rather than spin.
                Ok(0) => std::thread::sleep(Duration::from_millis(5)),
                Ok(n) => {
                    reply.extend_from_slice(&buf[..n]);
                    if done(&reply) {
                        return Some(reply);
                    }
                }
                Err(Errno::INTR | Errno::AGAIN) => {}
                Err(_) => break,
            }
        }
        // Whatever part of an answer did arrive is dropped along with
        // anything else unread, rather than parsed later as keypresses.
        let _ = tcflush(fd, QueueSelector::IFlush);
        None
    }
}

#[cfg(not(unix))]
mod sys {
    use std::time::Duration;

    /// Windows consoles have no such replies to wait for; crossterm reads
    /// keys there through the console API instead.
    pub fn ask(_: &[u8], _: Duration, _: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplexers_are_recognised() {
        assert!(in_multiplexer(Some("/tmp/tmux-501/default,1,0"), None));
        assert!(in_multiplexer(None, Some("tmux-256color")));
        assert!(in_multiplexer(None, Some("screen")));
        assert!(in_multiplexer(None, Some("screen.xterm-256color")));
        assert!(!in_multiplexer(Some(""), Some("xterm-kitty")));
        assert!(!in_multiplexer(None, Some("xterm-256color")));
        assert!(!in_multiplexer(None, None));
    }

    #[test]
    fn device_attributes_close_a_reply() {
        assert_eq!(device_attributes_end(b"\x1b[?1;2;4c"), Some(9));
        assert_eq!(device_attributes_end(b"\x1b[?1u\x1b[?62;22c"), Some(14));
        assert_eq!(device_attributes_end(b"\x1b[?62;22"), None);
        assert_eq!(device_attributes_end(b"\x1b[?1u"), None);
        assert_eq!(device_attributes_end(b""), None);
    }

    #[test]
    fn keyboard_flags_only_count_before_the_attributes() {
        assert!(reports_keyboard_flags(b"\x1b[?0u\x1b[?62;c"));
        assert!(reports_keyboard_flags(b"\x1b[?15u\x1b[?1;2c"));
        assert!(!reports_keyboard_flags(b"\x1b[?1;2;4c"));
        assert!(!reports_keyboard_flags(b"\x1b[?1;2c\x1b[?1u"));
        assert!(!reports_keyboard_flags(b"\x1b[?u"));
    }

    #[cfg(unix)]
    mod pty {
        use std::fs::File;
        use std::io::Write;
        use std::os::fd::AsFd;
        use std::time::{Duration, Instant};

        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
        use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};

        use super::super::{device_attributes_end, sys};

        /// A pseudo-terminal: the end the program reads, and the end the
        /// "terminal" writes its answers into.
        fn pty() -> (File, File) {
            let leader = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
            grantpt(&leader).unwrap();
            unlockpt(&leader).unwrap();
            let name = ptsname(&leader, Vec::new()).unwrap();
            let follower = File::options()
                .read(true)
                .write(true)
                .open(name.to_str().unwrap())
                .unwrap();
            // Raw from the start, as the app's terminal is by the time it
            // asks, so what the "terminal" writes is neither echoed back nor
            // held for a newline.
            let mut mode = tcgetattr(&follower).unwrap();
            mode.make_raw();
            tcsetattr(&follower, OptionalActions::Now, &mode).unwrap();
            (follower, File::from(leader))
        }

        fn ask(input: &File, wait: Duration) -> (Option<Vec<u8>>, Duration) {
            let started = Instant::now();
            let reply = sys::ask_on(input.as_fd(), &mut Vec::new(), b"\x1b[c", wait, |reply| {
                device_attributes_end(reply).is_some()
            });
            (reply, started.elapsed())
        }

        #[test]
        fn a_silent_terminal_costs_the_wait_and_no_more() {
            let (input, _terminal) = pty();
            let (reply, took) = ask(&input, Duration::from_millis(200));
            assert_eq!(reply, None);
            assert!(took < Duration::from_secs(1), "took {took:?}");
        }

        #[test]
        fn an_answer_ends_the_wait() {
            let (input, mut terminal) = pty();
            terminal.write_all(b"\x1b[?1;2c").unwrap();
            let (reply, took) = ask(&input, Duration::from_secs(5));
            assert_eq!(reply.as_deref(), Some(&b"\x1b[?1;2c"[..]));
            assert!(took < Duration::from_secs(1), "took {took:?}");
        }

        #[test]
        fn half_an_answer_is_neither_waited_on_nor_left_behind() {
            let (input, mut terminal) = pty();
            terminal.write_all(b"\x1b[?1;").unwrap();
            let (reply, took) = ask(&input, Duration::from_millis(200));
            assert_eq!(reply, None);
            assert!(took < Duration::from_secs(1), "took {took:?}");

            // The fragment was flushed: the next question sees only its own
            // answer.
            terminal.write_all(b"\x1b[?62c").unwrap();
            let (reply, _) = ask(&input, Duration::from_secs(5));
            assert_eq!(reply.as_deref(), Some(&b"\x1b[?62c"[..]));
        }
    }
}
