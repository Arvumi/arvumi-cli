use std::process::{Command, Output};

fn run(args: &[&str], cwd: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arvumi"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run arvumi")
}

#[test]
fn help_and_version_work_without_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let version = run(&["--version"], dir.path());
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("arvumi {}", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        vec!["--help"],
        vec!["login", "--help"],
        vec!["db", "connect", "--help"],
        vec!["environments", "create", "--help"],
    ] {
        let output = run(&args, dir.path());
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn linking_is_local_and_invalid_commands_fail() {
    let dir = tempfile::tempdir().unwrap();
    let project = "a8c9199b-69df-4cec-9b97-933159780d60";
    assert!(run(&["link", project], dir.path()).status.success());
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".arvumi")).unwrap(),
        project
    );
    for args in [
        vec!["not-a-command"],
        vec!["env", "list"],
        vec![
            "environments",
            "create",
            "--name",
            "staging",
            "--branch",
            "develop",
        ],
    ] {
        assert_eq!(run(&args, dir.path()).status.code(), Some(2));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn config_hides_credentials_and_init_protects_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let config_root = dir.path().join("config");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_arvumi"))
            .args(args)
            .env("XDG_CONFIG_HOME", &config_root)
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(!run(&["config"]).status.success());
    // Dummy non-ASCII key exercises the old UTF-8 slicing panic as well as redaction.
    let key = "短い鍵";
    assert!(
        run(&["init", "--api", "http://127.0.0.1:1", "--key", key])
            .status
            .success()
    );
    let file = config_root.join("arvumi/config.json");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let output = run(&["config"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("API Key: configured"));
    assert!(!stdout.contains(key));
}
