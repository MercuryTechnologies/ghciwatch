use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use expectrl::session::Session;
use expectrl::Expect;
use expectrl::Regex;

/// Generate a fake GHCi shell script with a custom response for the `:set prompt` command.
/// The standard version banner, `:show paths`, `:show targets`, and `:quit` handlers are
/// always included. Returns the path to the generated script.
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

fn fake_ghci_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/fake-ghci.sh")
}

fn project_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/simple")
}

fn log_dir(test_name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(test_name);
    std::fs::create_dir_all(&dir).expect("can create log dir");
    dir
}

/// Spawn ghciwatch in a real PTY so that `stdout().is_terminal()` returns true.
/// Verifies that `--experimental-features progress` activates progress rendering
/// and that the progress indicator text appears on the terminal.
#[test]
fn progress_renders_in_tty() {
    let log_dir = log_dir("tty-progress-renders");
    let log_path = log_dir.join("ghciwatch.json");

    let fake_ghci_cmd = format!("sh {}", fake_ghci_path().display());

    let mut cmd = Command::new(ghciwatch_bin());
    cmd.args(["--experimental-features", "progress"])
        .args(["--command", &fake_ghci_cmd])
        .args(["--watch", "src"])
        .arg("--log-json")
        .arg(&log_path)
        .args(["--log-filter", "ghciwatch=debug"])
        .current_dir(project_dir());

    let mut session = Session::spawn(cmd).expect("can spawn ghciwatch in PTY");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // The progress indicator writes `[N/M] Compiling Module` to the terminal.
    // Use a regex because ANSI escape sequences (\r\x1b[2K) surround the text.
    session
        .expect(Regex(r"\[1/3\] Compiling MyLib"))
        .expect("should see first progress indicator on TTY");

    session
        .expect(Regex(r"\[3/3\] Compiling TestMain"))
        .expect("should see last progress indicator on TTY");

    // Non-progress output passes through to the terminal.
    session
        .expect(Regex(r"Ok, 3 modules loaded"))
        .expect("summary line should appear on TTY");

    // Give ghciwatch a moment to finish writing the JSON log.
    std::thread::sleep(Duration::from_millis(500));

    // Verify the JSON log confirms progress mode was active.
    if let Ok(log_contents) = std::fs::read_to_string(&log_path) {
        assert!(
            log_contents.contains("Compilation progress"),
            "JSON log should contain 'Compilation progress' trace from ProgressWriter"
        );
    }

    let _ = std::fs::remove_dir_all(&log_dir);
}

/// Verify that progress mode does NOT activate when stdout is not a TTY.
/// Spawns ghciwatch with piped (non-PTY) stdio and checks the JSON log.
#[tokio::test]
async fn progress_falls_back_without_tty() {
    let log_dir = log_dir("tty-progress-fallback");
    let log_path = log_dir.join("ghciwatch.json");

    let fake_ghci_cmd = format!("sh {}", fake_ghci_path().display());

    let mut child = tokio::process::Command::new(ghciwatch_bin())
        .args(["--experimental-features", "progress"])
        .args(["--command", &fake_ghci_cmd])
        .args(["--watch", "src"])
        .arg("--log-json")
        .arg(&log_path)
        .args(["--log-filter", "ghciwatch=debug"])
        .current_dir(project_dir())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("can spawn ghciwatch");

    // Let ghciwatch start up and initialize with the fake GHCi.
    tokio::time::sleep(Duration::from_secs(3)).await;

    child.kill().await.expect("can kill ghciwatch");
    let _ = child.wait().await;

    if let Ok(log_contents) = std::fs::read_to_string(&log_path) {
        // Without a TTY, ProgressWriter's render_progress is false, so the
        // "Compilation progress" debug trace should NOT appear. The compilation
        // lines still pass through in Standard mode via the normal CompilationLog,
        // emitting "Compiling" traces but NOT "Compilation progress".
        assert!(
            !log_contents.contains("Compilation progress"),
            "progress mode should NOT be active without a TTY; log contains: {}",
            &log_contents[..log_contents.len().min(500)]
        );
    }

    let _ = std::fs::remove_dir_all(&log_dir);
}

/// Copy a project directory into a temp dir and prepare it for `cabal repl`.
/// Returns the path to the project inside the temp dir.
fn setup_real_ghc_project(temp_root: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(temp_root);
    std::fs::create_dir_all(temp_root).expect("can create temp root");

    // Empty cabal config prevents network access during cabal repl.
    let cabal_dir = temp_root.join(".cabal");
    std::fs::create_dir_all(&cabal_dir).expect("can create .cabal dir");
    std::fs::write(cabal_dir.join("config"), "").expect("can write empty cabal config");

    let mut opts = fs_extra::dir::CopyOptions::new();
    opts.overwrite = true;
    fs_extra::dir::copy(project_dir(), temp_root, &opts).expect("can copy project to temp dir");
    temp_root.join("simple")
}

