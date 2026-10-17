# mdbook-memmap

Native Rust memory-map and register documentation for
[mdBook](https://rust-lang.github.io/mdBook/), using reusable YAML IP definitions
and hierarchical system maps.

**Status: checkpoint 1, package and fence contract ready for review.**
This is not yet a functional replacement for the Python-backed preprocessor.
Active `memmap` inclusions fail explicitly until native rendering is implemented.
Books without active inclusions are preserved. No Python process or backend
command is invoked.

## Scope and Review Gates

1. Standard Rust library/binary package, mdBook protocol, configuration installer,
   unchanged fence contract, tests, and packaging.
2. Native typed YAML model, strict validation, diagnostics, and hierarchy resolver.
3. Native Markdown renderer, reference parity, and direct mdBook integration.
4. Release locks/provenance, real-book acceptance, and final packaging checks.

Each checkpoint is validated and reviewed before a Conventional Commit is made
in this repository. Standalone validation/export/IP-XACT utilities remain Python
for this phase; they are not silently replaced or removed from rds-designspec.

## Installation

The qualified build toolchain is Rust 1.99.0. From a checkout:

```sh
cargo install --path . --locked
mdbook-memmap install path/to/book
```

Registry publication is not yet performed; `cargo install mdbook-memmap` will be
documented when a release is published. Maintainers can distribute the compiled
binary directly to users who do not have Cargo or network access.
No Python runtime will be required for native book rendering.

`install` edits an existing `book.toml`, preserves comments and existing
configuration, and is idempotent. Review its default root (`"."`, the book
directory) and narrow it to the released map directories needed by the book.
No JavaScript assets are required.

```toml
[preprocessor.memmap]
command = "mdbook-memmap"
source-roots = ["../released-maps"]
renderers = ["html"]
before = ["links"]
```

The native preprocessor uses the standard `mdbook-preprocessor` library like
mdbook-wavedrom. Its library exports `Memmap` implementing `Preprocessor`; the
binary supports `--help`, `--version`, `supports html`, and `install [book-dir]`.
It supports mdBook `>=0.5.4, <0.6.0` and HTML only.
Protocol stdout contains JSON only; diagnostics use stderr.
At checkpoint 1 the configuration is scaffolded, not a claim that source-root
enforcement or rendering is implemented.

## Unchanged Fence Options

````markdown
```memmap
source: ../released-maps/systems/soc.yaml
address_space: cpu
view: summary
```

```memmap
source: ../released-maps/ips/timer.yaml
map: csr
view: registers
```
````

| Property | Usage |
|----------|-------|
| `source` | Required nonempty string. YAML path relative to the containing chapter; canonical path/root checking is implemented with the resolver in checkpoint 2. |
| `view` | Required `summary` or `registers`. |
| `map` | Exact IP-local map name; required for IP inputs and mutually exclusive with `address_space`. |
| `address_space` | Exact system address-space name; required for system inputs and mutually exclusive with `map`. |

Unknown/duplicate properties, non-string values, empty selectors, and extra fence
info tokens fail. Only top-level backtick/tilde fences with info exactly `memmap`
are active. List/blockquote inclusions fail instead of corrupting Markdown.
Longer outer Markdown fences and indented code preserve literal examples.

The existing YAML source contract remains the compatibility target: schema
version 1, `rds-v1` naming, separate IP/system definitions, software access,
`hwAccess`, volatility, reset types, and reusable hierarchy. Python remains the
reference for renderer parity, not a runtime dependency or fallback.
The old `backend`, `backend-args`, and `backend-timeout-seconds` settings are not
part of the native architecture; migration diagnostics and strict configuration
will be implemented with the integrated renderer.

## Development and Packaging

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
cargo package --locked
```

Keep `CARGO_HOME` and `CARGO_TARGET_DIR` on project storage in quota-limited
environments. After dependencies are cached, use `--offline`. The small
[example book](examples/book/) contains literal examples and can be built with
mdBook after putting the compiled binary on `PATH`.

## License

[Mozilla Public License 2.0](LICENSE).
