#!/bin/bash
# create_alpine_ext4.sh - Download Alpine minirootfs and create ext4 image
#
# Usage: ./tools/create_alpine_ext4.sh [output_image] [size_mb]
#
# Dependencies: wget, tar, mkfs.ext4, mount, umount, sudo

set -e

# Configuration
ALPINE_VERSION="3.21.3"
ALPINE_ARCH="x86_64"
ALPINE_TAR="alpine-minirootfs-${ALPINE_VERSION}-${ALPINE_ARCH}.tar.gz"
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION%.*}/releases/${ALPINE_ARCH}/${ALPINE_TAR}"

# Output configuration
OUTPUT_IMG="${1:-http-boot/alpine.ext4}"
IMG_SIZE_MB="${2:-32}"
MOUNT_DIR="/tmp/alpine_ext4_mount"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check dependencies
check_dependencies() {
    local deps=("wget" "tar" "mkfs.ext4" "mount" "umount")
    for dep in "${deps[@]}"; do
        if ! command -v "$dep" &> /dev/null; then
            log_error "Missing dependency: $dep"
            echo "Install with: sudo apt-get install wget e2fsprogs"
            exit 1
        fi
    done

    # Check if running as root or can use sudo
    if [ "$EUID" -ne 0 ] && ! sudo -n true 2>/dev/null; then
        log_warn "This script requires root privileges for mount/umount"
        log_warn "You may be prompted for your password"
    fi
}

# Download Alpine minirootfs
download_alpine() {
    if [ -f "$ALPINE_TAR" ]; then
        log_info "Alpine tarball already exists: $ALPINE_TAR"
        return 0
    fi

    log_info "Downloading Alpine minirootfs ${ALPINE_VERSION}..."
    log_info "URL: $ALPINE_URL"

    if ! wget -q --show-progress -O "$ALPINE_TAR" "$ALPINE_URL"; then
        log_error "Failed to download Alpine minirootfs"
        rm -f "$ALPINE_TAR"
        exit 1
    fi

    log_info "Download complete: $ALPINE_TAR ($(du -h "$ALPINE_TAR" | cut -f1))"
}

# Create ext4 image with Alpine rootfs
create_image() {
    log_info "Creating ${IMG_SIZE_MB}MB ext4 image: $OUTPUT_IMG"

    # Create temporary directory for Alpine rootfs
    local tmp_dir="/tmp/alpine_rootfs_$$"
    mkdir -p "$tmp_dir"

    # Extract Alpine minirootfs
    log_info "Extracting Alpine minirootfs..."
    tar -xzf "$ALPINE_TAR" -C "$tmp_dir"

    # Create necessary directories
    log_info "Creating system directories..."
    mkdir -p "$tmp_dir"/{dev,proc,sys,tmp,run,mnt,media}

    # Create basic device nodes (requires fakeroot or root)
    log_info "Creating device nodes..."
    mknod -m 666 "$tmp_dir/dev/null" c 1 3 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/zero" c 1 5 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/random" c 1 8 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/urandom" c 1 9 2>/dev/null || true
    mknod -m 620 "$tmp_dir/dev/console" c 5 1 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/tty" c 5 0 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/tty0" c 4 0 2>/dev/null || true
    mknod -m 666 "$tmp_dir/dev/tty1" c 4 1 2>/dev/null || true

    # Create symlinks for busybox if present
    if [ -f "$tmp_dir/bin/busybox" ]; then
        log_info "Creating busybox symlinks..."
        cd "$tmp_dir/bin"
        for cmd in sh ls cat echo mkdir rm cp mv ln chmod chown mount umount \
                   ps kill grep sed awk find vi head tail wc sort uniq tr \
                   date hostname uname id whoami pwd cd export set env \
                   test expr true false yes no sleep sync reboot halt poweroff; do
            ln -sf busybox "$cmd" 2>/dev/null || true
        done
        cd - > /dev/null
    fi

    # Create /etc/resolv.conf for DNS
    echo "nameserver 8.8.8.8" > "$tmp_dir/etc/resolv.conf" 2>/dev/null || true

    # Create /etc/hostname
    echo "sel4-alpine" > "$tmp_dir/etc/hostname" 2>/dev/null || true

    # Create a simple /etc/fstab
    cat << 'FSTAB' > "$tmp_dir/etc/fstab" 2>/dev/null || true
# <file system> <mount point>   <type>  <options>       <dump>  <pass>
proc            /proc           proc    defaults        0       0
sysfs           /sys            sysfs   defaults        0       0
tmpfs           /tmp            tmpfs   defaults        0       0
FSTAB

    # Create init.d directory and init script
    mkdir -p "$tmp_dir/etc/init.d"
    cat << 'INIT' > "$tmp_dir/etc/init.d/rcS"
#!/bin/sh
# Simple init script for seL4

# Mount virtual filesystems
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t tmpfs tmpfs /tmp

# Set PATH
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# Print welcome message
echo "========================================="
echo "  Alpine Linux on seL4"
echo "  Linux Compatible Layer (LCL)"
echo "========================================="
echo ""
INIT
    chmod +x "$tmp_dir/etc/init.d/rcS"

    # Create /etc/profile
    cat << 'PROFILE' > "$tmp_dir/etc/profile" 2>/dev/null || true
export PS1='\u@sel4:\w\$ '
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
PROFILE

    # Create ext4 image using mke2fs -d
    log_info "Creating ext4 image with mke2fs -d..."
    mke2fs -t ext4 -L alpine-root -d "$tmp_dir" -m 0 "$OUTPUT_IMG" "${IMG_SIZE_MB}M" > /dev/null 2>&1

    # Cleanup
    rm -rf "$tmp_dir"

    log_info "Alpine ext4 image created: $OUTPUT_IMG ($(du -h "$OUTPUT_IMG" | cut -f1))"
}

# Print usage information
print_usage() {
    echo ""
    echo "Usage: $0 [output_image] [size_mb]"
    echo ""
    echo "Arguments:"
    echo "  output_image  Output ext4 image path (default: http-boot/alpine.ext4)"
    echo "  size_mb       Image size in MB (default: 32)"
    echo ""
    echo "Examples:"
    echo "  $0                              # Create http-boot/alpine.ext4 (32MB)"
    echo "  $0 my_alpine.ext4 64           # Create 64MB image"
    echo ""
    echo "The script will:"
    echo "  1. Download Alpine minirootfs ${ALPINE_VERSION}"
    echo "  2. Create an ext4 filesystem image"
    echo "  3. Extract Alpine and set up basic system"
    echo "  4. Create busybox symlinks"
    echo ""
}

# Main execution
main() {
    echo "========================================="
    echo "  Alpine ext4 Image Creator"
    echo "  Version: ${ALPINE_VERSION}"
    echo "========================================="
    echo ""

    # Parse arguments
    if [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
        print_usage
        exit 0
    fi

    # Check dependencies
    check_dependencies

    # Download Alpine
    download_alpine

    # Create image with Alpine rootfs
    create_image

    echo ""
    log_info "Done! You can now use the image with:"
    echo "  1. Update root-task to load the image"
    echo "  2. Run: make run"
    echo ""
    log_info "To manually inspect the image:"
    echo "  sudo mkdir -p /tmp/alpine_mount"
    echo "  sudo mount -o loop $OUTPUT_IMG /tmp/alpine_mount"
    echo "  ls /tmp/alpine_mount"
    echo "  sudo umount /tmp/alpine_mount"
}

# Run main function
main "$@"
