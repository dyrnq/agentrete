# Agentrete — Makefile
# Targets for development, testing, and CI.

CARGO      ?= cargo
TEST_CFG   ?= /tmp/test-m2v.toml
TEST_DB    ?= /tmp/test-db
M2V_256    ?= $(HOME)/.cache/model2vec/bge-small-256d
M2V_512    ?= $(HOME)/.cache/model2vec/bge-small-512d
PORT       ?= 9092

.PHONY: all build check lint test clean run-mcp run-scan seed \
        test-all test-unit test-integration test-reembed test-kg \
        fmt clippy doctor clean-db clean-all help

all: lint build test-unit

build:
	$(CARGO) build

release:
	$(CARGO) build --release

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

clippy:
	$(CARGO) clippy --all-targets -- -D warnings

lint: fmt-check clippy

test-unit:
	$(CARGO) test

test: test-unit

test-integration: check-deps
	@echo "=== Integration Test Suite ==="
	$(MAKE) clean-db
	$(MAKE) _phase-3-4
	$(MAKE) _phase-5
	$(MAKE) _phase-6
	$(MAKE) _phase-8-9
	@echo "=== Integration: PASS ==="

test-reembed: check-deps
	@echo "=== Re-Embed Stress Test (Phase 7) ==="
	$(MAKE) clean-db
	$(MAKE) _phase-7
	@echo "=== Re-Embed: PASS ==="

test-kg: check-deps
	@echo "=== KG Test ==="
	$(MAKE) clean-db
	$(MAKE) _phase-6
	@echo "=== KG: PASS ==="

# ─── Internal test phases ───────────────────────────────────────

_check-server:
	@curl -s --connect-timeout 3 http://127.0.0.1:$(PORT)/ > /dev/null \
	  || (echo "Server not running"; exit 1)

_phase-3-4:
	@echo "--- Phase 3+4: Startup + Initialize ---"
	-systemctl --user stop agentrete.service 2>/dev/null; sleep 1
	@sed -e "s|PATH_TO_BINARY|$(CURDIR)/target/debug/agentrete|" \
	  -e "s|PATH_TO_CONFIG|$(TEST_CFG)|" \
	  -e "s|9092|$(PORT)|" \
	  docs/agentrete.service.in > ~/.config/systemd/user/agentrete.service
	-systemctl --user daemon-reload 2>/dev/null
	systemctl --user restart agentrete.service
	@sleep 4
	@curl -s http://127.0.0.1:$(PORT)/ | jq '.status' | grep -q '"ok"' \
	  || (echo "FAIL: health"; exit 1)
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"initialize","params":{"protocolVersion":"2025-11-25"},"id":1}' \
	  | jq '.result.capabilities.tasks' | grep -q '{}' \
	  || (echo "FAIL: tasks capability"; exit 1)
	@curl -s http://127.0.0.1:$(PORT)/ -d '{"method":"tools/list","id":2}' \
	  | jq '.result.tools | length' | grep -q '9' \
	  || (echo "FAIL: 9 tools"; exit 1)
	@echo "PASS: Phase 3+4"

_phase-5:
	@echo "--- Phase 5: Memory Ops ---"
	@ID=$$(curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"makefile test","type":"test"}},"id":3}' \
	  | jq -r '.result.content[0].text' | grep -oP 'mem_[a-f0-9-]+'); \
	  echo "Saved: $$ID"; \
	  curl -s http://127.0.0.1:$(PORT)/ \
	  -d "{\"method\":\"tools/call\",\"params\":{\"name\":\"memory_forget\",\"arguments\":{\"id\":\"$$ID\"}},\"id\":4}" \
	  | jq -r '.result.content[0].text' | grep -q "Deleted" \
	  || (echo "FAIL: forget"; exit 1)
	@echo "PASS: Phase 5"

_phase-6:
	@echo "--- Phase 6: Knowledge Graph ---"
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":"/data/work/agentrete","watch":false}},"id":10}' \
	  | jq '.result.content[0].text' | grep -q 'running' \
	  || (echo "FAIL: scan"; exit 1)
	@sleep 10
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tasks/status","params":{"id":"task_0001"},"id":11}' \
	  | jq '.result.content[0].text' | grep -q 'completed' \
	  || (echo "FAIL: scan complete"; exit 1)
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":"agentrete"}},"id":12}' \
	  | jq -r '.result.content[0].text' | grep -q 'Relations' \
	  || (echo "FAIL: kg query"; exit 1)
	@echo "PASS: Phase 6"

