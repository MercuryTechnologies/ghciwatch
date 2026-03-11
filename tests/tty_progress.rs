use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use expectrl::session::Session;
use expectrl::Expect;
use expectrl::Regex;

// --- Helpers ----------------------------------------------------------------

const MODULES_OK: &str = r#"            echo "[1 of 3] Compiling MyLib ( src/MyLib.hs, interpreted )"
            echo "[2 of 3] Compiling MyModule ( src/MyModule.hs, interpreted )"
            echo "[3 of 3] Compiling TestMain ( test/TestMain.hs, interpreted )"
            echo "Ok, 3 modules loaded."
"#;

/// Generate a fake GHCi shell script with a custom `:set prompt` response.
/// The version banner, `:show paths`, `:show targets`, and `:quit` handlers
/// are always included.
fn write_fake_ghci(dir: &Path, set_prompt_response: &str) -> PathBuf {
    let path = dir.join("fake-ghci.sh");
    let script = format!(
        r#"#!/bin/sh
PROMPT="ghci> "
CWD="$(pwd)"
echo "GHCi, version 9.8.4: https://www.haskell.org/ghc/  :? for help"
while IFS= read -r cmd; do
    case "$cmd" in
        *"set prompt"*)
            PROMPT=$(echo "$cmd" | sed 's/:set prompt\(-cont\)\{{0,1\}} //')
{set_prompt_response}
            printf "%s" "$PROMPT"
            ;;
        *"show paths"*)
            echo "current working directory:"
            echo "  $CWD"
            echo "module import search paths:"
            echo "  src"
            printf "%s" "$PROMPT"
            ;;
        *"show targets"*)
            echo "MyLib"
            echo "MyModule"
            printf "%s" "$PROMPT"
            ;;
        *"quit"*)
            echo "Leaving GHCi."
            exit 0
            ;;
        *)
            printf "%s" "$PROMPT"
            ;;
    esac
done
"#
    );
    std::fs::write(&path, script).expect("can write fake ghci script");
    path
}

fn ghciwatch_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ghciwatch"))
}

fn project_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/simple")
}

fn create_test_dir(test_name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(test_name);
    std::fs::create_dir_all(&dir).expect("can create test dir");
    dir
}

/// Build a ghciwatch `Command` in progress mode using a fake GHCi script.
/// Covers the common case; tests that need different args (e.g. `--log-filter warn`,
/// `stty` wrapper) build their own command.
fn ghciwatch_cmd(fake_ghci: &Path, log_path: &Path) -> Command {
    let mut cmd = Command::new(ghciwatch_bin());
    cmd.args(["--experimental-features", "progress"])
        .args(["--command", &format!("sh {}", fake_ghci.display())])
        .args(["--watch", "src"])
        .arg("--log-json")
        .arg(log_path)
        .args(["--log-filter", "ghciwatch=debug"])
        .current_dir(project_dir());
    cmd
}

/// Spawn a command inside a PTY allocated by the `script` utility.
/// Stdio is nulled so `script` doesn't try to manipulate the parent terminal
/// (which may be a pipe when running under cargo test).
fn spawn_script_pty(
    cmdline: &str,
    output_file: &Path,
    cwd: &Path,
    envs: &[(&str, &str)],
) -> std::process::Child {
    // macOS: script -q <output> <command> [args...]
    // Linux: script -q -c <command> <output>
    let mut cmd = Command::new("script");
    if cfg!(target_os = "macos") {
        cmd.args(["-q", &output_file.to_string_lossy()])
            .args(["/bin/sh", "-c", cmdline]);
    } else {
        cmd.args(["-q", "-c", cmdline]).arg(output_file);
    }
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for &(k, v) in envs {
        cmd.env(k, v);
    }
    cmd.spawn().expect("can spawn script")
}

/// Copy `tests/data/simple` into a temp dir and write an empty cabal config
/// so `cabal repl` doesn't try to access the network.
fn setup_real_ghc_project(temp_root: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(temp_root);
    std::fs::create_dir_all(temp_root).expect("can create temp root");

    let cabal_dir = temp_root.join(".cabal");
    std::fs::create_dir_all(&cabal_dir).expect("can create .cabal dir");
    std::fs::write(cabal_dir.join("config"), "").expect("can write empty cabal config");

    let mut opts = fs_extra::dir::CopyOptions::new();
    opts.overwrite = true;
    fs_extra::dir::copy(project_dir(), temp_root, &opts).expect("can copy project to temp dir");
    temp_root.join("simple")
}

