#!/usr/bin/env bash
# Builds blitzwolf-capsled-x86_64-linux.tar.gz with a static (musl) binary, ready
# to install on another x86_64 Linux machine without Rust: extract it and run ./install.sh
set -euo pipefail
cd "$(dirname "$0")"

rustup target add x86_64-unknown-linux-musl >/dev/null
cargo test --quiet
cargo build --release --target x86_64-unknown-linux-musl
install -Dm755 target/x86_64-unknown-linux-musl/release/blitzwolf-capsled dist/blitzwolf-capsled
OUT=blitzwolf-capsled-x86_64-linux.tar.gz

tar -czf "$OUT" --transform 's,^,blitzwolf-capsled/,' \
    install.sh uninstall.sh README.md LICENSE packaging dist
echo "Built: $(pwd)/$OUT"
