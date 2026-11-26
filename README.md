# 🐍🦀 SPY — Super Python

**One `.rs` file. Two ecosystems. One native application.**

SPY (**Super Python**) is a Rust-based compiler frontend and runtime toolchain that brings Python libraries directly inside `.rs` source files alongside native Rust performance.

---

## ✅ Implementation Status

| Phase | Milestone | Status | Highlights |
| --- | --- | --- | --- |
| **Phase 0** | 🛠️ Project Foundation | ✅ Done | Workspace (`spy-cli`, `spy-compiler`, `spy-parser`, `spy-runtime`, `spy-codegen`), Linux/WSL2 & toolchain detection, PyO3 compatibility checks, pure Rust `.rs` execution |
| **Phase 1** | 🐍 Python Runtime Integration | ✅ Done | Embedded CPython via `pyo3` (`0.30.1`), `SpyRuntime` lifecycle & GIL attach/detach, module imports, function/method calls, class instantiation, `SpyObject`, rich Python tracebacks |
| **Phase 2** | 📦 Python Import Syntax | ✅ Done | `use python::...;` syntax (`as` aliases, nested paths, `{...}` groups), global module caching (`LazyPyModule`), actionable `ImportError` hints, auto `PYO3_PYTHON` & `rpath` |

See [roadmap.md](roadmap.md) for the complete 13-phase plan and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for compiler internals.

---

## 📊 Benchmark Highlights (SPY vs CPython 3.15)

Measured on Linux WSL2 (`rustc 1.100.0`, `Python 3.15.2`, `pyo3 0.30.1`) — full report & charts in [benchmark/benchmark.md](benchmark/benchmark.md):

| Workload | SPY (Median) | CPython 3.15 (Median) | Result |
| --- | ---: | ---: | --- |
| ⚡ Startup (pure Rust) | `2.19 ms` | `20.32 ms` | **9.27x faster** |
| 🧮 Prime Count (`< 150,000`) | `25.47 ms` | `150.59 ms` | **5.91x faster** |
| 🚀 Mixed (`200k` Collatz + `json.dumps`) | `55.52 ms` | `1,596.10 ms` | **28.75x faster** |
| 🔗 `100k` `math.sqrt` (`with_gil` batched) | `28.56 ms` | `28.05 ms` | **On par (`0.98x`)** |
| 🏗️ Warm `spy build --release` (Python) | `394.6 ms` | — | **`599.3 KiB` binary** |

---

## 🗂️ Workspace Layout

```text
Spy/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── spy-cli/        # 💻 `spy` CLI binary (run, build, check, doctor, emit, tokens)
│   ├── spy-compiler/   # ⚙️ Toolchain detection & rustc/Cargo orchestration
│   ├── spy-parser/     # 🔍 `logos` lexer & `use python::...` parser
│   ├── spy-codegen/    # 🧬 Rust / LazyPyModule lowering with line preservation
│   └── spy-runtime/    # 🐍 Embedded CPython + PyO3 runtime, caching, diagnostics
├── benchmark/
│   ├── benchmark.md    # 📈 Performance, memory, build & chart report
│   ├── run_benchmarks.py
│   └── cases/          # 🧪 Paired CPython & SPY benchmark sources
├── docs/
│   └── ARCHITECTURE.md # 🏛️ Pipeline & crate design
├── examples/
│   ├── hello.rs        # 👋 Phase 0 pure Rust example
│   └── python_imports.rs # 🐍 Phase 1-2 mixed Rust + Python imports example
└── roadmap.md          # 🗺️ Full development roadmap
```

---

## 🚀 Quick Start

Ensure Rust (`~/.cargo/bin`) and Python (`~/.local/bin`) are on `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
```

### 🩺 Inspect Environment & PyO3 Readiness
```bash
cargo run --offline -p spy-cli -- doctor --probe
```

### ▶️ Run Examples
```bash
# Phase 0 — pure Rust
cargo run --offline -p spy-cli -- run examples/hello.rs -- --phase 0

# Phases 1–2 — Python imports + native Rust
cargo run --offline -p spy-cli -- run examples/python_imports.rs
```

### 🛠️ Check, Emit, & Build Standalone Binaries
```bash
cargo run --offline -p spy-cli -- emit examples/python_imports.rs
cargo run --offline -p spy-cli -- check examples/python_imports.rs
cargo run --offline -p spy-cli -- build examples/python_imports.rs --release -o ./python_imports
./python_imports
```

### 🧪 Run Tests, Clippy, & Benchmarks
```bash
cargo test --offline --workspace
cargo clippy --offline --workspace --all-targets -- -D warnings
python3.15 benchmark/run_benchmarks.py
```

---

## 📄 License

MIT — see [LICENSE](LICENSE).
