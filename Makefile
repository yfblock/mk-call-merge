# rel4-linux-kit Makefile
# ========================
# Build and boot the seL4 root task on x86_64 via QEMU.
#
# x86_64 does NOT use a kernel loader. Instead, mkimage.py creates a
# 32-bit ELF wrapper for the kernel, and QEMU passes root-task via -initrd.
BUILD_DIR := target
TARGET    := x86_64-sel4
APP_ELF   := $(BUILD_DIR)/x86_64-sel4/release/root-task
IMAGE_ELF := $(BUILD_DIR)/image.elf
ISO_DIR   := $(BUILD_DIR)/iso
ISO_FILE  := $(BUILD_DIR)/sel4.iso

# seL4 kernel source (cloned from GitHub if not present)
SEL4_REPO  := https://github.com/seL4/seL4.git
SEL4_PREFIX := $(abspath seL4)
KERNEL_ELF := $(SEL4_PREFIX)/build/kernel.elf

CARGO_FLAGS := --target $(TARGET) --release \
	-Z build-std=core,alloc,compiler_builtins \
	-Z build-std-features=compiler-builtins-mem

.DEFAULT_GOAL := help
.PHONY: build image run run-kvm iso iso-run iso-run-kvm uefi uefi-run uefi-run-kvm clean help kernel all

## Build everything (kernel + root-task + image) in one command
all: kernel build image
	@echo "==> All done. Run 'make run' to boot in QEMU."

## Build root-task
build:
	cargo build $(CARGO_FLAGS) -p root-task

## Build seL4 kernel (clones repo if needed, requires cmake + gcc)
kernel:
	@which cmake >/dev/null 2>&1 || { echo "cmake required. Install: apt-get install cmake"; exit 1; }
	@test -d $(SEL4_PREFIX)/.git || { echo "==> Cloning seL4..."; git clone $(SEL4_REPO) $(SEL4_PREFIX); }
	@mkdir -p $(SEL4_PREFIX)/build
	cd $(SEL4_PREFIX)/build && cmake -DCROSS_COMPILER_PREFIX="" \
		-DKernelPlatform=pc99 -DKernelSel4Arch=x86_64 .. && \
		make kernel.elf
	@echo "==> Kernel built: $(KERNEL_ELF)"

## Build bootable image (32-bit ELF kernel wrapper via objcopy)
image: build
	@test -f $(KERNEL_ELF) || { echo "Run: make kernel"; exit 1; }
	objcopy -O elf32-i386 $(KERNEL_ELF) $(IMAGE_ELF)
	@echo "==> Ready: $(IMAGE_ELF)"

## Boot in QEMU (root-task passed via -initrd as multiboot module)
run: image
	qemu-system-x86_64 -cpu max \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-device isa-debug-exit \
		-kernel $(IMAGE_ELF) -initrd $(APP_ELF) || true

## Boot in QEMU with KVM acceleration
run-kvm: image
	qemu-system-x86_64 -cpu host -enable-kvm \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-device isa-debug-exit \
		-kernel $(IMAGE_ELF) -initrd $(APP_ELF) || true

## Build bootable ISO (GRUB multiboot, can run on real hardware)
## Auto-downloads grub-pc-bin if /usr/lib/grub/i386-pc is missing.
GRUB_I386_PC := /usr/lib/grub/i386-pc
GRUB_I386_PC_LOCAL := $(BUILD_DIR)/grub-i386-pc

