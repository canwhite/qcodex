.DEFAULT_GOAL := install

BUILD_SCRIPT := ./build-fast.sh
SOURCE_BINARY := code-rs/bin/code
RUST_WORKSPACE := code-rs
LINT_PACKAGE ?= code-tui
ASAN_TOOLCHAIN ?= nightly
ASAN_TARGET ?= aarch64-apple-darwin
MIRI_TOOLCHAIN ?= nightly
DEFAULT_INSTALL_DIR := $(shell \
	for candidate in "$(HOME)/.local/bin" "$(HOME)/bin"; do \
		if printf '%s\n' "$$PATH" | tr ':' '\n' | grep -Fqx "$$candidate" && [ -d "$$candidate" ] && [ -O "$$candidate" ] && [ -w "$$candidate" ]; then printf '%s' "$$candidate"; exit 0; fi; \
	done; \
	old_ifs=$$IFS; IFS=:; set -- $$PATH; IFS=$$old_ifs; for candidate; do \
		if [ -n "$$candidate" ] && [ -d "$$candidate" ] && [ -O "$$candidate" ] && [ -w "$$candidate" ]; then printf '%s' "$$candidate"; exit 0; fi; \
	done)
INSTALL_DIR ?= $(DEFAULT_INSTALL_DIR)
COMMAND := $(INSTALL_DIR)/qcodex
OWNER_MARKER := $(INSTALL_DIR)/.qcodex-owner
OWNER_ID := qcodex-repository=$(CURDIR)

.PHONY: build lint lint-deps lint-clippy lint-audit lint-memory lint-asan lint-miri lint-all install uninstall clean

build:
	$(BUILD_SCRIPT)
	@test -x $(SOURCE_BINARY)

# Run routine static and supply-chain checks. Use lint-memory for the slower
# runtime checks that can expose memory-safety and undefined-behaviour issues.
lint: lint-deps lint-clippy lint-audit

lint-deps:
	@set -eu; \
	if ! command -v cargo-audit >/dev/null 2>&1; then \
		echo "[qcodex-lint] Installing cargo-audit..."; \
		cargo install cargo-audit --locked; \
	fi; \
	cd $(RUST_WORKSPACE); \
	project_toolchain="$$(rustup show active-toolchain | awk '{print $$1}')"; \
	if ! rustup component list --toolchain "$$project_toolchain" | grep -q '^clippy-.*installed'; then \
		echo "[qcodex-lint] Installing Clippy for $$project_toolchain..."; \
		rustup component add clippy --toolchain "$$project_toolchain"; \
	fi; \
	if ! rustup run $(MIRI_TOOLCHAIN) rustc --version >/dev/null 2>&1; then \
		echo "[qcodex-lint] Installing $(MIRI_TOOLCHAIN)..."; \
		rustup toolchain install $(MIRI_TOOLCHAIN) --profile minimal; \
	fi; \
	if ! rustup component list --toolchain $(MIRI_TOOLCHAIN) | grep -q '^rust-src.*installed'; then \
		echo "[qcodex-lint] Installing rust-src for $(MIRI_TOOLCHAIN)..."; \
		rustup component add rust-src --toolchain $(MIRI_TOOLCHAIN); \
	fi; \
	if ! rustup component list --toolchain $(MIRI_TOOLCHAIN) | grep -q '^miri-.*installed'; then \
		echo "[qcodex-lint] Installing Miri for $(MIRI_TOOLCHAIN)..."; \
		rustup component add miri --toolchain $(MIRI_TOOLCHAIN); \
	fi

lint-clippy: lint-deps
	cd $(RUST_WORKSPACE) && cargo clippy --workspace --all-targets --all-features -- -D warnings

lint-audit: lint-deps
	cargo audit --file $(RUST_WORKSPACE)/Cargo.lock

lint-memory: lint-deps lint-asan lint-miri

lint-asan: lint-deps
	cd $(RUST_WORKSPACE) && RUSTUP_TOOLCHAIN=$(ASAN_TOOLCHAIN) RUSTFLAGS="-Zsanitizer=address" ASAN_OPTIONS=detect_leaks=1 cargo test -p $(LINT_PACKAGE) -Zbuild-std --target $(ASAN_TARGET)

