// Minimal EFI stub to boot seL4 via multiboot2
#include <efi.h>
#include <efilib.h>

// COM1 port
#define COM1 0x3f8

static void outb(unsigned short port, unsigned char val) {
    __asm__ volatile("outb %0, %1" : : "a"(val), "Nd"(port));
}

static void serial_putc(char c) {
    // Wait for transmit buffer empty
    while (!(__inbyte(COM1 + 5) & 0x20));
    __outbyte(COM1, c);
}

static void serial_puts(const char *s) {
    while (*s) serial_putc(*s++);
}

// Multiboot2 header magic
#define MB2_MAGIC 0xe85250d6
#define MB2_ARCH_X86_64 4

// We need to load kernel.elf and root-task from the EFI file system
// For simplicity, we'll embed the multiboot info and jump to kernel entry

EFI_STATUS efi_main(EFI_HANDLE ImageHandle, EFI_SYSTEM_TABLE *SystemTable) {
    (void)ImageHandle;
    (void)SystemTable;

    // Initialize COM1
    __outbyte(COM1 + 1, 0x00);  // Disable interrupts
    __outbyte(COM1 + 3, 0x80);  // Enable DLAB
    __outbyte(COM1 + 0, 0x01);  // Set divisor lo (115200 baud)
    __outbyte(COM1 + 1, 0x00);  // Set divisor hi
    __outbyte(COM1 + 3, 0x03);  // 8N1
    __outbyte(COM1 + 2, 0xC7);  // Enable FIFO
    __outbyte(COM1 + 4, 0x0B);  // IRQs enabled, RTS/DSR set

    serial_puts("EFI: boot_sel4 started\n");

    // TODO: Load kernel.elf and root-task from HTTP or embedded
    // For now, just halt
    serial_puts("EFI: halting (stub)\n");

    for (;;) {
        __asm__ volatile("hlt");
    }

    return EFI_SUCCESS;
}