/// Poll a file until it contains `needle`, returning true if found before `timeout`.
fn poll_log_for(path: &Path, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if contents.contains(needle) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

// --- Tests ------------------------------------------------------------------

/// Spawn ghciwatch in a real PTY so that `stdout().is_terminal()` returns true.
/// Verifies that `--experimental-features progress` activates progress rendering
/// and that the progress indicator text appears on the terminal.
#[test]
fn progress_renders_in_tty() {
    let test_dir = create_test_dir("tty-progress-renders");
    let log_path = test_dir.join("ghciwatch.json");
    let fake = write_fake_ghci(&test_dir, MODULES_OK);

    let mut session =
        Session::spawn(ghciwatch_cmd(&fake, &log_path)).expect("can spawn ghciwatch in PTY");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Regex matches through surrounding ANSI escape sequences (\r\x1b[2K).
    session
        .expect(Regex(r"\[1/3\] Compiling MyLib"))
        .expect("should see first progress indicator on TTY");
    session
        .expect(Regex(r"\[3/3\] Compiling TestMain"))
        .expect("should see last progress indicator on TTY");
    session
        .expect(Regex(r"Ok, 3 modules loaded"))
        .expect("summary line should appear on TTY");

    std::thread::sleep(Duration::from_millis(500));

    let log_contents =
        std::fs::read_to_string(&log_path).expect("can read JSON log after PTY test");
    assert!(
        log_contents.contains("Compilation progress"),
        "JSON log should contain 'Compilation progress' trace from ProgressWriter"
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Verify that progress mode does NOT activate when stdout is not a TTY.
/// Spawns ghciwatch with piped (non-PTY) stdio and checks the JSON log.
#[test]
fn progress_falls_back_without_tty() {
    let test_dir = create_test_dir("tty-progress-fallback");
    let log_path = test_dir.join("ghciwatch.json");
    let fake = write_fake_ghci(&test_dir, MODULES_OK);

    let mut child = ghciwatch_cmd(&fake, &log_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("can spawn ghciwatch");

    std::thread::sleep(Duration::from_secs(3));

    let _ = child.kill();
    let _ = child.wait();

    // Without a TTY, ProgressWriter's render_progress is false, so the
    // "Compilation progress" debug trace should NOT appear.
    let log_contents =
        std::fs::read_to_string(&log_path).expect("can read JSON log after non-TTY test");
    assert!(
        !log_contents.contains("Compilation progress"),
        "progress mode should NOT be active without a TTY; log contains: {}",
        &log_contents[..log_contents.len().min(500)]
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

/// End-to-end test with a **real** GHC and `cabal repl` inside a PTY.
///
/// Skips when `$GHC_VERSIONS` is not set (e.g. outside the nix devshell).
/// Uses the `script` utility to allocate the PTY, avoiding process-group
/// interactions that `ptyprocess` has with ghciwatch's child pipe chain.
#[test]
fn progress_renders_in_tty_real_ghc() {
    let ghc_version = match option_env!("GHC_VERSIONS") {
        Some(v) => match v.split_whitespace().next() {
            Some(ver) => ver,
            None => {
                eprintln!("skipping: $GHC_VERSIONS is empty");
                return;
            }
        },
        None => {
            eprintln!("skipping: $GHC_VERSIONS not set");
            return;
        }
    };

    let test_dir = create_test_dir("tty-progress-real-ghc");
    let log_path = test_dir.join("ghciwatch.json");
    let cwd = setup_real_ghc_project(&test_dir);
    let repl_command = format!("make ghci GHC=ghc-{ghc_version}");

    let ghciwatch_cmdline = format!(
        "{bin} --experimental-features progress \
         --command {repl:?} \
         --watch src --watch package.yaml \
         --restart-glob '**/package.yaml' \
         --before-startup-shell 'hpack --force .' \
         --log-json {log} \
         --log-filter ghciwatch=debug \
         --poll 1000ms",
        bin = ghciwatch_bin().display(),
        repl = repl_command,
        log = log_path.display(),
    );

    let mut child = spawn_script_pty(
        &ghciwatch_cmdline,
        &test_dir.join("script.out"),
        &cwd,
        &[
            ("HOME", &test_dir.to_string_lossy()),
            ("GHC_NO_UNICODE", "1"),
        ],
    );

    // "Compilation progress" is only emitted when ProgressWriter is active (PTY).
    let found = poll_log_for(&log_path, "Compilation progress", Duration::from_secs(120));

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        found,
        "JSON log should contain 'Compilation progress' from ProgressWriter with real GHC \
         (timed out after 120s; log at {})",
        log_path.display()
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Verify that the progress indicator is cleared before error output appears.
/// When a non-progress line arrives after progress was active, `ProgressWriter`
/// emits `\r\x1b[2K` to erase the indicator, then forwards the error text.
#[test]
fn progress_clears_before_error() {
    let test_dir = create_test_dir("tty-progress-clears-error");
    let log_path = test_dir.join("ghciwatch.json");

    let fake = write_fake_ghci(
        &test_dir,
        r#"            echo "[1 of 3] Compiling MyLib ( src/MyLib.hs, interpreted )"
            echo "[2 of 3] Compiling MyModule ( src/MyModule.hs, interpreted )"
            echo ""
            echo "src/MyModule.hs:4:11: error:"
            echo "    Type mismatch"
            echo "Failed, one module loaded."
"#,
    );

    let mut session =
        Session::spawn(ghciwatch_cmd(&fake, &log_path)).expect("can spawn ghciwatch in PTY");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    session
        .expect(Regex(r"\[1/3\] Compiling MyLib"))
        .expect("should see progress indicator before error");

    // Error text appearing on the PTY proves the progress line was cleared
    // (they can't coexist on the same terminal line) and the error was
    // forwarded through the inner writer.
    session
        .expect(Regex(r"src/MyModule\.hs:4:11: error:"))
        .expect("error output should appear on TTY after progress clears");
    session
        .expect(Regex(r"Failed, one module loaded"))
        .expect("compilation summary should appear on TTY");

    let _ = std::fs::remove_dir_all(&test_dir);
}

/// Verify that progress lines are truncated to the PTY's terminal width.
/// Uses `stty` to set the PTY to 40 columns before ghciwatch starts, then
/// checks that a long module name is cut to fit.
#[test]
fn progress_truncates_to_terminal_width() {
    let test_dir = create_test_dir("tty-progress-truncation");
    let log_path = test_dir.join("ghciwatch.json");

    let fake = write_fake_ghci(
        &test_dir,
        r#"            echo "[1 of 1] Compiling VeryLongModuleName.That.Exceeds.Width ( src/VeryLong.hs, interpreted )"
            echo "Ok, 1 module loaded."
"#,
    );

    // stty sets the PTY winsize to 40 columns; crossterm::terminal::size()
    // returns (40, 24), and render_progress truncates to width-1 = 39 chars.
    // log-filter=warn suppresses debug traces from stderr that would include
    // the full raw module name and pollute the PTY stream we assert on.
    let shell_cmd = format!(
        "stty columns 40 rows 24 2>/dev/null; \
         exec {bin} --experimental-features progress \
         --command 'sh {fake}' \
         --watch src \
         --log-json {log} \
         --log-filter ghciwatch=warn",
        bin = ghciwatch_bin().display(),
        fake = fake.display(),
        log = log_path.display(),
    );

    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(&shell_cmd).current_dir(project_dir());

    let mut session = Session::spawn(cmd).expect("can spawn ghciwatch with narrow PTY");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Full rendered line: "[1/1] Compiling VeryLongModuleName.That.Exceeds.Width" (52 chars).
    // At 40 columns it should be truncated to 39.
    session
        .expect(Regex(r"\[1/1\] Compiling VeryLong"))
        .expect("should see truncated progress indicator on narrow PTY");

    // The full untruncated text should NOT appear on the PTY.
    std::thread::sleep(Duration::from_millis(500));
    let mut buf = vec![0u8; 8192];
    let n = session.try_read(&mut buf).unwrap_or(0);
    let remaining = String::from_utf8_lossy(&buf[..n]);
    assert!(
        !remaining.contains("Exceeds.Width"),
        "full module name should be truncated, but found it in PTY output: {remaining:?}"
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}
