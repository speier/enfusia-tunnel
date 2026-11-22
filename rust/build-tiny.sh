#!/usr/bin/env sh
# Optional: smallest possible binary. Rebuilds std itself with panic=immediate-abort
# (needs nightly + rust-src:  rustup toolchain install nightly -c rust-src).
# On Linux x86_64 this takes the binary from ~652 KB to ~375 KB. Same output, same
# audio; the only difference is that a panic aborts without printing a message.
set -e
cd "$(dirname "$0")"
TARGET=$(rustc +nightly -vV | sed -n 's/^host: //p')
RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort" \
  cargo +nightly build --release --target "$TARGET" -Z build-std=std,panic_abort
ls -l "target/$TARGET/release/enfusia-tunnel"
