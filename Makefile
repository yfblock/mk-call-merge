# rel4-linux-kit Makefile
# ========================
# Build and boot the seL4 root task on x86_64 via QEMU 10.0.
#
# Uses Linux bzImage format (cloned from system kernel header).
BUILD_DIR := target
TARGET    := x86_64-sel4
APP_ELF   := $(BUILD_DIR)/x86_64-sel4/release/root-task
IMAGE_ELF := $(BUILD_DIR)/image.elf
IMAGE_BZ  := $(BUILD_DIR)/image.bz

SEL4_PREFIX := $(abspath seL4)
INSTALL_DIR := .env/seL4
KERNEL_LOADER_BIN ?= $(INSTALL_DIR)/bin/sel4-kernel-loader
LOADER_CLI := $(INSTALL_DIR)/bin/sel4-kernel-loader-add-payload
LOADER_SRC := /tmp/rust-sel4-patched

CARGO_FLAGS := --target $(TARGET) --release \
	-Z build-std=core,alloc,compiler_builtins \
	-Z build-std-features=compiler-builtins-mem

.DEFAULT_GOAL := help
.PHONY: build image run clean help install-loader

## Build root-task
build:
	cargo build $(CARGO_FLAGS) -p root-task

## Build bootable bzImage
image: build
	@test -f $(KERNEL_LOADER_BIN) || { echo "Run: make install-loader"; exit 1; }
	@test -f $(LOADER_CLI) || { echo "Run: make install-loader"; exit 1; }
	$(LOADER_CLI) --sel4-prefix $(SEL4_PREFIX) --loader $(KERNEL_LOADER_BIN) --app $(APP_ELF) -o $(IMAGE_ELF)
	python3 tools/mk-bzimage.py $(IMAGE_ELF) $(IMAGE_BZ)
	@echo "==> Ready: $(IMAGE_BZ)"

## Boot in QEMU
run: image
	qemu-system-x86_64 -machine q35 -cpu max -m 512M \
		-nographic -serial mon:stdio -kernel $(IMAGE_BZ)

## Build/install kernel-loader (one-time)
install-loader:
	@echo "Building kernel-loader for x86_64..."
	@mkdir -p $(INSTALL_DIR)
	@export SEL4_PREFIX=$(SEL4_PREFIX) && \
	 export CC_x86_64_unknown_none=x86_64-linux-gnu-gcc && \
	 cd $(LOADER_SRC) && cargo clean >/dev/null 2>&1; \
	 cargo install --force --path crates/sel4-kernel-loader \
		--root $(abspath $(INSTALL_DIR)) --target x86_64-unknown-none \
		-Z build-std=core,compiler_builtins \
		-Z build-std-features=compiler-builtins-mem sel4-kernel-loader && \
	 cargo install --force --path crates/sel4-kernel-loader/add-payload \
		--root $(abspath $(INSTALL_DIR)) sel4-kernel-loader-add-payload
	@echo "==> Done"

## Clean
clean:
	cargo clean
	rm -f $(IMAGE_ELF) $(IMAGE_BZ)

## Help
help:
	@echo "rel4-linux-kit — seL4 x86_64 Root Task"
	@echo "  make install-loader   One-time setup"
	@echo "  make run              Build + boot in QEMU"
	@sed -n 's/^## //p' Makefile
