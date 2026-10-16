# Agent instructions

Read [`llms.txt`](llms.txt) for Topcoat's APIs and application conventions. Consult the relevant linked guide before changing a feature, and update `llms.txt` when its guidance is affected.

Read [`REBASE.md`](REBASE.md) before changing the iroh integration or rebasing this fork onto upstream Topcoat. Keep its patch map and upstream base current. Prefix fork commits with `iroh! <type>:`, such as `iroh! feat:` or `iroh! fix:`.

- Unsafe code is not allowed.
- Macro-body formatters live behind each grammar crate's `pretty` feature and are used by `topcoat fmt`.
- Runtime expressions must behave consistently in Rust and JavaScript. Use `crates/topcoat-runtime/coherence` to check parity; see [`docs/runtime/coherence.md`](docs/runtime/coherence.md).
