//! Smoke test for the `openjill-jn-view` binary. The viewer itself is an
//! interactive egui GUI that cannot run headless in CI, so this test only
//! verifies that the binary links and parses arguments via `--help`.

use std::process::Command;

#[test]
fn help_runs_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_openjill-jn-view"))
        .arg("--help")
        .output()
        .expect("jn-view binary should run");
    assert!(output.status.success(), "--help should exit successfully");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("--file"),
        "help should document the --file flag"
    );
}
