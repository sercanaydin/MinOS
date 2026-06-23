//! Bellek (RAM) tabanlı blok aygıtı.
//!
//! Gerçek bir disk denetleyicimiz olmadığında (ör. modern UEFI makineler: NVMe;
//! ya da hiç disk yokken) dosya sistemini RAM'de tutmak için kullanılır. Veri
//! kalıcı DEĞİLDİR — yeniden başlatınca silinir. Ama masaüstü ve tüm dosya
//! komutları her donanımda çalışır.

use crate::ata::SECTOR_SIZE;

// 2 MiB kapasite (4096 sektör) — disk imajıyla aynı boyut.
pub const SECTORS: u32 = 4096;
const BYTES: usize = SECTORS as usize * SECTOR_SIZE;

static mut DISK: [u8; BYTES] = [0; BYTES];

/// Tek bir sektörü `buf` içine okur. Sınır dışıysa `false`.
pub fn read_sector(lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> bool {
    if lba >= SECTORS {
        return false;
    }
    let base = lba as usize * SECTOR_SIZE;
    let disk = core::ptr::addr_of!(DISK) as *const u8;
    unsafe {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = core::ptr::read(disk.add(base + i));
        }
    }
    true
}

/// Tek bir sektörü `buf`'tan diske yazar. Sınır dışıysa `false`.
pub fn write_sector(lba: u32, buf: &[u8; SECTOR_SIZE]) -> bool {
    if lba >= SECTORS {
        return false;
    }
    let base = lba as usize * SECTOR_SIZE;
    let disk = core::ptr::addr_of_mut!(DISK) as *mut u8;
    unsafe {
        for (i, b) in buf.iter().enumerate() {
            core::ptr::write(disk.add(base + i), *b);
        }
    }
    true
}
