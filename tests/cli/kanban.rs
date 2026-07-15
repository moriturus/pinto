//! Kanban command validation and configuration.

use super::common::*;

#[test]
fn kanban_subcommand_listed_in_help() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("kanban"));
}

#[test]
fn kanban_help_lists_display_column_and_maximize_options() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["kanban", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--column <COLUMN>..."))
        .stdout(predicate::str::contains("hidden_columns"))
        .stdout(predicate::str::contains("--maximize"))
        .stdout(predicate::str::contains("-F, --search"))
        .stdout(predicate::str::contains("-R, --regex"));
}

#[test]
fn kanban_accepts_multiple_display_columns_and_validates_each_one() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["kanban", "--column", "todo", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("column not found: missing"));
}

#[test]
fn kanban_rejects_unknown_hidden_column_from_config_before_terminal_startup() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("[tui]\n", "[tui]\nhidden_columns = [\"missing\"]\n"),
    )
    .expect("write config");

    pinto(dir.path())
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("hidden_columns"))
        .stderr(predicate::str::contains("missing"));
}

#[test]
fn kanban_accepts_custom_key_bindings_before_opening_the_terminal() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        r#"columns = ["todo", "in-progress", "review", "done"]
done_column = "done"

[project]
name = "test"
key = "T"

[tui]
confirm_quit = true

[tui.key_bindings]
details = ["Cmd+d", "v"]
quit = ["Ctrl+a", "Esc"]

[storage]
backend = "file"

[wip]
enabled = true
"#,
    )
    .expect("write config");

    // A missing display column fails after settings have been parsed, but before a TTY is opened.
    pinto(dir.path())
        .args(["kanban", "--column", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("column not found: missing"))
        .stderr(predicate::str::contains("invalid Kanban key bindings").not());
}

#[test]
fn kanban_rejects_invalid_key_bindings_with_actionable_guidance() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        r#"columns = ["todo", "in-progress", "review", "done"]
done_column = "done"

[project]
name = "test"
key = "T"

[tui]
confirm_quit = true

[tui.key_bindings]
details = ["Controlled+d"]

[storage]
backend = "file"

[wip]
enabled = true
"#,
    )
    .expect("write config");

    pinto(dir.path())
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("details"))
        .stderr(predicate::str::contains("Ctrl"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn kanban_rejects_an_unknown_display_column_before_opening_the_terminal() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["kanban", "--column", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("column not found: missing"));
}

#[test]
fn kanban_on_uninitialized_dir_errors_before_opening_terminal() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn kanban_initialized_non_tty_restores_setup_and_reports_terminal_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("terminal"))
        .stderr(predicate::str::contains("panicked").not());
}

#[cfg(unix)]
mod pty_tests {
    use super::*;
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command as ProcessCommand, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const WAIT: Duration = Duration::from_secs(3);
    const CURSOR_QUERY: &[u8] = b"\x1b[6n";
    const CURSOR_RESPONSE: &[u8] = b"\x1b[1;1R";

    struct Pty {
        master: File,
        slave: File,
        initial_lflag: libc::tcflag_t,
        answered_queries: usize,
    }

