#!/usr/bin/env python3
"""Reproducible benchmark suite comparing CPython 3.15 and SPY (Phases 0-2)."""

from __future__ import annotations

import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = Path(__file__).resolve().parent / "cases"
PYTHON_EXE = Path("/home/xagi/.local/bin/python3.15")
if not PYTHON_EXE.exists():
    PYTHON_EXE = Path(sys.executable)


def run_measured(cmd: list[str], env: dict[str, str] | None = None) -> tuple[float, int, str]:
    """Runs cmd in a child process and returns (wall_ms, peak_rss_kib, stdout)."""
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)

    stdout_r, stdout_w = os.pipe()
    stderr_r, stderr_w = os.pipe()
    t0 = time.perf_counter_ns()
    pid = os.fork()
    if pid == 0:
        try:
            os.dup2(stdout_w, 1)
            os.dup2(stderr_w, 2)
            os.close(stdout_r)
            os.close(stdout_w)
            os.close(stderr_r)
            os.close(stderr_w)
            os.execvpe(cmd[0], cmd, merged_env)
        except Exception as exc:  # pragma: no cover
            os.write(2, f"exec failed: {exc}\n".encode())
            os._exit(127)

    os.close(stdout_w)
    os.close(stderr_w)

    out_chunks: list[bytes] = []
    err_chunks: list[bytes] = []
    while True:
        chunk = os.read(stdout_r, 65536)
        if not chunk:
            break
        out_chunks.append(chunk)
    while True:
        chunk = os.read(stderr_r, 65536)
        if not chunk:
            break
        err_chunks.append(chunk)
    os.close(stdout_r)
    os.close(stderr_r)

    _, status, rusage = os.wait4(pid, 0)
    t1 = time.perf_counter_ns()

    stdout_text = b"".join(out_chunks).decode("utf-8", errors="replace").strip()
    stderr_text = b"".join(err_chunks).decode("utf-8", errors="replace").strip()
    exit_code = os.waitstatus_to_exitcode(status)
    if exit_code != 0:
        raise RuntimeError(f"Command {cmd} failed ({exit_code}): {stderr_text}")

    wall_ms = (t1 - t0) / 1_000_000.0
    return wall_ms, int(rusage.ru_maxrss), stdout_text


def bench_series(
    label: str,
    cmd: list[str],
    warmups: int,
    runs: int,
    env: dict[str, str] | None = None,
) -> dict[str, object]:
    for _ in range(warmups):
        run_measured(cmd, env)

    times_ms: list[float] = []
    rss_kib: list[int] = []
    last_out = ""
    for _ in range(runs):
        wall_ms, peak_kib, out = run_measured(cmd, env)
        times_ms.append(wall_ms)
        rss_kib.append(peak_kib)
        last_out = out

    return {
        "label": label,
        "runs": runs,
        "min_ms": round(min(times_ms), 3),
        "median_ms": round(statistics.median(times_ms), 3),
        "mean_ms": round(statistics.mean(times_ms), 3),
        "max_ms": round(max(times_ms), 3),
        "peak_rss_kib": max(rss_kib),
        "median_rss_kib": int(statistics.median(rss_kib)),
        "stdout": last_out,
    }


