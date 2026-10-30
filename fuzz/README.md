# Native parser fuzzing

On Linux with Rust nightly, `cargo install cargo-fuzz --locked`, then run:

```shell
cargo +nightly fuzz run decompress_component -- -max_total_time=60 -rss_limit_mb=2048
cargo +nightly fuzz run metadata_directory -- -max_total_time=60 -rss_limit_mb=2048
cargo +nightly fuzz run decode_page -- -max_total_time=60 -rss_limit_mb=2048
cargo +nightly fuzz run read_members -- -max_total_time=60 -rss_limit_mb=2048
```

`python -m scripts.seed_fuzz` writes deterministic synthetic valid seeds for
each target. No held-out input belongs in these seeds. Directory and member
targets mutate complete IVTs; component and page targets mutate native records.
Keep crash artifacts for minimized regression fixtures. The `fuzzing` feature
only exposes entry points for these targets, without changing Python APIs.
