# Flowy Router — common dev & ops targets
#
# Usage:
#   make              # show help
#   make release      # build optimized binary
#   make test         # run tests
#   make start      # start gateway daemon

CARGO       ?= cargo
BIN         := flowy-router
TARGET      ?= target
RELEASE_BIN := $(TARGET)/release/$(BIN)
DEBUG_BIN   := $(TARGET)/debug/$(BIN)

# Override: make start CONFIG=example/config.toml
CONFIG      ?=
CONFIG_FLAG := $(if $(CONFIG),--config $(CONFIG),)

# Release binary if built; otherwise `cargo run --`.
FLOWY       = $(if $(wildcard $(RELEASE_BIN)),$(RELEASE_BIN),$(CARGO) run --)

.PHONY: help build release test check clean install \
        start stop restart status \
        env setup stats stats-global stats-zh stats-global-zh

help:
	@echo "  env              Print resolved paths & config"
	@echo "  setup            Interactive upstream setup wizard"
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

start:
	$(FLOWY) $(CONFIG_FLAG) gateway start

stop:
	$(FLOWY) $(CONFIG_FLAG) gateway stop

restart:
	$(FLOWY) $(CONFIG_FLAG) gateway restart

status:
	$(FLOWY) $(CONFIG_FLAG) gateway status

env:
	$(FLOWY) $(CONFIG_FLAG) env

setup:
	$(FLOWY) $(CONFIG_FLAG) setup

stats:
	$(FLOWY) $(CONFIG_FLAG) stats

stats-global:
	$(FLOWY) $(CONFIG_FLAG) stats --global

stats-zh:
	$(FLOWY) $(CONFIG_FLAG) stats --lang zh

stats-global-zh:
	$(FLOWY) $(CONFIG_FLAG) stats --global --lang zh