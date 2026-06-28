//! Faz 3 — Çok basit bir ELF32 yükleyici.
//!
//! Ham bayt dizisi yerine GERÇEK bir program ikilisini (ELF) çalıştırabilmek
//! için, ELF başlığını ve program başlıklarını (program headers) ayrıştırıp
//! yüklenebilir (`PT_LOAD`) segmentleri kullanıcı adres alanına eşleriz.
//!
//! Yükleyici, segmentleri sayfa sayfa kullanıcı sayfalarına (`mem::user_map_page`)
//! eşler, dosya içeriğini kopyalar, kalanını (.bss) sıfır bırakır ve giriş
//! noktası (`e_entry`) ile kullanıcı yığın tepesini döndürür.
//!
//! Yalnızca i386 (ELFCLASS32, EM_386), statik, `PT_LOAD` segmentli ikilileri
//! destekler — bizim ürettiğimiz `userprog.elf` için yeterlidir.

use crate::mem;

const PT_LOAD: u32 = 1;
const PF_W: u32 = 2;

fn rd16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
fn rd32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// Bir ELF imajını kullanıcı adres alanına yükler.
/// Dönüş: `(giris_noktasi, kullanici_yigin_tepesi)`.
///
/// Çağıran, başarıda `enter_user_mode(entry, esp)` çağırmalı ve dönüşte
/// `mem::user_end()` ile temizlemelidir.
pub fn load(image: &[u8]) -> Result<(u32, u32), &'static str> {
    if image.len() < 52 {
        return Err("ELF cok kisa");
    }
    if &image[0..4] != b"\x7fELF" {
        return Err("ELF imzasi yok");
    }
    if image[4] != 1 {
        return Err("32-bit (ELFCLASS32) degil");
    }
    if image[5] != 1 {
        return Err("little-endian degil");
    }
    let e_machine = rd16(image, 18);
    if e_machine != 3 {
        return Err("i386 (EM_386) degil");
    }

    let e_entry = rd32(image, 24);
    let e_phoff = rd32(image, 28) as usize;
    let e_phentsize = rd16(image, 42) as usize;
    let e_phnum = rd16(image, 44) as usize;

    if e_phoff == 0 || e_phnum == 0 {
        return Err("program basligi yok");
    }

    mem::user_begin();

    let mut max_end: u32 = mem::USER_BASE;
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 32 > image.len() {
            mem::user_end();
            return Err("program basligi tasti");
        }
        let p_type = rd32(image, ph);
        if p_type != PT_LOAD {
            continue;
        }
        let p_offset = rd32(image, ph + 4) as usize;
        let p_vaddr = rd32(image, ph + 8);
        let p_filesz = rd32(image, ph + 16);
        let p_memsz = rd32(image, ph + 20);
        let p_flags = rd32(image, ph + 24);
        let writable = p_flags & PF_W != 0;

        // Segment kullanıcı bölgesinde mi?
        if !mem::user_range_ok(p_vaddr, p_memsz) {
            mem::user_end();
            return Err("segment kullanici bolgesi disinda");
        }
        if p_offset + p_filesz as usize > image.len() {
            mem::user_end();
            return Err("segment dosya disinda");
        }

        // Segmentin kapsadığı her sayfayı eşle ve dosya baytlarını kopyala.
        let seg_end = p_vaddr + p_memsz;
        if seg_end > max_end {
            max_end = seg_end;
        }
        let mut va = p_vaddr & !0xFFF;
        while va < seg_end {
            let frame = mem::user_map_page(va, writable);
            let mut off = 0u32;
            while off < 0x1000 {
                let cur = va + off;
                if cur >= p_vaddr && (cur - p_vaddr) < p_filesz {
                    let file_off = p_offset + (cur - p_vaddr) as usize;
                    unsafe {
                        *((frame + off as usize) as *mut u8) = image[file_off];
                    }
                }
                off += 1;
            }
            va += 0x1000;
        }
    }

    // Heap, en yüksek segment sonunun hemen üstünde başlar (sbrk ile büyür).
    mem::user_set_brk(max_end);

    let stack_top = mem::user_setup_stack();
    mem::user_flush();
    Ok((e_entry, stack_top))
}
