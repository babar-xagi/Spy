# SPY — Super Python

**One `.rs` file. Two ecosystems. One application.**

SPY is a Rust-based toolchain and compiler frontend designed to let developers write native Rust alongside Python library usage inside `.rs` files.

## Phase 0 Status

Implemented in Phase 0 (Project Foundation):

- Cargo workspace initialized with `spy-cli`, `spy-compiler`, `spy-parser`, `spy-runtime`, and `spy-codegen`.
- Compiler architecture defined (see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [roadmap.md](roadmap.md)).
- Host detection for Rust (`rustc` / `cargo`), C/C++ compilers, Zig, Python installations (system, virtualenvs, `uv`), and Linux/WSL environments.
- PyO3 compatibility verification (headers, `libpython`, shared linkage, version compatibility, optional CPython C-API compile/link probe).
- `spy` CLI capable of running, building, checking, and inspecting `.rs` files.

## Workspace Layout

```text
Spy/
├── Cargo.toml
├── crates/
│   ├── spy-cli/
│   ├── spy-compiler/
│   ├── spy-parser/
│   ├── spy-runtime/
│   └── spy-codegen/
├── docs/
│   └── ARCHITECTURE.md
├── examples/
│   └── hello.rs
└── roadmap.md
```

## Quick Start

Inspect detected toolchains and PyO3 compatibility:

```bash
cargo run -p spy-cli -- doctor
```

Run the Phase 0 example:

```bash
cargo run -p spy-cli -- run examples/hello.rs
```

Build or check a `.rs` file:

```bash
cargo run -p spy-cli -- check examples/hello.rs
cargo run -p spy-cli -- build examples/hello.rs --release
```