def main() -> None:
    env = os.environ.copy()
    env["PATH"] = f"{Path.home() / '.cargo/bin'}:{Path.home() / '.local/bin'}:{env.get('PATH', '')}"
    os.environ["PATH"] = env["PATH"]

    # Build release `spy` CLI first.
    subprocess.run(
        ["cargo", "build", "--offline", "--release", "-p", "spy-cli"],
        cwd=ROOT,
        check=True,
    )
    spy_bin = str(ROOT / "target/release/spy")

    with tempfile.TemporaryDirectory(prefix="spy-bench-") as tmp:
        tmp_path = Path(tmp)
        cold_target = tmp_path / "cold-target"
        warm_target = tmp_path / "warm-target"

        # 1. Build & Check benchmarks
        pure_debug = tmp_path / "startup_pure_debug"
        pure_rel = tmp_path / "startup_pure_rel"
        py_cold_rel = tmp_path / "startup_py_cold_rel"
        py_warm_dbg = tmp_path / "startup_py_warm_dbg"
        py_warm_rel = tmp_path / "startup_py_warm_rel"

        # Prime warm target once for warm incremental builds? No: cold build uses cold_target;
        # then first warm_target release build primes warm_target before measuring warm incremental rebuilds.
        build_metrics = []

        t_chk_pure, rss_chk_pure, _ = run_measured([spy_bin, "check", str(CASES / "startup_pure.rs")])
        build_metrics.append({
            "step": "spy check (pure Rust)",
            "wall_ms": round(t_chk_pure, 1),
            "peak_rss_kib": rss_chk_pure,
            "binary_bytes": None,
        })

        t_dbg_pure, rss_dbg_pure, _ = run_measured([spy_bin, "build", str(CASES / "startup_pure.rs"), "-o", str(pure_debug)])
        build_metrics.append({
            "step": "spy build debug (pure Rust)",
            "wall_ms": round(t_dbg_pure, 1),
            "peak_rss_kib": rss_dbg_pure,
            "binary_bytes": pure_debug.stat().st_size,
        })

        t_rel_pure, rss_rel_pure, _ = run_measured([spy_bin, "build", str(CASES / "startup_pure.rs"), "--release", "-o", str(pure_rel)])
        build_metrics.append({
            "step": "spy build --release (pure Rust)",
            "wall_ms": round(t_rel_pure, 1),
            "peak_rss_kib": rss_rel_pure,
            "binary_bytes": pure_rel.stat().st_size,
        })

        t_cold, rss_cold, _ = run_measured(
            [spy_bin, "build", str(CASES / "startup_py.rs"), "--release", "-o", str(py_cold_rel)],
            env={"SPY_TARGET_DIR": str(cold_target)},
        )
        build_metrics.append({
            "step": "spy build --release (Python, cold cache)",
            "wall_ms": round(t_cold, 1),
            "peak_rss_kib": rss_cold,
            "binary_bytes": py_cold_rel.stat().st_size,
        })

        # Warm target cache: prime debug + release once so incremental timings reflect warm edits.
        run_measured(
            [spy_bin, "build", str(CASES / "startup_py.rs"), "-o", str(py_warm_dbg)],
            env={"SPY_TARGET_DIR": str(warm_target)},
        )
        run_measured(
            [spy_bin, "build", str(CASES / "startup_py.rs"), "--release", "-o", str(py_warm_rel)],
            env={"SPY_TARGET_DIR": str(warm_target)},
        )

        t_chk_py, rss_chk_py, _ = run_measured(
            [spy_bin, "check", str(CASES / "startup_py.rs")],
            env={"SPY_TARGET_DIR": str(warm_target)},
        )
        build_metrics.append({
            "step": "spy check (Python, warm cache)",
            "wall_ms": round(t_chk_py, 1),
            "peak_rss_kib": rss_chk_py,
            "binary_bytes": None,
        })

        t_wdbg, rss_wdbg, _ = run_measured(
            [spy_bin, "build", str(CASES / "startup_py.rs"), "-o", str(py_warm_dbg)],
            env={"SPY_TARGET_DIR": str(warm_target)},
        )
        build_metrics.append({
            "step": "spy build debug (Python, warm cache)",
            "wall_ms": round(t_wdbg, 1),
            "peak_rss_kib": rss_wdbg,
            "binary_bytes": py_warm_dbg.stat().st_size,
        })

        t_wrel, rss_wrel, _ = run_measured(
            [spy_bin, "build", str(CASES / "startup_py.rs"), "--release", "-o", str(py_warm_rel)],
            env={"SPY_TARGET_DIR": str(warm_target)},
        )
        build_metrics.append({
            "step": "spy build --release (Python, warm cache)",
            "wall_ms": round(t_wrel, 1),
            "peak_rss_kib": rss_wrel,
            "binary_bytes": py_warm_rel.stat().st_size,
        })

        # Build all runtime benchmark binaries in --release using warm_target
        bins: dict[str, Path] = {}
        for name in ["startup_pure", "startup_py", "compute_loop", "python_calls", "mixed_workload"]:
            out_bin = tmp_path / name
            subprocess.run(
                [spy_bin, "build", str(CASES / f"{name}.rs"), "--release", "-o", str(out_bin)],
                env={**os.environ, "SPY_TARGET_DIR": str(warm_target)},
                check=True,
                stdout=subprocess.DEVNULL,
            )
            bins[name] = out_bin

        # 2. Runtime benchmarks
        startup_results = [
            bench_series("SPY (pure Rust)", [str(bins["startup_pure"])], warmups=3, runs=15),
            bench_series("SPY (embedded CPython 3.15)", [str(bins["startup_py"])], warmups=3, runs=15),
            bench_series("CPython 3.15", [str(PYTHON_EXE), str(CASES / "startup_py.py")], warmups=3, runs=15),
        ]

        compute_results = [
            bench_series("SPY (native Rust loop)", [str(bins["compute_loop"])], warmups=2, runs=7),
            bench_series("CPython 3.15", [str(PYTHON_EXE), str(CASES / "compute_loop.py")], warmups=1, runs=5),
        ]
        assert compute_results[0]["stdout"] == compute_results[1]["stdout"], (
            f"Mismatch: {compute_results[0]['stdout']} vs {compute_results[1]['stdout']}"
        )

        bridge_results = [
            bench_series("SPY (LazyPyModule per-call)", [str(bins["python_calls"]), "per-call"], warmups=1, runs=5),
            bench_series("SPY (SpyRuntime::with_gil batched)", [str(bins["python_calls"]), "batched"], warmups=1, runs=5),
            bench_series("CPython 3.15 (local sqrt)", [str(PYTHON_EXE), str(CASES / "python_calls.py")], warmups=1, runs=5),
        ]
        assert (
            bridge_results[0]["stdout"] == bridge_results[1]["stdout"] == bridge_results[2]["stdout"]
        ), f"Mismatch: {bridge_results}"

        mixed_results = [
            bench_series("SPY (Rust compute + Python json)", [str(bins["mixed_workload"])], warmups=2, runs=7),
            bench_series("CPython 3.15", [str(PYTHON_EXE), str(CASES / "mixed_workload.py")], warmups=1, runs=5),
        ]
        assert mixed_results[0]["stdout"] == mixed_results[1]["stdout"], (
            f"Mismatch: {mixed_results[0]['stdout']} vs {mixed_results[1]['stdout']}"
        )

        # Clean temp cargo targets inside tmp_path handled by TemporaryDirectory
        shutil.rmtree(cold_target, ignore_errors=True)
        shutil.rmtree(warm_target, ignore_errors=True)

        # Environment metadata
        rustc_ver = subprocess.check_output(["rustc", "--version"], text=True).strip()
        py_ver = subprocess.check_output([str(PYTHON_EXE), "--version"], text=True).strip()
        cpu_model = "unknown"
        try:
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
        except OSError:
            pass

        report = {
            "environment": {
                "os": platform.platform(),
                "arch": platform.machine(),
                "cpu_model": cpu_model,
                "cpu_cores": os.cpu_count(),
                "rustc": rustc_ver,
                "python": f"{py_ver} ({PYTHON_EXE})",
            },
            "build_metrics": build_metrics,
            "startup": startup_results,
            "compute_primes_150k": compute_results,
            "python_calls_100k_sqrt": bridge_results,
            "mixed_collatz_200k_json": mixed_results,
        }

        out_json = Path(__file__).resolve().parent / "results.json"
        out_json.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
