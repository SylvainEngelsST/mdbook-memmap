# mdbook-memmap

Native Rust memory-map and register documentation for
[mdBook](https://rust-lang.github.io/mdBook/), using reusable YAML IP definitions
and hierarchical system maps.

**Status: checkpoint 4, native release verification ready for review.**
Active `memmap` inclusions now render directly in Rust. Books without inclusions
are preserved. No Python runtime, package installation, or backend command is
needed by end users.

Existing version-1 release locks are consumed natively. The approved version-2
mdBook provenance envelope identifies the Rust implementation and executable;
it no longer claims that a Python backend was used.

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
No Python runtime is required for native book rendering.

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
`source-roots` is required for active inclusions and must be a nonempty array of
directories relative to the book root (absolute paths also work). Both direct
sources and transitive references must stay inside these canonical roots.
Only accessed source files are loaded; source directories must not change during
a build. Chapter paths, including symlinks, are resolved to their actual source
directory before resolving the fence's `source`.

The configuration is strict: unknown keys/types fail. Standard mdBook keys
`command`, `renderers`, `before`, `after`, and `optional = false` are accepted.
`optional = true` is rejected because validation failures must abort the build.

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
| `source` | Required nonempty string. YAML path relative to the containing chapter; the native library enforces canonical paths and allowed roots. |
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
When migrating a Python-backed book, remove `backend`, `backend-args`, and
`backend-timeout-seconds`; each causes an explicit migration diagnostic. The
fences do not change. Configure `release-lock` and `provenance` together for
locked builds; neither setting is optional when the other is present.

## Native Markdown Rendering

```rust,no_run
use mdbook_memmap::{
    fences::View,
    markdown::{render, Source},
    project::Project,
};
use std::path::PathBuf;

let root = PathBuf::from("released-maps").canonicalize()?;
let mut project = Project::new(std::slice::from_ref(&root))?;
let ip = project.load(&root.join("ips/timer.yaml"))?;
let markdown = render(Source::Ip(&ip), Some("csr"), View::Registers, "")?;
let system = project.resolve(&root.join("systems/soc.yaml"), "cpu")?;
let overview = render(Source::System(&system), None, View::Summary, "memmap-1-1")?;
# Ok::<(), anyhow::Error>(())
```

Library rendering expects validated definitions from the loader or resolver;
do not mutate inputs to violate their invariants. IP rendering can select one
map or all maps. System rendering uses a resolved address space; a local map
selector is invalid there. Detailed register views reject memory/reserved maps.

The renderer preserves authored Markdown descriptions, escapes table cells,
distinguishes implemented and allocated extents, fills reserved field gaps,
and retains partial/missing reset semantics. Table fields are sorted by bit
position while their descriptions stay in authored order. Arbitrary-precision
addresses and reset values are never truncated. Each mdBook inclusion receives
a unique `memmap-CHAPTER-FENCE` namespace, including nested chapters and repeated
views; library callers can supply their own lowercase ASCII namespace.

Fourteen checked-in outputs from the independent Python reference exporter are
compared byte-for-byte in Rust tests, including rich descriptions, memory and
reserved maps, the last 32-bit address, and addresses/resets wider than 64 bits.
The legacy `Generated by rds-memmap` comment is intentionally preserved for
compatibility; it does **not** indicate a Python/backend invocation.

## Native Model and Hierarchy API

```rust,no_run
use mdbook_memmap::project::Project;
use std::path::PathBuf;

let mut project = Project::new(&[PathBuf::from("released-maps")])?;
let view = project.resolve(
    &PathBuf::from("released-maps/systems/soc.yaml"), "cpu"
)?;
for block in view.blocks {
    for register in &block.memory_map.registers {
        println!("{}: {}", register.name, &block.base + &register.offset);
    }
}
# Ok::<(), anyhow::Error>(())
```

`loader::load_definition` parses and validates one definition without traversing
references. Use `Project::load` for the complete hierarchy or `Project::resolve`
for a selected flattened system view. Sources resolve relative to the YAML file
containing the reference. Every declared space is traversed, not only the one
selected for display. The typed model retains source file/line/column/object-path
locations, descriptions, reset values/masks, and instance roles.

The strict scalar subset matches the Python authoring contract: decimal/hex
nonnegative integers, true/false booleans, and string prose. Quoted numeric text,
floating-point syntax, nulls, and booleans are not accepted as integers.
`on`/`off` and date-like strings remain text. Decimal leading zeros do not imply
octal. Binary/octal/separator integer syntax is not recognized as an integer.
Reject duplicate keys, merge keys, aliases, anchors, explicit tags, multiple
documents, non-string mapping keys, and unsupported directives; `%YAML 1.2`
is allowed.

Local checks cover naming, unknown/missing properties, unique names, widths,
field/register bounds and overlaps, register alignment, software access/effects,
hardware-write volatility, and reset references/widths/duplicates. Unspecified
resets stay unspecified; zero masks do not imply retention; masked-out value bits
are not zeroed. Memory/reserved maps remain distinct from register maps.

Hierarchy checks cover file existence, canonical allowed roots, symlink escapes,
cycles, unique VLNV-to-file identity, target existence, complete allocation fit,
overlaps including allocation tails, absolute register alignment, and ambiguous
flattened instance/map/reset names. A tail allocation does not enlarge implemented
storage; aliases remain unsupported.

A `Project` is a per-operation snapshot. Reused IP definitions share `Arc`
identity; a fresh project is required to reread edits. Failed hierarchy loads
clear the source/identity caches. `snapshots()` exposes the exact parsed bytes for
provenance checks. File contents are not locked against concurrent edits:
release inputs must remain immutable during a build.

Addresses, sizes, and reset values/masks use arbitrary-precision `BigUint`;
there is no silent 64-bit truncation. Widths and bit indices use checked `u64`
arithmetic. As explicit operational limits, YAML nesting and source-reference
depth are limited to 128 levels, and width/index values above `u64::MAX` fail.
These limits fail diagnostically instead of overflowing or exhausting the stack.
Reset bit strings in detailed Markdown have a maximum field width of 65,536
bits; wider displayed resets fail with `[markdown-width]` rather than allocating
unbounded output or silently truncating. Summary rendering is unaffected.

The timer/subsystem/system [fixtures](tests/fixtures/) are copied compatibility
inputs from the accepted rds-designspec example. Additional rendering fixtures
exercise wide values, table escaping, and partial resets. The native tests assert
all four reference addresses, shared IP identity, and the same access/reset
semantics. No Python executable is needed to run the Rust test suite.

## Locked Books and Native Provenance

Create a version-1 lock with the existing standalone tool, which remains a
separate Python utility in this phase:

```sh
rds-memmap lock released-maps/systems/soc.yaml \
  --source-root released-maps --address-space cpu --release soc-v1 \
  --output inputs.lock.json
```

Consuming that lock in mdBook requires only the native binary:

```toml
[preprocessor.memmap]
command = "mdbook-memmap"
source-roots = ["../released-maps"]
release-lock = "../inputs.lock.json"
provenance = "memmap-provenance.json"
renderers = ["html"]
before = ["links"]
```

Configuration paths are relative to the book root; pin paths and the entry
source inside a lock are relative to the lock's directory. Locks are portable
when moved together with their source trees. The configured canonical roots
still apply to every pinned and transitively referenced source.

Native verification:

- Rejects malformed/unknown/duplicate JSON properties, invalid version/release,
  absolute pin paths, duplicate canonical sources, and invalid digests/VLNVs.
- Verifies exact SHA-256 bytes **before parsing**, then checks the native VLNV.
- Validates the complete pinned inventory and its references, including inputs
  not selected for display, and validates the lock's entry selector.
- Rejects unpinned referenced/included sources. A fence can select any pinned
  IP/system with a valid selector; it is not restricted to the entry alone.
- Binds hashes to the bytes actually parsed, then rechecks lock/source bytes
  and canonical pin paths immediately before publishing provenance.
- Requires at least one active inclusion in a locked book.

Use `Project::with_release_lock(roots, Arc<ReleaseLock>)` for library loading
with pre-parse pin checks, and call `ReleaseLock::verify_inventory` before use.
The mdBook integration performs the complete verification/publishing lifecycle.
Do not use `Project::new` alone as a substitute for locked loading.

### Provenance format

The mdBook report deliberately changes its outer `format_version` from **1 to
2** to distinguish native execution. The old `backend` property is removed.
The `inputs` object retains version 1, release label, exact lock digest, original
relative source records, and ordered selection records with fragment hashes.

| Property | Meaning |
|----------|---------|
| `adapter` | `mdbook-memmap` package name and version. |
| `mdbook_version` | Calling mdBook version. |
| `source_roots` | Canonical allowed roots for this invocation. |
| `inputs.release` / `inputs.release_lock_sha256` | Release label and SHA-256 of exact lock bytes. |
| `inputs.sources` | Complete pins sorted by their original relative path. |
| `inputs.generator.kind` | `native-rust`, never a fabricated Python identity. |
| `inputs.generator.name` / `version` | Native package identity. |
| `inputs.generator.implementation_sha256` | SHA-256 of compact UTF-8 JSON mapping sorted package filenames to their SHA-256 values. Includes every Rust source, the build script, Cargo manifest, and Cargo lock, embedded at compile time. |
| `inputs.generator.executable_sha256` | SHA-256 of the running executable. For a library embedding this is the host executable containing the engine. |
| `inputs.selections` | Preorder inclusion ID, lock-relative source, canonical roots, view, selector, anchor prefix, and `output_sha256` of exact generated UTF-8 Markdown. |

Reports are deterministic for the same executable, paths, lock bytes, and
selections. Relocation changes the recorded absolute roots, but not relative
pins, selection fragment hashes, or generator identity. Different compiler,
linker, build paths, or packaging normalization can change executable and/or
implementation identity; compare the actual qualified artifact, not just its
version string.

Provenance is atomically replaced only after all inclusions and release checks
succeed. Its parent must already exist. It cannot overwrite a source tree,
chapter input (including a symlink target), lock, configuration, or running
executable, and cannot replace a symbolic link. Failed builds preserve any
previous report; **always check the build exit status** rather than treating a
pre-existing report as proof of a successful rebuild.

Locks provide integrity against a trusted manifest, not authenticity or a
digital signature. Provenance attests the preprocessor's generated fragments,
not the final HTML or subsequent mdBook preprocessors/renderers. Keep release
inputs immutable; final byte rechecks are not filesystem locks.

## Development and Packaging

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
cargo doc --locked --no-deps
cargo package --locked
```

Keep `CARGO_HOME` and `CARGO_TARGET_DIR` on project storage in quota-limited
environments. After dependencies are cached, use `--offline`. The small
[example book](examples/book/) contains literal fences and active IP/system
summary/detail inclusions. Build it after putting the compiled binary on `PATH`:

```sh
cargo build --locked
PATH="$PWD/target/debug:$PATH" mdbook build examples/book
MDBOOK="$(command -v mdbook)" cargo test --locked --test book -- --ignored
```

The explicitly enabled book test requires mdBook 0.5.4. It builds twice with
only the native binary directory on the child process's `PATH`, checks identical
HTML bytes, all four system register addresses, 18 tables, 23 unique memory-map
anchors, and their links. CI runs this test as well as the Python-free Rust suite.
If using `CARGO_TARGET_DIR`, adjust the binary path in the build command.

The [locked example](examples/locked-book/) reuses the same Markdown with a
combined reference inventory. Build it with `mdbook build examples/locked-book`;
its report is written beside its configuration and ignored by Git. The second
explicit book test copies the inputs into a clean temporary workspace, compares
HTML and provenance from two independent output builds, checks four fragment
hashes against Python goldens, and proves that changing an unused pin fails
without publishing new HTML or replacing the previous report.

For offline end users, distribute the compiled native executable built with
`cargo build --release --locked`. They need neither Python nor Cargo nor
internet access to run it. Building from source offline additionally requires a
prepopulated Cargo cache or a separately prepared Cargo vendor tree; a source
checkout or `.crate` archive alone does not bundle third-party dependencies.
Crate contents include example sources/configuration, not generated HTML or
per-build provenance reports, even after building the examples locally.

Only maintainers regenerating the parity goldens need the Python reference:

```sh
python3 tests/reference/generate.py /path/to/qualified/bin/rds-memmap
```

Goldens were generated from the accepted rds-designspec implementation
(`897fb47`). Do not regenerate them using the renderer under test.

## License

[Mozilla Public License 2.0](LICENSE).
