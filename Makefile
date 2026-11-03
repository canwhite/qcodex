.DEFAULT_GOAL := install

BUILD_SCRIPT := ./build-fast.sh
SOURCE_BINARY := code-rs/bin/code
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

.PHONY: build install uninstall clean

build:
	$(BUILD_SCRIPT)
	@test -x $(SOURCE_BINARY)

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
	echo "[qcodex-install] Ready: $$resolved"

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
