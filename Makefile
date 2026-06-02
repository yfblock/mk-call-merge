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

# LAPIC mode: XAPIC (default, works everywhere) or X2APIC (real hardware with many CPUs)
LAPIC_MODE ?= XAPIC

CARGO_FLAGS := --target $(TARGET) --release \
	-Z build-std=core,alloc,compiler_builtins \
	-Z build-std-features=compiler-builtins-mem

.DEFAULT_GOAL := help
.PHONY: build image run run-kvm iso iso-run iso-run-kvm uefi uefi-run uefi-run-kvm clean help kernel patch http-boot http-boot-all http-boot-iso http-boot-grub http-boot-ipxe all

## Build everything (kernel + root-task + image) in one command
all: kernel build image
	@echo "==> All done. Run 'make run' to boot in QEMU."

## Build root-task, blk-task, and lwext4-task
build:
	PATH=/tmp/x86_64-linux-musl-cross/bin:$$PATH cargo build $(CARGO_FLAGS) -p blk-task
	PATH=/tmp/x86_64-linux-musl-cross/bin:$$PATH cargo build $(CARGO_FLAGS) -p lwext4-task
	cargo build $(CARGO_FLAGS) -p root-task

## Apply seL4 UEFI boot patches
patch:
	@test -d $(SEL4_PREFIX)/.git || { echo "==> Cloning seL4..."; git clone $(SEL4_REPO) $(SEL4_PREFIX); }
	@echo "==> Applying seL4 UEFI boot patches..."
	cd $(SEL4_PREFIX) && git apply --check ../support/sel4-uefi-boot.patch 2>/dev/null && \
		git apply ../support/sel4-uefi-boot.patch && echo "  Patch applied." || echo "  Patch already applied or not applicable."

## Build seL4 kernel (clones repo if needed, requires cmake + gcc + ninja)
## Usage: make kernel                     (default XAPIC)
##        make kernel LAPIC_MODE=X2APIC   (for real hardware with x2APIC)
kernel: patch
	@which cmake >/dev/null 2>&1 || { echo "cmake required. Install: apt-get install cmake"; exit 1; }
	@which ninja >/dev/null 2>&1 || { echo "ninja required. Install: apt-get install ninja-build"; exit 1; }
	@mkdir -p $(SEL4_PREFIX)/build
	cd $(SEL4_PREFIX)/build && cmake -G Ninja -DCROSS_COMPILER_PREFIX="" \
		-DKernelPlatform=pc99 -DKernelSel4Arch=x86_64 \
		-DKernelLAPICMode=$(LAPIC_MODE) \
		-DKernelVerificationBuild=OFF \
		-DKernelPrinting=ON \
		-DKernelSupportPCID=OFF .. && \
		ninja kernel.elf
	@echo "==> Kernel built: $(KERNEL_ELF) (LAPIC=$(LAPIC_MODE))"

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
		-vga std \
		-bios /usr/share/ovmf/OVMF.fd \
		-cdrom $(UEFI_ISO_FILE) || true

## Boot UEFI ISO in QEMU with OVMF + KVM
uefi-run-kvm: uefi
	qemu-system-x86_64 -cpu host -enable-kvm \
		-m 512M -nographic -serial mon:stdio -no-reboot \
		-vga std \
		-bios /usr/share/ovmf/OVMF.fd \
		-cdrom $(UEFI_ISO_FILE) || true

## Clean
clean:
	cargo clean
	rm -f $(IMAGE_ELF)

## Build everything for iPXE HTTP boot (kernel + root-task + UEFI ISO)
## Requires: cmake, ninja, gcc, grub-mkrescue, python3 (for iPXE build)
HTTP_BOOT_DIR := http-boot
HTTP_BOOT_IPXE_REPO := https://github.com/ipxe/ipxe.git
HTTP_BOOT_IPXE_PREFIX := $(abspath ipxe)

http-boot-iso: kernel build
	@mkdir -p $(UEFI_ISO_DIR)/boot/grub
	@cp $(KERNEL_ELF) $(UEFI_ISO_DIR)/boot/kernel.elf
	@cp $(APP_ELF) $(UEFI_ISO_DIR)/boot/root-task
	@printf 'insmod acpi\nserial --unit=0 --speed=115200\nterminal_input serial\nterminal_output serial\nset timeout=0\n\n\
multiboot2 /boot/kernel.elf\nmodule2 /boot/root-task\nboot\n' > $(UEFI_ISO_DIR)/boot/grub/grub.cfg
	grub-mkrescue -o $(HTTP_BOOT_DIR)/sel4.iso $(UEFI_ISO_DIR)
	@echo "==> HTTP boot ISO ready: $(HTTP_BOOT_DIR)/sel4.iso"

