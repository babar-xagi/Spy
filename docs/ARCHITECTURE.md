# 🏛️ SPY Architecture (Phases 0–2)

SPY (**Super Python**) compiles `.rs` source files that mix native Rust with Python-style imports and expressions into native Rust/PyO3 executables.

---

## 🧩 Crate Responsibilities

| Crate | Component | Responsibility |
| --- | --- | --- |
| 💻 `spy-cli` | CLI Frontend | Provides `spy run`, `spy build`, `spy check`, `spy doctor`, `spy tokens`, and `spy emit`. |
| ⚙️ `spy-compiler` | Compiler Orchestrator | Coordinates parsing, code generation, host/toolchain detection (`rustc`, `cargo`, C/C++, Zig, Linux/WSL2), and routes builds to either direct `rustc` or a cached Cargo runner configured with `PYO3_PYTHON` and Unix `-Wl,-rpath`. |
| 🔍 `spy-parser` | Lexer & Parser | Tokenizes `.rs` input with byte/line/column spans (`logos`) and parses Rust items alongside `use python::...;` syntax (`as` aliases, nested module paths, `{...}` groups). |
| 🧬 `spy-codegen` | Translator / Emitter | Translates `use python::...;` statements into cached `spy_runtime::LazyPyModule` constants with line-aligned output and preloads modules at `fn main` entry. |
| 🐍 `spy-runtime` | Python & PyO3 Runtime | Discovers Python toolchains, verifies PyO3 compatibility (`Python.h`, `libpython`, shared linkage), embeds CPython via `pyo3 0.30.1` (`SpyRuntime`, `SpyModule`, `SpyObject`, `LazyPyModule`), caches imported modules, and maps Python exceptions/import errors into structured Rust diagnostics. |

---

## 🔄 Compilation Pipeline

```text
📄 source (.rs)
      │
      ▼
🔍 spy-parser      (lexing + AST / `use python::...` detection with source spans)
      │
      ▼
🧬 spy-codegen     (pure Rust passthrough OR `LazyPyModule` lowering + preload)
      │
      ├──► 🦀 [pure Rust]      ──► direct `rustc` compilation
      └──► 🐍 [Python imports] ──► shared-cache Cargo runner + `spy-runtime` + `pyo3`
```

---

## 📦 Dependency Decoupling

- `spy-runtime` exposes an `embedded` Cargo feature (enabled by default for compiled SPY programs and runtime tests).
- `spy-compiler` and `spy-cli` depend on `spy-runtime` with `default-features = false` so the `spy` CLI binary stays lightweight and builds independently of `libpython`, while user programs referencing `use python::...;` automatically pull in `embedded` (`pyo3`) with the best host Python detected by `spy-runtime`.
