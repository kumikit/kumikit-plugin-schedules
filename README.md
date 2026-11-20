# kumikit-plugin-schedules

First-party scheduled prompts plugin (`kumikit.schedules`) for
[Kumikit](https://github.com/kumikit/kumikit).

## Build and test

```sh
cargo test -p kumikit-schedules
cargo build --target wasm32-wasip2 --release -p kumikit-schedules
KUMIKIT_DIR=../kumikit cargo test -p kumikit-schedules-e2e
```
