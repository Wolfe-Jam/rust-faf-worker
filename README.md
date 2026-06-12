# rust-faf-worker

**Rust → WASM FAF MCP tool executor on Cloudflare Workers** — the edge-native backend behind `mcpaas.live/rust/mcp/v1`.

The mcpaas.live RC edge owns the MCP protocol (advertises `2026-07-28`); this Worker runs the FAF tools — `faf_validate`, `faf_score`, `faf_read` — in **Rust compiled to WASM, at every Cloudflare edge** (~1ms startup, no region hop). Tool logic via [`faf-rust-sdk`](https://crates.io/crates/faf-rust-sdk).

## Develop

Requires the rustup toolchain with the `wasm32-unknown-unknown` target.

```bash
cargo test                 # unit tests (native)
worker-build --release     # build to WASM
wrangler deploy            # deploy to Cloudflare
```

> macOS note: if Homebrew's `rust` shadows rustup on `PATH` (no wasm32 std), prefix builds with
> `PATH="$HOME/.rustup/toolchains/stable-x86_64-apple-darwin/bin:$PATH"`.

## Architecture

Stateless JSON-RPC — `tools/list` + `tools/call` → the FS-free FAF read tools. The edge forwards
data methods here; this Worker executes them in WASM. Write tools that need a filesystem stay in the
native `rust-faf-mcp` stdio binary.

If `rust-faf-worker` has been useful, consider starring the repo — it helps others find it.

## License

MIT
