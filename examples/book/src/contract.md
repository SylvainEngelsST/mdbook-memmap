# Authoring Contract

This checkpoint demonstrates literal authoring examples only. Active inclusions
will become available after the native model and renderer are implemented.

````markdown
```memmap
source: ../maps/systems/soc.yaml
address_space: cpu
view: summary
```

```memmap
source: ../maps/ips/timer.yaml
map: csr
view: registers
```
````
