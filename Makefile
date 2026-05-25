# Flowy Router — common dev & ops targets
#
# Usage:
#   make              # show help
#   make release      # build optimized binary
#   make test         # run tests
#   make gateway-run  # foreground gateway (dev)

CARGO       ?= cargo
BIN         := flowy
TARGET      ?= target
RELEASE_BIN := $(TARGET)/release/$(BIN)
DEBUG_BIN   := $(TARGET)/debug/$(BIN)

# Override: make gateway-start CONFIG=example/config.toml
CONFIG      ?=
CONFIG_FLAG := $(if $(CONFIG),--config $(CONFIG),)

# Release binary if built; otherwise `cargo run --`.
FLOWY       = $(if $(wildcard $(RELEASE_BIN)),$(RELEASE_BIN),$(CARGO) run --)

.PHONY: help build release test check clean install run \
        gateway-start gateway-stop gateway-restart gateway-status gateway-run \
        env stats stats-global stats-zh

help:
	@echo "Flowy Router — make targets"
	@echo ""
	@echo "  build            Debug build ($(DEBUG_BIN))"
	@echo "  release          Release build ($(RELEASE_BIN))"
	@echo "  test             Run all tests"
	@echo "  check            cargo check (fast compile check)"
	@echo "  clean            Remove $(TARGET)/"
	@echo "  install          cargo install --path . --force"
	@echo "  run              cargo run -- $(CONFIG_FLAG) --help"
	@echo ""
	@echo "  gateway-start    Start gateway daemon"
	@echo "  gateway-stop     Stop gateway"
	@echo "  gateway-restart  Restart gateway"
	@echo "  gateway-status   Show gateway status"
	@echo "  gateway-run      Run gateway in foreground (dev)"
	@echo ""
	@echo "  env              Print resolved paths & config"
	@echo "  stats            Session stats (English)"
	@echo "  stats-global     Global stats from stats.json"
	@echo "  stats-zh         Session stats (Chinese)"
	@echo ""
	@echo "Options:"
	@echo "  CONFIG=path      Pass --config to flowy (e.g. CONFIG=example/config.toml)"

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

check:
	$(CARGO) check

clean:
	$(CARGO) clean

install: release
	$(CARGO) install --path . --force

run:
	$(CARGO) run -- $(CONFIG_FLAG) --help

gateway-start:
	$(FLOWY) $(CONFIG_FLAG) gateway start

gateway-stop:
	$(FLOWY) $(CONFIG_FLAG) gateway stop

gateway-restart:
	$(FLOWY) $(CONFIG_FLAG) gateway restart

gateway-status:
	$(FLOWY) $(CONFIG_FLAG) gateway status

gateway-run:
	$(CARGO) run -- $(CONFIG_FLAG) gateway run

env:
	$(FLOWY) $(CONFIG_FLAG) env

stats:
	$(FLOWY) $(CONFIG_FLAG) stats

stats-global:
	$(FLOWY) $(CONFIG_FLAG) stats --global

stats-zh:
	$(FLOWY) $(CONFIG_FLAG) stats --lang zh

stats-global-zh:
	$(FLOWY) $(CONFIG_FLAG) stats --global --lang zh