.PHONY: rust_agiserver

rust_agiserver:
	RUST_AGISERVER_BUILD_DATE="$$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
	RUST_AGISERVER_GIT_HASH="$$(git rev-parse --short HEAD 2>/dev/null || echo unknown)" \
	CARGO_TARGET_DIR=./target cargo zigbuild --release --example rust_agiserver --target aarch64-unknown-linux-gnu.2.17