iso: image
	@if [ ! -d $(GRUB_I386_PC) ] && [ ! -d $(GRUB_I386_PC_LOCAL) ]; then \
		echo "==> grub-pc-bin not found, downloading..."; \
		mkdir -p $(GRUB_I386_PC_LOCAL); \
		cd $(BUILD_DIR) && apt-get download grub-pc-bin 2>/dev/null && \
		dpkg-deb -x grub-pc-bin_*.deb grub-pc-bin-extract && \
		cp -r grub-pc-bin-extract/usr/lib/grub/i386-pc/* $(GRUB_I386_PC_LOCAL)/ && \
		rm -rf grub-pc-bin-extract grub-pc-bin_*.deb && \
		echo "==> Extracted i386-pc GRUB modules locally"; \
	fi
	@mkdir -p $(ISO_DIR)/boot/grub
	@cp $(IMAGE_ELF) $(ISO_DIR)/boot/kernel.elf
	@cp $(APP_ELF) $(ISO_DIR)/boot/root-task
	@printf 'serial --unit=0 --speed=115200\nterminal_input serial console\nterminal_output serial console\n\n\
set timeout=0\nset default=0\n\n\
menuentry "seL4" {\n\
    multiboot /boot/kernel.elf\n\
    module /boot/root-task\n\
    boot\n\
}\n' > $(ISO_DIR)/boot/grub/grub.cfg
	@if [ -d $(GRUB_I386_PC) ]; then \
		grub-mkrescue -o $(ISO_FILE) $(ISO_DIR); \
	else \
		grub-mkrescue -d $(GRUB_I386_PC_LOCAL) -o $(ISO_FILE) $(ISO_DIR); \
	fi
	@echo "==> ISO ready: $(ISO_FILE)"

## Boot ISO in QEMU (BIOS mode, slow without KVM)
iso-run: iso
	qemu-system-x86_64 -cpu max \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-device isa-debug-exit \
		-cdrom $(ISO_FILE) || true

## Boot ISO in QEMU with KVM acceleration (recommended)
iso-run-kvm: iso
	qemu-system-x86_64 -cpu host -enable-kvm \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-device isa-debug-exit \
		-cdrom $(ISO_FILE) || true

## Build UEFI-bootable ISO (GRUB with EFI support)
## Auto-downloads grub-efi-amd64-bin if needed.
GRUB_X86_64_EFI := /usr/lib/grub/x86_64-efi
GRUB_X86_64_EFI_LOCAL := $(BUILD_DIR)/grub-x86_64-efi
UEFI_ISO_DIR := $(BUILD_DIR)/uefi-iso
UEFI_ISO_FILE := $(BUILD_DIR)/sel4-uefi.iso

uefi: image
	@if [ ! -d $(GRUB_X86_64_EFI) ] && [ ! -d $(GRUB_X86_64_EFI_LOCAL) ]; then \
		echo "==> grub-efi-amd64-bin not found, downloading..."; \
		mkdir -p $(GRUB_X86_64_EFI_LOCAL); \
		cd $(BUILD_DIR) && apt-get download grub-efi-amd64-bin 2>/dev/null && \
		dpkg-deb -x grub-efi-amd64-bin_*.deb grub-efi-amd64-extract && \
		cp -r grub-efi-amd64-extract/usr/lib/grub/x86_64-efi/* $(GRUB_X86_64_EFI_LOCAL)/ && \
		rm -rf grub-efi-amd64-extract grub-efi-amd64-bin_*.deb && \
		echo "==> Extracted x86_64-efi GRUB modules locally"; \
	fi
	@mkdir -p $(UEFI_ISO_DIR)/boot/grub
	@cp $(IMAGE_ELF) $(UEFI_ISO_DIR)/boot/kernel.elf
	@cp $(APP_ELF) $(UEFI_ISO_DIR)/boot/root-task
	@printf 'serial --unit=0 --speed=115200\nterminal_input serial console\nterminal_output serial console\n\n\
set timeout=0\nset default=0\n\n\
menuentry "seL4 (UEFI)" {\n\
    multiboot /boot/kernel.elf\n\
    module /boot/root-task\n\
    boot\n\
}\n' > $(UEFI_ISO_DIR)/boot/grub/grub.cfg
	@if [ -d $(GRUB_X86_64_EFI) ]; then \
		grub-mkrescue -o $(UEFI_ISO_FILE) $(UEFI_ISO_DIR); \
	else \
		grub-mkrescue -d $(GRUB_X86_64_EFI_LOCAL) -o $(UEFI_ISO_FILE) $(UEFI_ISO_DIR); \
	fi
	@echo "==> UEFI ISO ready: $(UEFI_ISO_FILE)"

## Boot UEFI ISO in QEMU with OVMF (shows seL4 UEFI boot failure)
uefi-run: uefi
	qemu-system-x86_64 -cpu max \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-bios /usr/share/ovmf/OVMF.fd \
		-cdrom $(UEFI_ISO_FILE) || true

## Boot UEFI ISO in QEMU with OVMF + KVM
uefi-run-kvm: uefi
	qemu-system-x86_64 -cpu host -enable-kvm \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-bios /usr/share/ovmf/OVMF.fd \
		-cdrom $(UEFI_ISO_FILE) || true

## Clean
clean:
	cargo clean
	rm -f $(IMAGE_ELF)

## Help
help:
	@echo "rel4-linux-kit — seL4 x86_64 Root Task"
	@echo ""
	@echo "  make all        Full build: clone seL4 + build kernel + root-task + image"
	@echo "  make kernel     Build seL4 kernel (clones repo if needed)"
	@echo "  make build      Build root task"
	@echo "  make run        Build image + boot in QEMU"
	@echo "  make run-kvm    Build image + boot in QEMU with KVM acceleration"
	@echo "  make iso        Build bootable ISO (GRUB multiboot)"
	@echo "  make iso-run    Boot ISO in QEMU"
	@echo "  make iso-run-kvm  Boot ISO in QEMU with KVM"
	@echo "  make uefi       Build UEFI-bootable ISO (OVMF)"
	@echo "  make uefi-run   Boot UEFI ISO in QEMU with OVMF"
	@echo "  make uefi-run-kvm  Boot UEFI ISO in QEMU with OVMF + KVM"
	@echo ""
	@sed -n 's/^## //p' Makefile