http-boot-ipxe:
	@test -f $(HTTP_BOOT_DIR)/autoexec.ipxe || { echo "==> $(HTTP_BOOT_DIR)/autoexec.ipxe not found. Create it first."; exit 1; }
	@test -d $(HTTP_BOOT_IPXE_PREFIX)/.git || { echo "==> Cloning iPXE..."; git clone --depth 1 $(HTTP_BOOT_IPXE_REPO) $(HTTP_BOOT_IPXE_PREFIX); }
	cd $(HTTP_BOOT_IPXE_PREFIX)/src && $(MAKE) -j$$(nproc) bin-x86_64-efi/ipxe.efi \
		EMBED=../../$(HTTP_BOOT_DIR)/autoexec.ipxe
	cp $(HTTP_BOOT_IPXE_PREFIX)/src/bin-x86_64-efi/ipxe.efi $(HTTP_BOOT_DIR)/ipxe.efi
	@echo "==> iPXE ready: $(HTTP_BOOT_DIR)/ipxe.efi"

http-boot-grub: http-boot-iso
	@which grub-mkstandalone >/dev/null 2>&1 || { echo "grub-mkstandalone required."; exit 1; }
	grub-mkstandalone \
		--format=x86_64-efi \
		--output=$(HTTP_BOOT_DIR)/grubx64.efi \
		--modules="multiboot multiboot2 serial acpi" \
		--install-modules="multiboot multiboot2 serial acpi" \
		--locales="" --fonts="" \
		"boot/grub/grub.cfg=$(UEFI_ISO_DIR)/boot/grub/grub.cfg" \
		"boot/kernel.elf=$(KERNEL_ELF)" \
		"boot/root-task=$(APP_ELF)"
	@echo "==> GRUB EFI ready: $(HTTP_BOOT_DIR)/grubx64.efi"

## Build all HTTP boot components (iPXE + GRUB + ISO)
http-boot-all: http-boot-iso http-boot-grub http-boot-ipxe
	@echo ""
	@echo "==> HTTP boot files ready in $(HTTP_BOOT_DIR)/"
	@ls -lh $(HTTP_BOOT_DIR)/ipxe.efi $(HTTP_BOOT_DIR)/grubx64.efi $(HTTP_BOOT_DIR)/sel4.iso
	@echo ""
	@echo "  Start HTTP server:  cd $(HTTP_BOOT_DIR) && python3 -m http.server 8000"
	@echo "  Boot via iPXE:      chain http://<server>:8000/grubx64.efi"
	@echo "  Boot via sanboot:   sanboot http://<server>:8000/sel4.iso"

## Start HTTP boot server (default port 8000)
http-boot:
	cd $(HTTP_BOOT_DIR) && python3 -m http.server 8000

## Help
help:
	@echo "rel4-linux-kit — seL4 x86_64 Root Task"
	@echo ""
	@echo "  make all        Full build: clone seL4 + build kernel + root-task + image"
	@echo "  make kernel     Build seL4 kernel (applies patches + clones repo if needed)"
	@echo "                  LAPIC_MODE=XAPIC (default) or LAPIC_MODE=X2APIC"
	@echo "  make patch      Apply seL4 UEFI boot patches"
	@echo "  make build      Build root task"
	@echo "  make run        Build image + boot in QEMU"
	@echo "  make run-kvm    Build image + boot in QEMU with KVM acceleration"
	@echo "  make iso        Build bootable ISO (GRUB multiboot)"
	@echo "  make iso-run    Boot ISO in QEMU"
	@echo "  make iso-run-kvm  Boot ISO in QEMU with KVM"
	@echo "  make uefi       Build UEFI-bootable ISO (OVMF)"
	@echo "  make uefi-run   Boot UEFI ISO in QEMU with OVMF"
	@echo "  make uefi-run-kvm  Boot UEFI ISO in QEMU with OVMF + KVM"
	@echo "  make http-boot-all   Build all HTTP boot components (iPXE + GRUB + ISO)"
	@echo "  make http-boot       Start HTTP boot server (port 8000)"
	@echo "  make http-boot-iso   Build UEFI ISO for HTTP boot"
	@echo "  make http-boot-grub  Build GRUB EFI for HTTP boot"
	@echo "  make http-boot-ipxe  Build iPXE EFI for HTTP boot"
	@echo ""
	@sed -n 's/^## //p' Makefile

