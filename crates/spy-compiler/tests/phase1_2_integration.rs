use std::path::PathBuf;
use std::process::Command;

use spy_compiler::{build_file, check_file, run_file, BuildOptions};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn checks_builds_and_runs_python_imports_example() {
    let root = workspace_root();
    let example = root.join("examples/python_imports.rs");

    let artifact = check_file(&example, None).expect("python_imports.rs should pass check");
    assert!(artifact.requires_python_runtime);
    assert_eq!(artifact.python_imports.len(), 4);

    let out_bin = std::env::temp_dir().join(format!("spy-py-imports-{}", std::process::id()));
    let build_res = build_file(
        &example,
        &BuildOptions {
            release: false,
            output: Some(out_bin.clone()),
            edition: None,
        },
    )
    .expect("python_imports.rs should build");
    assert!(build_res.output_binary.is_file());

    let direct_out = Command::new(&out_bin)
        .output()
        .expect("built SPY executable should run directly via embedded rpath");
    assert!(
        direct_out.status.success(),
        "direct run failed: {}",
        String::from_utf8_lossy(&direct_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&direct_out.stdout);
    assert!(stdout.contains("math.sqrt(144) = 12"));
    assert!(stdout.contains("Counter['rust'] = 2"));
    assert!(stdout.contains("json.dumps = [10, 20, 30]"));
    assert!(stdout.contains("native_sum = 499999500000"));
    let _ = std::fs::remove_file(&out_bin);

    let code = run_file(&example, &BuildOptions::default(), &[]).expect("run_file should work");
    assert_eq!(code, 0);
}

#[test]
fn reports_helpful_import_error_for_missing_python_package() {
    let temp_src = std::env::temp_dir().join(format!("spy-bad-import-{}.rs", std::process::id()));
    let src = r#"
use python::spy_nonexistent_package_999 as missing;

fn main() {
    let _ = missing;
}
"#;
    std::fs::write(&temp_src, src).unwrap();

    let out_bin = std::env::temp_dir().join(format!("spy-bad-import-bin-{}", std::process::id()));
    build_file(
        &temp_src,
        &BuildOptions {
            release: false,
            output: Some(out_bin.clone()),
            edition: None,
        },
    )
    .expect("file with valid syntax should compile");

    let run_out = Command::new(&out_bin).output().unwrap();
    assert!(!run_out.status.success());
    let stderr = String::from_utf8_lossy(&run_out.stderr);
    assert!(stderr.contains("spy_nonexistent_package_999"));
    assert!(stderr.contains("hint:"));

    let _ = std::fs::remove_file(&temp_src);
    let _ = std::fs::remove_file(&out_bin);
}
