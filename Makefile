SHELL := /bin/sh

SKILL_DIR := skills/reclaim-disk-space
SCRIPTS := $(SKILL_DIR)/scripts
ROOT ?= $(HOME)/Library
PROFILE ?= auto
OUT ?= /tmp/reclaim-disk-space.tsv
CONFIRM ?=

.PHONY: build test scan plan delete profile clean

build:
	$(SCRIPTS)/build-disk-scout.sh
	$(SCRIPTS)/build-disk-clean.sh
	$(SCRIPTS)/build-fsevents-since.sh

test: build
	./scripts/test-safety.sh
	./scripts/test-release.sh
	./scripts/test-cleaner.sh
	./scripts/test-incremental.sh
	./scripts/test-scout.sh

scan: build
	$(SCRIPTS)/run-disk-scout.sh "$(ROOT)" "$(PROFILE)" > "$(OUT)"
	@echo "Wrote $(OUT)"

plan: build
	$(SCRIPTS)/run-disk-clean.sh --root "$(ROOT)"

delete: build
	@test -n "$(CONFIRM)" || (echo 'Refusing deletion: pass CONFIRM=/exact/canonical/path' >&2; exit 2)
	$(SCRIPTS)/run-disk-clean.sh --root "$(CONFIRM)" --execute --confirm "$(CONFIRM)" --workers auto --profile interactive

profile: build
	./scripts/benchmark-scan.sh "$(ROOT)" "1 2 4 8 16"
	./scripts/benchmark-clean.sh "1 2 4 8" 8192

clean:
	@echo 'Use make delete CONFIRM=/exact/canonical/path for guarded deletion.'
