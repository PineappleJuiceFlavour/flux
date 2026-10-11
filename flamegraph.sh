#!/bin/sh
RUSTFLAGS="-C force-frame-pointers=yes" PATH=~/.cargo/bin/:$PATH cargo flamegraph
