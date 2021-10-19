#!/bin/sh
uefi-run --bios /usr/share/ovmf/x64/OVMF_CODE.fd target/x86_64-none-efi/debug/ninos.efi -- -serial 'file:serial.log' -s -S
