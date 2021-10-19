#!/bin/sh
# replace file:serial.log with mon:stdio to use stdio
uefi-run --bios /usr/share/ovmf/x64/OVMF_CODE.fd target/x86_64-none-efi/debug/ninos.efi -- -serial 'file:serial.log' --no-reboot -no-shutdown -d guest_errors,cpu_reset -nographic | cat -v
