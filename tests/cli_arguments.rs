//! CLI diagnostics and exit behavior.
#![cfg(feature = "std")]

use std::process::Command;

#[test]
fn help_succeeds_and_invalid_argument_counts_fail() {
    for option in ["--help", "-h"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ahx"))
            .arg(option)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
    for args in [&[][..], &["input"][..], &["input", "output", "extra"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_ahx"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
}