lint-miri: lint-deps
	cd $(RUST_WORKSPACE) && cargo +$(MIRI_TOOLCHAIN) miri test -p $(LINT_PACKAGE)

lint-all: lint lint-memory

install: build
	@set -eu; \
	if [ -n "$${SUDO_USER:-}" ]; then \
		echo "[qcodex-install] Refusing to install with sudo; use a user-owned INSTALL_DIR." >&2; \
		exit 1; \
	fi; \
	if [ -z "$(INSTALL_DIR)" ]; then \
		echo "[qcodex-install] No writable user-owned directory was found on PATH." >&2; \
		echo "[qcodex-install] Choose one explicitly: make INSTALL_DIR=/path/on/PATH" >&2; \
		exit 1; \
	fi; \
	case ":$$PATH:" in \
		*":$(INSTALL_DIR):"*) ;; \
		*) \
			echo "[qcodex-install] $(INSTALL_DIR) is not on PATH." >&2; \
			echo "[qcodex-install] Choose a writable PATH directory: make INSTALL_DIR=/path/on/PATH" >&2; \
			exit 1; \
		;; \
	esac; \
	mkdir -p "$(INSTALL_DIR)"; \
	if [ ! -d "$(INSTALL_DIR)" ] || [ ! -O "$(INSTALL_DIR)" ] || [ ! -w "$(INSTALL_DIR)" ]; then \
		echo "[qcodex-install] $(INSTALL_DIR) must be a writable directory owned by the current user." >&2; \
		exit 1; \
	fi; \
	if [ -e "$(COMMAND)" ] || [ -L "$(COMMAND)" ]; then \
		if [ ! -f "$(OWNER_MARKER)" ] || ! grep -Fqx "$(OWNER_ID)" "$(OWNER_MARKER)"; then \
			echo "[qcodex-install] Refusing to replace unowned $(COMMAND)." >&2; \
			echo "[qcodex-install] Remove it yourself or rerun with QCODEX_FORCE=1 after verifying its owner." >&2; \
			if [ "$${QCODEX_FORCE:-0}" != "1" ]; then exit 1; fi; \
		fi; \
	fi; \
	resolved="$$(command -v qcodex || true)"; \
	if [ -n "$$resolved" ] && [ "$$resolved" != "$(COMMAND)" ]; then \
		echo "[qcodex-install] qcodex already resolves to $$resolved; refusing to shadow it." >&2; \
		exit 1; \
	fi; \
	tmp_command="$(COMMAND).tmp.$$$$"; \
	tmp_marker="$(OWNER_MARKER).tmp.$$$$"; \
	trap 'rm -f "$$tmp_command" "$$tmp_marker"' EXIT HUP INT TERM; \
	cp "$(SOURCE_BINARY)" "$$tmp_command"; \
	chmod 755 "$$tmp_command"; \
	test -x "$$tmp_command"; \
	printf '%s\n' "$(OWNER_ID)" > "$$tmp_marker"; \
	mv -f "$$tmp_command" "$(COMMAND)"; \
	mv -f "$$tmp_marker" "$(OWNER_MARKER)"; \
	resolved="$$(command -v qcodex)"; \
	if [ "$$resolved" != "$(COMMAND)" ]; then \
		echo "[qcodex-install] qcodex resolved to $$resolved, not $(COMMAND)." >&2; \
		exit 1; \
	fi; \
	echo "[qcodex-install] Ready: $$resolved"; \
	echo "[qcodex-install] Configure a portable provider with: qcodex config-start"

uninstall:
	@set -eu; \
	if [ -e "$(COMMAND)" ] || [ -L "$(COMMAND)" ]; then \
		if [ ! -f "$(OWNER_MARKER)" ] || ! grep -Fqx "$(OWNER_ID)" "$(OWNER_MARKER)"; then \
			echo "[qcodex-install] Refusing to remove unowned $(COMMAND)." >&2; \
			exit 1; \
		fi; \
		rm -f "$(COMMAND)" "$(OWNER_MARKER)"; \
		echo "[qcodex-install] Removed $(COMMAND)"; \
	fi

clean:
	rm -f code-rs/bin/qcodex
