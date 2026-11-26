use std::path::PathBuf;

use spy_compiler::{
    build_file, check_file, inspect_environment, run_file, BuildOptions,
};
use spy_runtime::{probe_cpython_c_embedding, CompatibilityStatus};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn inspects_host_environment_and_detects_toolchains() {
    let report = inspect_environment();
    assert!(
        report.rust.rustc.is_some(),
        "rustc must be detected in PATH or ~/.cargo/bin"
    );
    assert!(
        report.rust.cargo.is_some(),
        "cargo must be detected in PATH or ~/.cargo/bin"
    );
    assert!(
        !report.python_reports.is_empty(),
        "at least one Python installation must be detected"
    );
}

#[test]
fn checks_builds_and_runs_hello_example() {
    let root = workspace_root();
    let hello = root.join("examples/hello.rs");

    check_file(&hello, None).expect("hello.rs should pass spy check");

    let out_bin = std::env::temp_dir().join(format!("spy-hello-test-{}", std::process::id()));
    let build_res = build_file(
        &hello,
        &BuildOptions {
            release: false,
            output: Some(out_bin.clone()),
            edition: None,
        },
    )
    .expect("hello.rs should compile");
    assert!(build_res.output_binary.is_file());
    let _ = std::fs::remove_file(&out_bin);

    let exit_code = run_file(
        &hello,
        &BuildOptions::default(),
        &["arg1".to_owned(), "arg2".to_owned()],
    )
    .expect("hello.rs should run");
    assert_eq!(exit_code, 0);
}

#[test]
fn verifies_cpython_c_embedding_when_compatible_python_exists() {
    let report = inspect_environment();
    let Some(c_tool) = &report.native.c_compiler else {
        return;
    };
    let Some(idx) = report.preferred_python_index else {
        return;
    };
    let best = &report.python_reports[idx];
    if matches!(
        best.status,
        CompatibilityStatus::Ready | CompatibilityStatus::ReadyWithForwardCompat
    ) {
        let probe = probe_cpython_c_embedding(&best.python, &c_tool.path)
            .expect("compatible Python with headers and libpython should embed cleanly");
        assert!(probe.embedded_version_output.starts_with('3'));
    }
}
