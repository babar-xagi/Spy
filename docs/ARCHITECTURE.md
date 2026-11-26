# SPY Architecture (Phase 0 Foundation)

SPY ("Super Python") compiles `.rs` source files that mix native Rust with Python-style library imports and expressions into native Rust/PyO3 executables.

## Crate Responsibilities

| Crate | Component | Responsibility |
| --- | --- | --- |
| `spy-cli` | CLI Frontend | Provides `spy run`, `spy build`, `spy check`, `spy doctor`, and `spy tokens`. |
| `spy-compiler` | Compiler Orchestrator | Coordinates parsing, code generation, host/toolchain detection (`rustc`, `cargo`, C/C++, Linux/WSL), and compilation/execution. |
| `spy-parser` | Lexer & Parser | Tokenizes `.rs` input with span tracking (`logos`) and recognizes Rust items alongside `use python::...` import syntax. |
| `spy-codegen` | Translator / Emitter | Translates parsed SPY units into valid Rust source code consumable by `rustc` / Cargo. |
| `spy-runtime` | Python & PyO3 Runtime Layer | Discovers Python installations, validates CPython dev headers/shared libraries, verifies PyO3 compatibility, and will host embedded CPython lifecycle management in Phase 1. |

## Compilation Pipeline

```text
source (.rs)
   │
   ▼
spy-parser      (lexing + AST / import detection with source spans)
   │
   ▼
spy-codegen     (Rust / PyO3 bridge code emission)
   │
   ▼
spy-compiler    (rustc invocation, artifact management, runner)
   │
   ▼
spy-runtime     (host CPython discovery, PyO3 compatibility, embedded runtime)
```

## Phase 0 Scope

Phase 0 establishes the Rust workspace, CLI, compiler pipeline boundaries, host toolchain detection (Rust, Cargo, C/C++, Python, Linux/WSL), PyO3 compatibility verification, and end-to-end execution of pure Rust `.rs` files such as `examples/hello.rs`.
