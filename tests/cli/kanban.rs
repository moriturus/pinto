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
        .stdout(predicate::str::contains("-R, --regex"))
        .stdout(predicate::str::contains("-S, --sprint <SPRINT>"))
        .stdout(predicate::str::contains("-L, --label <LABEL>..."))
        .stdout(predicate::str::contains("-a, --all-labels"));
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
    let config_home = dir.path().join("xdg");
    std::fs::create_dir_all(config_home.join("pinto")).expect("create user config directory");
    std::fs::write(
        config_home.join("pinto/config.toml"),
        r#"[tui.key_bindings]
details = ["Cmd+d", "v"]
quit = ["Ctrl+a", "Esc"]
"#,
    )
    .expect("write user config");

    // A missing display column fails after settings have been parsed, but before a TTY is opened.
    pinto(dir.path())
        .env("XDG_CONFIG_HOME", &config_home)
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
    let config_home = dir.path().join("xdg");
    std::fs::create_dir_all(config_home.join("pinto")).expect("create user config directory");
    std::fs::write(
        config_home.join("pinto/config.toml"),
        r#"[tui.key_bindings]
details = ["Controlled+d"]
"#,
    )
    .expect("write user config");

    pinto(dir.path())
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("details"))
        .stderr(predicate::str::contains("Ctrl"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn kanban_rejects_key_bindings_left_in_shared_board_configuration() {
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
details = ["v"]

[storage]
backend = "file"

[wip]
enabled = true
"#,
    )
    .expect("write legacy board config");

    pinto(dir.path())
        .arg("kanban")
        .assert()
        .failure()
        .stderr(predicate::str::contains("key_bindings"))
        .stderr(predicate::str::contains("XDG_CONFIG_HOME"))
        .stderr(predicate::str::contains("config.toml"));
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
    use std::path::Path;
    use std::process::{Child, Command as ProcessCommand, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    const WAIT: Duration = Duration::from_secs(3);
    // macOS runners can take longer to hand the PTY back to rustyline after repeated TUI
    // lifecycles. Keep the exit assertion strict while allowing that platform-specific teardown
    // latency to settle.
    const SHELL_EXIT_WAIT: Duration = Duration::from_secs(10);
    const CURSOR_QUERY: &[u8] = b"\x1b[6n";
    const CURSOR_RESPONSE: &[u8] = b"\x1b[1;1R";
    const ALTERNATE_ENTER: &[u8] = b"\x1b[?1049h";
    const ALTERNATE_LEAVE: &[u8] = b"\x1b[?1049l";

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
                let ready = poll_pty(&mut poll, timeout)?;
                if ready == 0 {
                    continue;
                }
                let mut buffer = [0_u8; 8192];
                match self.master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => output.extend_from_slice(&buffer[..read]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
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

        fn read_available(&mut self, output: &mut Vec<u8>) -> io::Result<()> {
            loop {
                let mut buffer = [0_u8; 8192];
                let read = match self.master.read(&mut buffer) {
                    Ok(0) => return Ok(()),
                    Ok(read) => read,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) => return Err(error),
                };
                output.extend_from_slice(&buffer[..read]);
                let cursor_queries = output
                    .windows(CURSOR_QUERY.len())
                    .filter(|window| *window == CURSOR_QUERY)
                    .count();
                while self.answered_queries < cursor_queries {
                    self.send(CURSOR_RESPONSE)?;
                    self.answered_queries += 1;
                }
            }
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

    struct TuiSession {
        pty: Pty,
        child: ChildGuard,
        output: Vec<u8>,
    }

    impl TuiSession {
        fn start(dir: &Path, args: &[&str], editor: Option<&Path>) -> Self {
            let binary = pinto(dir).get_program().to_owned();
            let pty = Pty::open().expect("open pseudo terminal");
            let mut command = ProcessCommand::new(binary);
            command
                .args(args)
                .current_dir(dir)
                .env("TERM", "xterm-256color")
                .env_remove("VISUAL")
                .stdin(Stdio::from(pty.slave.try_clone().expect("clone stdin")))
                .stdout(Stdio::from(pty.slave.try_clone().expect("clone stdout")))
                .stderr(Stdio::from(pty.slave.try_clone().expect("clone stderr")));
            if let Some(editor) = editor {
                command.env("EDITOR", editor);
            }
            let child = ChildGuard(pty.spawn(&mut command).expect("spawn pinto TUI command"));
            Self {
                pty,
                child,
                output: Vec::new(),
            }
        }

        fn wait_until(&mut self, condition: impl Fn(&[u8]) -> bool, context: &str) {
            self.pty
                .read_until(&mut self.output, condition)
                .unwrap_or_else(|error| {
                    panic!(
                        "{context}: {error}; output: {}",
                        String::from_utf8_lossy(&self.output)
                    )
                });
        }

        fn send(&mut self, input: &[u8], context: &str) {
            self.pty
                .send(input)
                .unwrap_or_else(|error| panic!("{context}: {error}"));
        }

        fn enter_alternate_screen(&mut self) {
            self.wait_until(
                |bytes| {
                    bytes
                        .windows(ALTERNATE_ENTER.len())
                        .any(|window| window == ALTERNATE_ENTER)
                },
                "TUI did not enter the alternate screen",
            );
        }

        fn leave_with(&mut self, input: &[u8]) {
            let previous_leaves = self
                .output
                .windows(ALTERNATE_LEAVE.len())
                .filter(|window| *window == ALTERNATE_LEAVE)
                .count();
            self.send(input, "send TUI exit input");
            self.wait_until(
                move |bytes| {
                    bytes
                        .windows(ALTERNATE_LEAVE.len())
                        .filter(|window| *window == ALTERNATE_LEAVE)
                        .count()
                        > previous_leaves
                },
                "TUI did not leave the alternate screen",
            );
        }

        fn assert_success(&mut self, timeout: Duration) {
            let status = wait_for_exit_while_draining(
                &mut self.child.0,
                &mut self.pty,
                &mut self.output,
                timeout,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "wait for TUI command: {error}; output: {}",
                    String::from_utf8_lossy(&self.output)
                )
            });
            assert!(status.success(), "TUI command exited with {status}");
        }
    }

    fn wait_for_exit_while_draining(
        child: &mut Child,
        pty: &mut Pty,
        output: &mut Vec<u8>,
        timeout: Duration,
    ) -> io::Result<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait()? {
                pty.read_available(output)?;
                return Ok(status);
            }
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(100) as libc::c_int;
            let mut poll = libc::pollfd {
                fd: pty.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = poll_pty(&mut poll, timeout)?;
            if ready > 0 {
                pty.read_available(output)?;
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "child did not exit",
        ))
    }

    fn poll_pty(poll: &mut libc::pollfd, timeout: libc::c_int) -> io::Result<libc::c_int> {
        loop {
            let ready = unsafe { libc::poll(poll, 1, timeout) };
            if ready != -1 {
                return Ok(ready);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
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
        let mut session = TuiSession::start(dir.path(), &["kanban"], Some(&editor));
        session.enter_alternate_screen();

        session.pty.resize(40, 120).expect("resize pseudo terminal");
        let _ = unsafe { libc::kill(session.child.0.id() as libc::pid_t, libc::SIGWINCH) };
        session.send(b"j", "send resize smoke input");
        session.send(b"e", "open editor");
        let alternate_enters = |bytes: &[u8]| {
            bytes
                .windows(ALTERNATE_ENTER.len())
                .filter(|window| *window == ALTERNATE_ENTER)
                .count()
                >= 2
        };
        session.wait_until(alternate_enters, "editor handoff did not return to TUI");

        // `Terminal::clear` asks a real terminal for the cursor position after re-entering the
        // alternate screen. Answer the same DSR query a terminal emulator would answer.
        session.leave_with(b"qy");
        session.assert_success(WAIT);
        assert!(
            session
                .output
                .windows(ALTERNATE_LEAVE.len())
                .filter(|window| *window == ALTERNATE_LEAVE)
                .count()
                >= 2,
            "alternate screen was not restored after editor handoff and exit: {}",
            String::from_utf8_lossy(&session.output)
        );
        let restored_lflag = Pty::lflag(&session.pty.slave).expect("read restored terminal state");
        let raw_mode_bits = libc::ICANON | libc::ECHO;
        assert_eq!(
            restored_lflag & raw_mode_bits,
            session.pty.initial_lflag & raw_mode_bits,
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

        let mut session = TuiSession::start(dir.path(), &["kanban"], None);
        session.wait_until(
            |bytes| {
                bytes
                    .windows(b"First card".len())
                    .any(|window| window == b"First card")
                    && bytes.windows(b"T-2".len()).any(|window| window == b"T-2")
                    && bytes
                        .windows(b"Second".len())
                        .any(|window| window == b"Second")
            },
            "kanban did not render both cards",
        );

        // Give the popup enough vertical space for both its metadata and body. Move the selection
        // to the second card, then open its detail popup. The body marker is absent from the board
        // card and proves that both the key event and the popup render ran.
        session
            .pty
            .resize(40, 120)
            .expect("resize for details popup");
        let _ = unsafe { libc::kill(session.child.0.id() as libc::pid_t, libc::SIGWINCH) };
        session.send(b"jv", "navigate and open details");
        session.wait_until(
            |bytes| {
                bytes
                    .windows(b"marker".len())
                    .any(|window| window == b"marker")
            },
            "details popup did not render the selected card",
        );

        session.leave_with(b"vq");
        session.assert_success(WAIT);
    }

    #[test]
    fn kanban_pty_startup_filters_compose_without_mutating_the_board() {
        let dir = TempDir::new().expect("temp dir");
        pinto(dir.path()).arg("init").assert().success();
        pinto(dir.path())
            .args(["sprint", "new", "S-1", "Sprint One"])
            .assert()
            .success();
        pinto(dir.path())
            .args([
                "add",
                "Sprint label target",
                "--sprint",
                "S-1",
                "--label",
                "ui",
                "backend",
            ])
            .assert()
            .success();
        pinto(dir.path())
            .args([
                "add",
                "Sprint other label",
                "--sprint",
                "S-1",
                "--label",
                "ops",
            ])
            .assert()
            .success();
        pinto(dir.path())
            .args(["add", "Other sprint target", "--label", "ui"])
            .assert()
            .success();

        let config_path = dir.path().join(".pinto/config.toml");
        let config = std::fs::read_to_string(&config_path).expect("config");
        std::fs::write(
            config_path,
            config.replace("confirm_quit = true", "confirm_quit = false"),
        )
        .expect("disable quit confirmation");

        let mut session = TuiSession::start(
            dir.path(),
            &[
                "kanban",
                "--column",
                "todo",
                "--sprint",
                "S-1",
                "--label",
                "ui",
                "backend",
                "--all-labels",
                "--search",
                "^Sprint label target$",
                "--regex",
            ],
            None,
        );
        session.enter_alternate_screen();
        session.wait_until(
            |bytes| {
                bytes
                    .windows(b"Sprint label target".len())
                    .any(|window| window == b"Sprint label target")
            },
            "filtered card did not render",
        );
        assert!(
            !session
                .output
                .windows(b"Sprint other label".len())
                .any(|window| window == b"Sprint other label"),
            "label filter should exclude the other Sprint card: {}",
            String::from_utf8_lossy(&session.output)
        );
        assert!(
            !session
                .output
                .windows(b"Other sprint target".len())
                .any(|window| window == b"Other sprint target"),
            "Sprint filter should exclude the other Sprint card: {}",
            String::from_utf8_lossy(&session.output)
        );

        session.leave_with(b"q");
        session.assert_success(WAIT);

        let items = json_stdout(pinto(dir.path()).args(["list", "--json"]));
        assert_eq!(items.as_array().expect("list array").len(), 3);
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
        let mut session = TuiSession::start(dir.path(), &["shell"], None);
        let prompt = |bytes: &[u8], count| {
            bytes
                .windows(b"pinto> ".len())
                .filter(|window| *window == b"pinto> ")
                .count()
                >= count
        };
        session.wait_until(|bytes| prompt(bytes, 1), "shell prompt did not appear");

        session.send(b"k\r", "enter first kanban");
        session.wait_until(
            |bytes| {
                bytes
                    .windows(ALTERNATE_ENTER.len())
                    .filter(|window| *window == ALTERNATE_ENTER)
                    .count()
                    >= 1
            },
            "first kanban did not start",
        );
        session.send(b"Q", "return from first kanban");
        session.wait_until(|bytes| prompt(bytes, 2), "shell prompt did not return");

        session.send(b"k\r", "enter second kanban");
        session.wait_until(
            |bytes| {
                bytes
                    .windows(ALTERNATE_ENTER.len())
                    .filter(|window| *window == ALTERNATE_ENTER)
                    .count()
                    >= 2
            },
            "second kanban did not start",
        );
        session.send(b"Q", "return from second kanban");
        session.wait_until(
            |bytes| prompt(bytes, 3),
            "shell prompt did not return a second time",
        );

        session.send(b"\x04", "exit shell");
        session.assert_success(SHELL_EXIT_WAIT);
        assert!(
            session
                .output
                .windows(ALTERNATE_LEAVE.len())
                .filter(|window| *window == ALTERNATE_LEAVE)
                .count()
                >= 2,
            "both Kanban sessions must leave alternate screen"
        );
        let restored_lflag = Pty::lflag(&session.pty.slave).expect("read restored terminal state");
        let raw_mode_bits = libc::ICANON | libc::ECHO;
        assert_eq!(
            restored_lflag & raw_mode_bits,
            session.pty.initial_lflag & raw_mode_bits,
            "shell input terminal flags were not restored"
        );
    }
}
