//! Installation and byte-for-byte checks for the bundled se(1) manual.

use std::process::Command;

const MAN_PAGE: &[u8] = include_bytes!("../man/se.1");

#[test]
fn print_man_writes_the_packaged_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg("--print-man")
        .output()
        .expect("run se --print-man");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, MAN_PAGE);
    assert!(output.stderr.is_empty());
}

#[test]
fn install_man_creates_and_replaces_the_manual() {
    let root = tempfile::tempdir().expect("create temporary directory");
    let man1 = root.path().join("share/man/man1");
    let destination = man1.join("se.1");
    let option = format!("--install-man={}", man1.display());

    let first = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg(&option)
        .output()
        .expect("install se(1)");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        first.stdout,
        format!("{}\n", destination.display()).as_bytes()
    );
    assert!(first.stderr.is_empty());
    assert_eq!(
        std::fs::read(&destination).expect("read installed se.1"),
        MAN_PAGE
    );

    std::fs::write(&destination, b"stale manual\n").expect("replace manual with stale data");
    let second = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg(&option)
        .output()
        .expect("replace se(1)");
    assert!(
        second.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        std::fs::read(&destination).expect("read replaced se.1"),
        MAN_PAGE
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&destination)
            .expect("stat installed se.1")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
    }
}

#[test]
fn install_man_uses_the_user_data_directory() {
    let data_home = tempfile::tempdir().expect("create temporary data directory");
    let destination = data_home.path().join("man/man1/se.1");
    let output = Command::new(env!("CARGO_BIN_EXE_se"))
        .arg("--install-man")
        .env("XDG_DATA_HOME", data_home.path())
        .env_remove("HOME")
        .output()
        .expect("install se(1) under XDG_DATA_HOME");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        format!("{}\n", destination.display()).as_bytes()
    );
    assert_eq!(
        std::fs::read(destination).expect("read user-installed se.1"),
        MAN_PAGE
    );
}
