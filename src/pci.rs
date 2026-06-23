//! Çok küçük bir PCI yapılandırma alanı tarayıcısı.
//!
//! İki işi var:
//!   * ekran (VGA) denetleyicisini bulup doğrusal framebuffer adresini öğrenmek
//!     (Bochs-VBE yolu için),
//!   * NVMe denetleyicisini bulup MMIO yazmaç tabanını (BAR0/1) öğrenmek ve
//!     bus master + bellek erişimini etkinleştirmek.
//!
//! PCI yapılandırma alanına 0xCF8 (adres) / 0xCFC (veri) portlarından erişilir.

use crate::port::{inl, outl};

const CONFIG_ADDR: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

// BAR programlanmamışsa atayacağımız adres: PCI deliği içinde, RAM'in üstünde.
const DEFAULT_LFB: u32 = 0xFD00_0000;

fn read32(bus: u8, slot: u8, func: u8, off: u8) -> u32 {
    let addr = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((off as u32) & 0xFC);
    unsafe {
        outl(CONFIG_ADDR, addr);
        inl(CONFIG_DATA)
    }
}

fn write32(bus: u8, slot: u8, func: u8, off: u8, val: u32) {
    let addr = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((off as u32) & 0xFC);
    unsafe {
        outl(CONFIG_ADDR, addr);
        outl(CONFIG_DATA, val);
    }
}

/// Ekran denetleyicisinin doğrusal framebuffer fiziksel adresini döndürür.
/// Gerekirse BAR'ı programlar ve bellek erişimini etkinleştirir.
pub fn find_lfb() -> Option<u32> {
    // QEMU aygıtları 0. veri yolunda; 32 yuvayı taramak yeterli.
    for slot in 0..32u8 {
        let id = read32(0, slot, 0, 0x00);
        if id == 0xFFFF_FFFF {
            continue; // aygıt yok
        }
        // Sınıf kodu üst baytta (base class). 0x03 = ekran denetleyicisi.
        let class = read32(0, slot, 0, 0x08) >> 24;
        if class != 0x03 {
            continue;
        }

        let bar0 = read32(0, slot, 0, 0x10);
        let mut base = bar0 & 0xFFFF_FFF0;
        if base == 0 {
            // BAR atanmamış: kendimiz atayalım.
            write32(0, slot, 0, 0x10, DEFAULT_LFB);
            base = read32(0, slot, 0, 0x10) & 0xFFFF_FFF0;
            if base == 0 {
                base = DEFAULT_LFB;
            }
        }

        // Komut yazmacında bellek erişimini (bit 1) etkinleştir.
        let cmd = read32(0, slot, 0, 0x04);
        write32(0, slot, 0, 0x04, cmd | 0x02);

        return Some(base);
    }
    None
}

/// Intel e1000 (82540EM) ağ kartının MMIO yazmaç tabanını döndürür.
/// Bellek erişimi + bus master'ı (DMA) etkinleştirir.
pub fn find_e1000() -> Option<u32> {
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            for func in 0..8u8 {
                let id = read32(bus, slot, func, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let vendor = id & 0xFFFF;
                let device = id >> 16;
                // Intel (0x8086) + e1000 aileleri (82540EM=0x100E, 82545=0x100F vb.).
                let is_e1000 = vendor == 0x8086
                    && matches!(device, 0x100E | 0x100F | 0x10D3 | 0x153A | 0x1533);
                if !is_e1000 {
                    if func == 0 {
                        let hdr = (read32(bus, slot, 0, 0x0C) >> 16) & 0xFF;
                        if hdr & 0x80 == 0 {
                            break;
                        }
                    }
                    continue;
                }

                let bar0 = read32(bus, slot, func, 0x10);
                let base = bar0 & 0xFFFF_FFF0;
                if base == 0 {
                    continue;
                }
                let cmd = read32(bus, slot, func, 0x04);
                write32(bus, slot, func, 0x04, cmd | 0x06); // mem + bus master
                return Some(base);
            }
        }
    }
    None
}

/// NVMe denetleyicisinin MMIO yazmaç tabanı (64-bit fiziksel adres).
pub fn find_nvme() -> Option<u64> {
    // NVMe gerçek donanımda PCIe kök portunun arkasında (sıfır olmayan bir veri
    // yolunda) olabilir; bu yüzden tüm veri yollarını tararız.
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            for func in 0..8u8 {
                let id = read32(bus, slot, func, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                // class (byte3)=0x01 mass storage, subclass (byte2)=0x08 NVM,
                // prog-IF (byte1)=0x02 NVMe.
                let cls = read32(bus, slot, func, 0x08);
                let base_class = (cls >> 24) & 0xFF;
                let sub_class = (cls >> 16) & 0xFF;
                let prog_if = (cls >> 8) & 0xFF;
                if base_class != 0x01 || sub_class != 0x08 || prog_if != 0x02 {
                    // Çok fonksiyonlu değilse fonksiyon taramasını kısalt.
                    if func == 0 {
                        let hdr = (read32(bus, slot, 0, 0x0C) >> 16) & 0xFF;
                        if hdr & 0x80 == 0 {
                            break; // tek fonksiyonlu aygıt
                        }
                    }
                    continue;
                }

                // BAR0/BAR1: 64-bit MMIO tabanı.
                let bar0 = read32(bus, slot, func, 0x10);
                let bar1 = read32(bus, slot, func, 0x14);
                let lo = (bar0 & 0xFFFF_FFF0) as u64;
                let hi = bar1 as u64;
                let mut mmio = (hi << 32) | lo;

                // 32-bit çekirdek 4 GB üstünü adresleyemez. UEFI firmware'i
                // 64-bit BAR'ı genelde 4 GB üstüne koyar; bu durumda BAR'ı
                // 4 GB altında, bilinen bir MMIO adresine yeniden programlarız.
                if mmio == 0 || mmio >= 0x1_0000_0000 {
                    const NVME_MMIO: u32 = 0xFE00_0000;
                    let cmd = read32(bus, slot, func, 0x04);
                    write32(bus, slot, func, 0x04, cmd & !0x06); // decode kapat
                    write32(bus, slot, func, 0x10, NVME_MMIO | (bar0 & 0x0F));
                    write32(bus, slot, func, 0x14, 0); // yüksek dword = 0
                    mmio = NVME_MMIO as u64;
                }

                // Komut: bellek erişimi (bit1) + bus master (bit2 DMA) aç.
                let cmd = read32(bus, slot, func, 0x04);
                write32(bus, slot, func, 0x04, cmd | 0x06);

                return Some(mmio);
            }
        }
    }
    None
}
