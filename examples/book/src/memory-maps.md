# Native Memory Maps

All tables below are rendered in-process by Rust. The YAML IP definition is
reused by the hierarchical system and by the standalone register view.

## Hierarchical overview

```memmap
source: ../../../tests/fixtures/systems/mosaic_example.yaml
address_space: cpu
view: summary
```

## Reusable IP registers

```memmap
source: ../../../tests/fixtures/ips/timer.yaml
map: csr
view: registers
```

## System registers

This repeats the same IP twice with absolute addresses and distinct anchors.

```memmap
source: ../../../tests/fixtures/systems/mosaic_example.yaml
address_space: cpu
view: registers
```

## Partial resets and wide fields

Reserved gaps, authored field order, partial/missing resets, and values wider
than 64 bits are preserved. Question marks mean unspecified, not retained.

```memmap
source: ../../../tests/fixtures/ips/rendering.yaml
map: csr
view: registers
```
