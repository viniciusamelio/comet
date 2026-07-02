# Vendored Rocket (patched for Workers)

This directory is a vendored copy of `core/lib`, `core/http`, and
`core/codegen` from [rwf2/Rocket](https://github.com/rwf2/Rocket), pinned at
commit `3a54d079aef060a8f732bd04ea54b0581a604087`, with two patches (see the
repo root's `patches/`) applied on top, in order:

1. `rocket-worker-feature.patch` — splits Rocket into transport-independent
   and server-enabled surfaces so it compiles for `wasm32-unknown-unknown`.
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
  the full Rocket workspace root, which isn't vendored). No other changes
  were made beyond what the two patches above apply.
- `core/lib/fuzz` was dropped (unrelated to building the library).

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
