# mesh-sim

A browser playground for the [Offline Protocol](https://github.com/Offline-Protocol/offline-protocol-sdk)
mesh. Every device on the map runs the real engine (`offline-protocol` 0.31.0
from crates.io), compiled to WebAssembly. Frames move between devices the way
the SDK's own forwarding tests move them (`tests/mesh_forwarding.rs`):
whatever a device hands to a neighbor reaches that neighbor and nobody else,
unless the air loses or delays it.

- **Double-click** to add a device, **drag** to move it, **right-click** to remove it
- **Click** a device, then another, to send a message, and watch it flood out and the receipt come back
- Radio range, links per device (4 is what a BLE phone holds), frame loss, delay and round length are live

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.105 --locked   # must match the crate pin
./build.sh
python3 -m http.server -d web 8080   # open http://localhost:8080
```

`web/` is the whole site once built: static files, deployable anywhere.

## Deploy

Hosted on Cloudflare Workers as static assets (`wrangler.jsonc`):

```bash
./build.sh
npx wrangler deploy   # https://mesh-sim.mizanalip786.workers.dev
```

`cargo test` runs a three-device check on the host (message carried two hops,
receipt carried back, sender told it was delivered).

To watch an unreleased engine change, point the three `offline-protocol*`
dependencies at an SDK checkout with a `[patch.crates-io]` section.