_phase-7:
	@echo "--- Phase 7: Re-Embed ---"
	-systemctl --user stop agentrete.service 2>/dev/null; sleep 1
	@sed -e "s|PATH_TO_BINARY|$(CURDIR)/target/debug/agentrete|" \
	  -e "s|PATH_TO_CONFIG|$(TEST_CFG)|" \
	  -e "s|9092|$(PORT)|" \
	  docs/agentrete.service.in > ~/.config/systemd/user/agentrete.service
	-systemctl --user daemon-reload 2>/dev/null
	systemctl --user restart agentrete.service
	@sleep 5
	python3 /tmp/insert_10k.py $(TEST_DB)/memory.db
	@sleep 20
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":1}' \
	  | jq -r '.result.content[0].text' | head -1 | grep -q '10001' \
	  || (echo "FAIL: count"; exit 1)
	@pkill -f "agentrete.*mcp" 2>/dev/null; sleep 1
	@sed 's/dims = 256/dims = 512/; s/bge-small-256d/bge-small-512d/' $(TEST_CFG) > /tmp/test-m2v-v2.toml
	@rm -f $(TEST_DB)/memory.db-wal $(TEST_DB)/memory.db-shm
	$(CARGO) run --bin agentrete -- -c /tmp/test-m2v-v2.toml mcp -p $(PORT) &
	@sleep 5
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":2}' \
	  | jq -r '.result.content[0].text' | head -1 | grep -q '10001' \
	  || (echo "FAIL: restart"; exit 1)
	@sleep 30
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":3}' \
	  | jq -r '.result.content[0].text' | grep -q '512d' \
	  || (echo "FAIL: dims"; exit 1)
	@echo "PASS: Phase 7"

_phase-8-9:
	@echo "--- Phase 8+9: Edge + Panic ---"
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":""}},"id":13}' \
	  | jq -r '.error.message' | grep -q 'requires' \
	  || (echo "FAIL: empty entity"; exit 1)
	@curl -s http://127.0.0.1:$(PORT)/ \
	  -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":"/nonexistent","watch":false}},"id":14}' \
	  | jq '.result.content[0].text' | grep -q 'running' \
	  || (echo "FAIL: scan start"; exit 1)
	@sleep 3
	@curl -s --connect-timeout 2 http://127.0.0.1:$(PORT)/ | jq '.status' | grep -q '"ok"' \
	  || (echo "FAIL: server crashed"; exit 1)
	@echo "PASS: Phase 8+9"

# ─── sqlite-vec Extension Download ──────────────────────────────

VEC_VERSION ?= 0.1.9
VEC_REPO   ?= asg017/sqlite-vec
EXT_DIR    ?= ext

# Download and extract sqlite-vec loadable extensions for all platforms
# Download sqlite-vec loadable extensions for all platforms



# Upgrade to latest version
# Download sqlite-vec loadable extensions for all platforms
download-ext:
	@echo "=== Downloading sqlite-vec v$(VEC_VERSION) ==="
	@mkdir -p $(EXT_DIR)
	curl -sL -o /tmp/vec0-linux-x86_64.tar.gz "https://github.com/$(VEC_REPO)/releases/download/v$(VEC_VERSION)/sqlite-vec-$(VEC_VERSION)-loadable-linux-x86_64.tar.gz"
	curl -sL -o /tmp/vec0-linux-aarch64.tar.gz "https://github.com/$(VEC_REPO)/releases/download/v$(VEC_VERSION)/sqlite-vec-$(VEC_VERSION)-loadable-linux-aarch64.tar.gz"
	curl -sL -o /tmp/vec0-macos-x86_64.tar.gz "https://github.com/$(VEC_REPO)/releases/download/v$(VEC_VERSION)/sqlite-vec-$(VEC_VERSION)-loadable-macos-x86_64.tar.gz"
	curl -sL -o /tmp/vec0-macos-aarch64.tar.gz "https://github.com/$(VEC_REPO)/releases/download/v$(VEC_VERSION)/sqlite-vec-$(VEC_VERSION)-loadable-macos-aarch64.tar.gz"
	curl -sL -o /tmp/vec0-windows-x86_64.tar.gz "https://github.com/$(VEC_REPO)/releases/download/v$(VEC_VERSION)/sqlite-vec-$(VEC_VERSION)-loadable-windows-x86_64.tar.gz"
	cd /tmp && tar xzf vec0-linux-x86_64.tar.gz && cp vec0 $(EXT_DIR)/vec0-linux-x86_64.so 2>/dev/null || true
	cd /tmp && tar xzf vec0-linux-aarch64.tar.gz && cp vec0 $(EXT_DIR)/vec0-linux-aarch64.so 2>/dev/null || true
	cd /tmp && tar xzf vec0-macos-x86_64.tar.gz && cp vec0 $(EXT_DIR)/vec0-macos-x86_64.dylib 2>/dev/null || true
	cd /tmp && tar xzf vec0-macos-aarch64.tar.gz && cp vec0 $(EXT_DIR)/vec0-macos-aarch64.dylib 2>/dev/null || true
	cd /tmp && tar xzf vec0-windows-x86_64.tar.gz && cp vec0.dll $(EXT_DIR)/vec0-windows-x86_64.dll 2>/dev/null || true
	@echo "Done. Extensions in $(EXT_DIR)/"
	ls -la $(EXT_DIR)/

