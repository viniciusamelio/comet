# Vendored Rocket (patched for Workers)

This directory is a vendored copy of `core/lib`, `core/http`, and
`core/codegen` from [rwf2/Rocket](https://github.com/rwf2/Rocket), pinned at
commit `3a54d079aef060a8f732bd04ea54b0581a604087`, with two patches (see the
repo root's `patches/`) applied on top, in order:

1. `rocket-worker-feature.patch` — splits Rocket into transport-independent
   and server-enabled surfaces so it compiles for `wasm32-unknown-unknown`.
   It also makes route/catcher handler futures local-boxed under the `worker`
   feature, which lets Worker routes await `!Send` JavaScript futures directly
   on a single-threaded isolate.
2. `rocket-worker-streaming-request.patch` — adds a `RawStream::Worker`
   variant and `Data::from_stream()` constructor so a Cloudflare Worker
   request body can be streamed into Rocket's `Data` instead of buffered
   first. See "Streaming adapter" in `docs/rocket-worker-roadmap.md` for why.

It is vendored (checked into `comet`'s own repository) rather than referenced
via a `path`/`git` dependency pointing outside this repo, so that cloning
`comet` is enough to build it — no manual clone-and-patch step, no dependency
on a machine-local checkout that can disappear (this is exactly what broke
the build before vendoring: the checkout lived in `/tmp` and didn't survive a
reboot).

Differences from a plain checkout of `core/{lib,http,codegen}`:

- `[lints] workspace = true` was removed from each `Cargo.toml` (it requires
  the full Rocket workspace root, which isn't vendored).
- `core/lib/fuzz` was dropped (unrelated to building the library).
- 2026-07-06: a set of lint fixes was applied directly to `core/lib` (not
  captured as a `patches/*.patch` file — see below for why) to clear the
  warning baseline described in "Current validation". No behavior changed
  under any feature combination; every fix either declares a cfg the crate
  already emits/consumes (`nightly`, `broken_fmt`, `rust_analyzer`), removes
  a genuinely unused import, or narrows an item's `#[cfg(feature = "...")]`
  gate to match the feature its *only* caller already requires (e.g.
  `Endpoint::fetch` is only called from `listener::{unix,tcp}`, both
  `#[cfg(feature = "server")]` — gating the definition the same way doesn't
  change what's compiled under `server`, only under configurations that
  never called it in the first place). One real upstream bug was fixed
  along the way: `ConnectionMeta::server_name`'s
  `#[cfg_attr(feature = "tls", allow(dead_code))]` had the condition
  backwards relative to its sibling field `peer_certs`'s
  `#[cfg_attr(not(feature = "mtls"), allow(dead_code))]` — the field is only
  *read* by `Request::sni()`, which is itself `#[cfg(feature = "tls")]`, so
  the allow needed to fire in the *absence* of `tls`, not its presence.
  This wasn't packaged as a numbered patch file because the two existing
  patches are large, structural deltas meant to be re-applied verbatim
  against a newer upstream commit (see "Updating this vendor drop"); this
  change is a small, scattered set of `#[cfg(...)]` additions across ~10
  files that doesn't survive a `git apply` re-vendor step cleanly anyway
  (upstream will have moved the surrounding lines). Re-derive it by fixing
  whatever the same validation commands report against the new commit
  instead of trying to reapply this as a patch.

## Updating this vendor drop

```sh
git clone https://github.com/rwf2/Rocket.git /tmp/rocket-src
cd /tmp/rocket-src && git checkout <new-commit>
git apply /path/to/comet/patches/rocket-worker-feature.patch
git apply /path/to/comet/patches/rocket-worker-streaming-request.patch
cp -R core /path/to/comet/vendor/rocket/core
# re-remove the `[lints] workspace = true` blocks and `core/lib/fuzz`, see above
```

If the patch no longer applies cleanly against a newer Rocket commit, that's
the signal to revisit upstreaming it instead of re-vendoring — see
`docs/rocket-worker-roadmap.md`.

## Current validation

As of 2026-07-02, the Worker-facing delta was validated with:

```sh
cargo fmt --check
cargo test --features cloudflare,cloudflare-d1,cloudflare-queue,cloudflare-kv,cloudflare-r2,cloudflare-service,cloudflare-hyperdrive
cargo check --manifest-path examples/cloudflare-worker/Cargo.toml
cd examples/cloudflare-worker && npm run test:integration
cd examples/cloudflare-worker && npm run test:perf
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" check --manifest-path vendor/rocket/core/lib/Cargo.toml
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" check --manifest-path vendor/rocket/core/lib/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features worker
```

Both Rocket checks pass. The example integration test covers D1, Queues, R2
object round-tripping, streaming responses, and a Worker WebSocket echo path.
The performance test completed without request errors under local `wrangler
dev`.

As of 2026-07-06, the previously-documented warning baseline was fixed (see
the lint-fixes entry above) and re-validated:

```sh
RUSTC="$(rustup which rustc)" cargo check --manifest-path vendor/rocket/core/lib/Cargo.toml                      # server feature (Rocket's default): clean except one pre-existing `deprecated` warning (see below)
cargo build --features cloudflare,cloudflare-d1,cloudflare-queue,cloudflare-r2,cloudflare-kv,cloudflare-service,cloudflare-hyperdrive,cloudflare-websocket   # comet's worker feature: 0 warnings
RUSTC="$(rustup which rustc)" cargo check --manifest-path examples/cloudflare-worker/Cargo.toml --target wasm32-unknown-unknown                            # 0 warnings
```

One warning was deliberately left alone: `tcp.rs`'s `TcpStream::set_linger`
call is deprecated by tokio (`SO_LINGER` can block the thread on drop), but
fixing it means changing real TCP shutdown behavior on the `server` feature
— something `comet` never builds with `server` enabled, so it's not
something this vendoring effort can responsibly validate. Left for whoever
next touches `listener/tcp.rs` under `server`.
