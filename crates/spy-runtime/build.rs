use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");
    println!("cargo:rerun-if-env-changed=DEP_PYO3_PYTHON_PYO3_CONFIG");

    if env::var_os("CARGO_FEATURE_EMBEDDED").is_none() {
        return;
    }

    #[cfg(target_family = "unix")]
    if let Ok(hex_cfg) = env::var("DEP_PYO3_PYTHON_PYO3_CONFIG")
        && let Some(cfg_text) = decode_hex(&hex_cfg)
    {
        let mut lib_dir: Option<String> = None;
        let mut lib_name: Option<String> = None;
        let mut executable: Option<String> = None;

        for line in cfg_text.lines() {
            if let Some(v) = line.strip_prefix("lib_dir=") {
                lib_dir = Some(v.trim().to_owned());
            } else if let Some(v) = line.strip_prefix("lib_name=") {
                lib_name = Some(v.trim().to_owned());
            } else if let Some(v) = line.strip_prefix("executable=") {
                executable = Some(v.trim().to_owned());
            }
        }

        if let Some(dir) = lib_dir.as_deref()
            && !dir.is_empty()
        {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");

            // If LIBDIR lacks the unversioned `libpythonX.Y.so` (common on Debian/Ubuntu
            // without `python3-dev`), fall back to Python's `LIBPL` directory if present.
            if let Some(name) = lib_name.as_deref() {
                let candidate = Path::new(dir).join(format!("lib{name}.so"));
                if !candidate.is_file()
                    && let Some(exe) = executable.as_deref()
                    && let Some(lib_pl) = query_libpl(exe)
                {
                    println!("cargo:rustc-link-search=native={lib_pl}");
                    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_pl}");
                }
            }
        }
    }
}

#[cfg(target_family = "unix")]
fn query_libpl(executable: &str) -> Option<String> {
    let out = Command::new(executable)
        .args([
            "-c",
            "import sysconfig; print(sysconfig.get_config_var('LIBPL') or '')",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

#[cfg(target_family = "unix")]
fn decode_hex(hex: &str) -> Option<String> {
    let bytes = hex.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let hi = (chunk[0] as char).to_digit(16)? as u8;
        let lo = (chunk[1] as char).to_digit(16)? as u8;
        out.push((hi << 4) | lo);
    }
    String::from_utf8(out).ok()
}