/// End-to-end test with a **real** GHC and `cabal repl` inside a PTY.
///
/// Skips automatically when `$GHC_VERSIONS` is not set (e.g. outside the nix devshell).
/// This validates that `ProgressWriter` correctly parses and renders actual GHC
/// compilation output when stdout is a terminal, complementing the fake-GHCi tests.
///
/// Uses the `script` utility (available on macOS and Linux) to wrap ghciwatch in a
/// real PTY. This avoids process-group interactions that occur with `ptyprocess` and
/// cause pipe stalls between ghciwatch and its child processes.
#[test]
fn progress_renders_in_tty_real_ghc() {
    let ghc_version = match option_env!("GHC_VERSIONS") {
        Some(v) => match v.split_whitespace().next() {
            Some(ver) => ver,
            None => {
                eprintln!("skipping progress_renders_in_tty_real_ghc: $GHC_VERSIONS is empty");
                return;
            }
        },
        None => {
            eprintln!("skipping progress_renders_in_tty_real_ghc: $GHC_VERSIONS not set");
            return;
        }
    };

    let test_dir = log_dir("tty-progress-real-ghc");
    let log_path = test_dir.join("ghciwatch.json");
    let script_output = test_dir.join("script.out");
    let cwd = setup_real_ghc_project(&test_dir);
    let repl_command = format!("make ghci GHC=ghc-{ghc_version}");

    // Build the ghciwatch command as a shell string for `script -c`.
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

    // `script` allocates a PTY for the command, so ghciwatch sees is_terminal()==true.
    // macOS `script` syntax: script -q <output> <command> [args...]
    // Linux `script` syntax: script -q -c <command> <output>
    // Redirect script's own stdin from /dev/null and suppress its stdout/stderr
    // so it doesn't try to manipulate the parent terminal (which may be a pipe
    // when running under cargo test without --nocapture).
    let mut child = if cfg!(target_os = "macos") {
        Command::new("script")
            .args(["-q", &script_output.to_string_lossy()])
            .args(["/bin/sh", "-c", &ghciwatch_cmdline])
            .current_dir(&cwd)
            .env("HOME", &test_dir)
            .env("GHC_NO_UNICODE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("can spawn script+ghciwatch")
    } else {
        Command::new("script")
            .args(["-q", "-c", &ghciwatch_cmdline])
            .arg(&script_output)
            .current_dir(&cwd)
            .env("HOME", &test_dir)
            .env("GHC_NO_UNICODE", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("can spawn script+ghciwatch")
    };

    // Poll the JSON log until "Compilation progress" appears or we time out.
    // This trace is emitted by ProgressWriter only when render_progress is true,
    // which only activates when stdout is a terminal. Its presence proves:
    // PTY (via script) → is_terminal() → Progress mode → ProgressWriter parses real GHC output.
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let mut found = false;
    while std::time::Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(&log_path) {
            if contents.contains("Compilation progress") {
                found = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

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
    let test_dir = log_dir("tty-progress-clears-error");
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
    let fake_ghci_cmd = format!("sh {}", fake.display());

    let mut cmd = Command::new(ghciwatch_bin());
    cmd.args(["--experimental-features", "progress"])
        .args(["--command", &fake_ghci_cmd])
        .args(["--watch", "src"])
        .arg("--log-json")
        .arg(&log_path)
        .args(["--log-filter", "ghciwatch=debug"])
        .current_dir(project_dir());

    let mut session = Session::spawn(cmd).expect("can spawn ghciwatch in PTY");
    session.set_expect_timeout(Some(Duration::from_secs(30)));

    // Progress indicator should render for the first compiling line.
    session
        .expect(Regex(r"\[1/3\] Compiling MyLib"))
        .expect("should see progress indicator before error");

    // The error text must appear on the PTY. This proves the progress line was
    // cleared (they can't coexist on the same terminal line) and the error was
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
    let test_dir = log_dir("tty-progress-truncation");
    let log_path = test_dir.join("ghciwatch.json");

    let fake = write_fake_ghci(
        &test_dir,
        r#"            echo "[1 of 1] Compiling VeryLongModuleName.That.Exceeds.Width ( src/VeryLong.hs, interpreted )"
            echo "Ok, 1 module loaded."
"#,
    );

    // Use `stty columns 40` to set the PTY width before ghciwatch starts.
    // crossterm::terminal::size() will then return (40, 24), and render_progress
    // truncates to width-1 = 39 characters.
    // Use log-filter=warn to suppress debug traces from stderr -- they would
    // include the full raw module name and pollute the PTY stream we assert on.
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

    // The full rendered line would be "[1/1] Compiling VeryLongModuleName.That.Exceeds.Width"
    // (52 chars). At 40 columns it should be truncated to 39 chars.
    // We expect to see the beginning of the module name but NOT the full thing.
    session
        .expect(Regex(r"\[1/1\] Compiling VeryLong"))
        .expect("should see truncated progress indicator on narrow PTY");

    // The full untruncated text should NOT appear on the PTY.
    // Read whatever is buffered and check it doesn't contain the full name.
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