download-ext-upgrade:
	@echo "Fetching latest version..."
	 LATEST=$$(curl -sL "https://api.github.com/repos/$(VEC_REPO)/releases/latest" | python3 -c "import sys,json; print(json.load(sys.stdin)['tag_name'].lstrip('v'))"); \\
	$(MAKE) download-ext VEC_VERSION=$$LATEST


# ─── Run ────────────────────────────────────────────────────────

run-mcp:
	$(CARGO) run --bin agentrete -- -c $(TEST_CFG) mcp -p $(PORT)

run-scan:
	$(CARGO) run --bin agentrete -- scan $(SCAN_PATH)

seed:
	$(CARGO) run --bin agentrete -- seed

doctor:
	$(CARGO) run --bin agentrete -- doctor

# ─── Dependencies ────────────────────────────────────────────────

check-deps:
	@which sg > /dev/null 2>&1 || (echo "ERROR: install ast-grep: cargo install ast-grep"; exit 1)
	@ls $(M2V_256)/model.safetensors > /dev/null 2>&1 || (echo "ERROR: 256d model not found"; exit 1)
	@ls $(M2V_512)/model.safetensors > /dev/null 2>&1 || (echo "ERROR: 512d model not found"; exit 1)
	@which jq > /dev/null 2>&1 || (echo "ERROR: install jq"; exit 1)
	@echo "Dependencies: OK"

install-deps:
	$(CARGO) install ast-grep

# ─── Clean ────────────────────────────────────────────────────────

clean-db:
	rm -rf $(TEST_DB)
	mkdir -p $(TEST_DB)

clean:
	$(CARGO) clean

clean-all: clean clean-db

# ─── Quick Commands ──────────────────────────────────────────────

stats:
	$(CARGO) run --bin agentrete -- stats

list:
	$(CARGO) run --bin agentrete -- list

search:
	$(CARGO) run --bin agentrete -- search "$(QUERY)"

save:
	$(CARGO) run --bin agentrete -- save "$(CONTENT)"

# ─── Help ────────────────────────────────────────────────────────

help:
	@echo "Agentrete Makefile"
	@echo ""
	@echo "Build & Lint:"
	@echo "  make build         cargo build"
	@echo "  make release       cargo build --release"
	@echo "  make fmt           cargo fmt"
	@echo "  make clippy        cargo clippy -D warnings"
	@echo "  make lint          fmt-check + clippy"
	@echo ""
	@echo "Test:"
	@echo "  make test          cargo test (33 cases)"
	@echo "  make test-integration  Phase 3-9 (M2V + sg required)"
	@echo "  make test-reembed      Phase 7 only"
	@echo "  make test-kg           Phase 6 only"
	@echo ""
	@echo "Extensions:"
	@echo "  make download-ext           Download sqlite-vec for all platforms"
	@echo "  make download-ext-upgrade   Upgrade to latest version"
	@echo ""
	@echo "Run:"
	@echo "  make run-scan      SCAN_PATH=/path make run-scan"
	@echo "  make seed          Seed community rules"
	@echo "  make stats/make list"
	@echo "  make search QUERY=xxx"
	@echo "  make save CONTENT=xxx"
	@echo ""
	@echo "Dependencies:"
	@echo "  make check-deps    Verify sg + model2vec + jq"
	@echo "  make install-deps  Install ast-grep"
	@echo ""
	@echo "Clean:"
	@echo "  make clean         cargo clean"
	@echo "  make clean-db      Remove test DB"
