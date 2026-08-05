# opencode-rs benchmarks

Compare `opencode-rs` (release build) against the stock `opencode` v1.18.13 binary
(`/root/.opencode/bin/opencode`) for:

- Peak RSS memory on an identical session workload (hyperfine `--warmup` + `/usr/bin/time -v`).
- Wall-clock time for `--help`, config parse, and a cached single-turn chat.
- Cold-start latency.

TODO(integration): implement benchmark script (hyperfine) once `oc-cli` runs.