    impl Pty {
        fn open() -> io::Result<Self> {
            let mut master = -1;
            let mut slave = -1;
            #[cfg(target_os = "linux")]
            let window = libc::winsize {
                ws_row: 24,
                ws_col: 100,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            #[cfg(not(target_os = "linux"))]
            let mut window = libc::winsize {
                ws_row: 24,
                ws_col: 100,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            #[cfg(target_os = "linux")]
            let result = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &window,
                )
            };
            #[cfg(not(target_os = "linux"))]
            let result = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut window,
                )
            };
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: openpty initialized both descriptors on success and transfers ownership to
            // these File values exactly once.
            let master = unsafe { File::from_raw_fd(master) };
            let slave = unsafe { File::from_raw_fd(slave) };
            let initial_lflag = Self::lflag(&slave)?;
            let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
            if flags == -1 {
                return Err(io::Error::last_os_error());
            }
            let result =
                unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                master,
                slave,
                initial_lflag,
                answered_queries: 0,
            })
        }

        fn lflag(file: &File) -> io::Result<libc::tcflag_t> {
            let mut state = unsafe { std::mem::zeroed::<libc::termios>() };
            let result = unsafe { libc::tcgetattr(file.as_raw_fd(), &mut state) };
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(state.c_lflag)
        }

        fn resize(&self, rows: u16, columns: u16) -> io::Result<()> {
            let window = libc::winsize {
                ws_row: rows,
                ws_col: columns,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let result =
                unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &window) };
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        fn send(&mut self, input: &[u8]) -> io::Result<()> {
            self.master.write_all(input)
        }

        fn spawn(&self, command: &mut ProcessCommand) -> io::Result<Child> {
            // SAFETY: `pre_exec` runs in the forked child before `exec`. The closure only calls
            // `setsid` so the child does not inherit the test runner's controlling terminal.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            command.spawn()
        }

        fn read_until(
            &mut self,
            output: &mut Vec<u8>,
            condition: impl Fn(&[u8]) -> bool,
        ) -> io::Result<()> {
            let deadline = Instant::now() + WAIT;
            while Instant::now() < deadline {
                let timeout = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(100) as libc::c_int;
                let mut poll = libc::pollfd {
                    fd: self.master.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
                if ready == -1 {
                    return Err(io::Error::last_os_error());
                }
                if ready == 0 {
                    continue;
                }
                let mut buffer = [0_u8; 8192];
                match self.master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => output.extend_from_slice(&buffer[..read]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                let cursor_queries = output
                    .windows(CURSOR_QUERY.len())
                    .filter(|window| *window == CURSOR_QUERY)
                    .count();
                while self.answered_queries < cursor_queries {
                    self.send(CURSOR_RESPONSE)?;
                    self.answered_queries += 1;
                }
                if condition(output) {
                    return Ok(());
                }
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                String::from_utf8_lossy(output).into_owned(),
            ))
        }
    }

    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                for _ in 0..100 {
                    if self.0.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    fn wait_for_exit(child: &mut Child, timeout: Duration) -> io::Result<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "child did not exit",
        ))
    }

    #[test]
    fn kanban_pty_smoke_covers_resize_editor_and_terminal_restore() {
        let dir = TempDir::new().expect("temp dir");
        init_with_items(dir.path(), &["PTY smoke"]);
        let config_path = dir.path().join(".pinto/config.toml");
        let config = std::fs::read_to_string(&config_path).expect("config");
        let config = config
            .replace("confirm_quit = true", "confirm_quit = false")
            .replace("shell = [\"Q\"]", "shell = [\"Ctrl+z\"]");
        std::fs::write(config_path, config).expect("disable quit confirmation");
        let editor = editor_script(dir.path(), "pty-editor.sh", "exit 0");
        let binary = pinto(dir.path()).get_program().to_owned();
        let mut pty = Pty::open().expect("open pseudo terminal");
        let mut command = ProcessCommand::new(binary);
        command
            .arg("kanban")
            .current_dir(dir.path())
            .env("TERM", "xterm-256color")
            .env("EDITOR", editor)
            .env_remove("VISUAL")
            .stdin(Stdio::from(pty.slave.try_clone().expect("clone stdin")))
            .stdout(Stdio::from(pty.slave.try_clone().expect("clone stdout")))
            .stderr(Stdio::from(pty.slave.try_clone().expect("clone stderr")));
        let mut child = ChildGuard(pty.spawn(&mut command).expect("spawn pinto kanban"));
        let mut output = Vec::new();
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"\x1b[?1049h".len())
                .any(|window| window == b"\x1b[?1049h")
        })
        .unwrap_or_else(|error| panic!("kanban did not enter alternate screen: {error}"));

        pty.resize(40, 120).expect("resize pseudo terminal");
        let _ = unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGWINCH) };
        pty.send(b"j").expect("send resize smoke input");
        pty.send(b"e").expect("open editor");
        let alternate_enters = |bytes: &[u8]| {
            bytes
                .windows(b"\x1b[?1049h".len())
                .filter(|window| *window == b"\x1b[?1049h")
                .count()
                >= 2
        };
        pty.read_until(&mut output, alternate_enters)
            .unwrap_or_else(|error| panic!("editor handoff did not return to TUI: {error}"));

        // `Terminal::clear` asks a real terminal for the cursor position after re-entering the
        // alternate screen. Answer the same DSR query a terminal emulator would answer.
        pty.send(b"qy").expect("quit kanban");
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"\x1b[?1049l".len())
                .filter(|window| *window == b"\x1b[?1049l")
                .count()
                >= 2
        })
        .unwrap_or_else(|error| panic!("kanban did not leave alternate screen: {error}"));
        let status = wait_for_exit(&mut child.0, WAIT).unwrap_or_else(|error| {
            panic!(
                "wait pinto kanban: {error}; output: {}",
                String::from_utf8_lossy(&output)
            )
        });
        assert!(status.success(), "kanban exited with {status}");
        assert!(
            output
                .windows(b"\x1b[?1049l".len())
                .any(|window| window == b"\x1b[?1049l"),
            "alternate screen was not left: {}",
            String::from_utf8_lossy(&output)
        );
        let restored_lflag = Pty::lflag(&pty.slave).expect("read restored terminal state");
        let raw_mode_bits = libc::ICANON | libc::ECHO;
        assert_eq!(
            restored_lflag & raw_mode_bits,
            pty.initial_lflag & raw_mode_bits,
            "raw mode flags were not restored"
        );
    }

    #[test]
    fn kanban_pty_smoke_renders_cards_navigates_and_opens_details() {
        let dir = TempDir::new().expect("temp dir");
        pinto(dir.path()).arg("init").assert().success();
        pinto(dir.path())
            .args(["add", "First card", "--body", "First card body"])
            .assert()
            .success();
        pinto(dir.path())
            .args(["add", "Second card", "--body", "Second card details marker"])
            .assert()
            .success();

        let config_path = dir.path().join(".pinto/config.toml");
        let config = std::fs::read_to_string(&config_path).expect("config");
        std::fs::write(
            config_path,
            config.replace("confirm_quit = true", "confirm_quit = false"),
        )
        .expect("disable quit confirmation");

        let binary = pinto(dir.path()).get_program().to_owned();
        let mut pty = Pty::open().expect("open pseudo terminal");
        let mut command = ProcessCommand::new(binary);
        command
            .arg("kanban")
            .current_dir(dir.path())
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(pty.slave.try_clone().expect("clone stdin")))
            .stdout(Stdio::from(pty.slave.try_clone().expect("clone stdout")))
            .stderr(Stdio::from(pty.slave.try_clone().expect("clone stderr")));
        let mut child = ChildGuard(pty.spawn(&mut command).expect("spawn pinto kanban"));
        let mut output = Vec::new();
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"First card".len())
                .any(|window| window == b"First card")
                && bytes.windows(b"T-2".len()).any(|window| window == b"T-2")
                && bytes
                    .windows(b"Second".len())
                    .any(|window| window == b"Second")
        })
        .unwrap_or_else(|error| panic!("kanban did not render both cards: {error}"));

        // Give the popup enough vertical space for both its metadata and body. Move the selection
        // to the second card, then open its detail popup. The body marker is absent from the board
        // card and proves that both the key event and the popup render ran.
        pty.resize(40, 120).expect("resize for details popup");
        let _ = unsafe { libc::kill(child.0.id() as libc::pid_t, libc::SIGWINCH) };
        pty.send(b"jv").expect("navigate and open details");
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"marker".len())
                .any(|window| window == b"marker")
        })
        .unwrap_or_else(|error| panic!("details popup did not render the selected card: {error}"));

        pty.send(b"vq").expect("close details and quit kanban");
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"\x1b[?1049l".len())
                .any(|window| window == b"\x1b[?1049l")
        })
        .unwrap_or_else(|error| panic!("kanban did not leave alternate screen: {error}"));
        let status = wait_for_exit(&mut child.0, WAIT).expect("wait pinto kanban");
        assert!(status.success(), "kanban exited with {status}");
    }

    #[test]
    fn shell_can_reenter_kanban_without_leaking_lifecycle_state() {
        let dir = TempDir::new().expect("temp dir");
        init_with_items(dir.path(), &["Repeated lifecycle"]);
        let config_path = dir.path().join(".pinto/config.toml");
        let config = std::fs::read_to_string(&config_path).expect("config");
        std::fs::write(
            config_path,
            config.replace("confirm_quit = true", "confirm_quit = false"),
        )
        .expect("disable quit confirmation");
        let binary = pinto(dir.path()).get_program().to_owned();
        let mut pty = Pty::open().expect("open pseudo terminal");
        let mut command = ProcessCommand::new(binary);
        command
            .arg("shell")
            .current_dir(dir.path())
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(pty.slave.try_clone().expect("clone stdin")))
            .stdout(Stdio::from(pty.slave.try_clone().expect("clone stdout")))
            .stderr(Stdio::from(pty.slave.try_clone().expect("clone stderr")));
        let mut child = ChildGuard(pty.spawn(&mut command).expect("spawn pinto shell"));
        let mut output = Vec::new();
        let prompt = |bytes: &[u8], count| {
            bytes
                .windows(b"pinto> ".len())
                .filter(|window| *window == b"pinto> ")
                .count()
                >= count
        };
        pty.read_until(&mut output, |bytes| prompt(bytes, 1))
            .unwrap_or_else(|error| panic!("shell prompt did not appear: {error}"));

        pty.send(b"k\r").expect("enter first kanban");
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"\x1b[?1049h".len())
                .filter(|window| *window == b"\x1b[?1049h")
                .count()
                >= 1
        })
        .unwrap_or_else(|error| panic!("first kanban did not start: {error}"));
        pty.send(b"Q").expect("return from first kanban");
        pty.read_until(&mut output, |bytes| prompt(bytes, 2))
            .unwrap_or_else(|error| panic!("shell prompt did not return: {error}"));

        pty.send(b"k\r").expect("enter second kanban");
        pty.read_until(&mut output, |bytes| {
            bytes
                .windows(b"\x1b[?1049h".len())
                .filter(|window| *window == b"\x1b[?1049h")
                .count()
                >= 2
        })
        .unwrap_or_else(|error| panic!("second kanban did not start: {error}"));
        pty.send(b"Q").expect("return from second kanban");
        pty.read_until(&mut output, |bytes| prompt(bytes, 3))
            .unwrap_or_else(|error| panic!("shell prompt did not return a second time: {error}"));

        pty.send(b"\x04").expect("exit shell");
        let status = wait_for_exit(&mut child.0, WAIT).unwrap_or_else(|error| {
            panic!(
                "wait pinto shell: {error}; output: {}",
                String::from_utf8_lossy(&output)
            )
        });
        assert!(status.success(), "shell exited with {status}");
        assert!(
            output
                .windows(b"\x1b[?1049l".len())
                .filter(|window| *window == b"\x1b[?1049l")
                .count()
                >= 2,
            "both Kanban sessions must leave alternate screen"
        );
        let restored_lflag = Pty::lflag(&pty.slave).expect("read restored terminal state");
        let raw_mode_bits = libc::ICANON | libc::ECHO;
        assert_eq!(
            restored_lflag & raw_mode_bits,
            pty.initial_lflag & raw_mode_bits,
            "shell input terminal flags were not restored"
        );
    }
}
