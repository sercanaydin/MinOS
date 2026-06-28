//! Faz 3 — Kullanıcı süreçleri için basit dosya tanıtıcı (file descriptor) tablosu.
//!
//! `open` çağrısı RFS dosya sisteminde (kabuğun geçerli dizininde) bir dosya
//! açar ve küçük bir tanıtıcı (fd ≥ 3) döndürür. Okuma kipinde dosyanın tüm
//! içeriği bir tampona (heap'te `Vec`) okunur; yazma kipinde boş bir tampon
//! biriktirilir ve `close` anında dosyaya yazılır (oluştur/üzerine yaz).
//!
//! fd 0 = stdin (klavye), fd 1 = stdout (konsol) — bunlar `idt.rs`'de işlenir;
//! bu modül yalnızca gerçek dosyalarla (fd ≥ 3) ilgilenir.

use crate::fs;
use alloc::vec;
use alloc::vec::Vec;

const MAX_FD: usize = 8;
const FD_BASE: u32 = 3;

/// `open` bayrakları.
#[allow(dead_code)]
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;

struct OpenFile {
    write: bool,
    parent: u8,
    name: [u8; 28],
    name_len: usize,
    buf: Vec<u8>,
    pos: usize,
}

static mut TABLE: [Option<OpenFile>; MAX_FD] = [const { None }; MAX_FD];

fn table() -> &'static mut [Option<OpenFile>; MAX_FD] {
    unsafe { &mut *core::ptr::addr_of_mut!(TABLE) }
}

fn slot(fd: u32) -> Option<&'static mut OpenFile> {
    if fd < FD_BASE {
        return None;
    }
    let idx = (fd - FD_BASE) as usize;
    table().get_mut(idx)?.as_mut()
}

/// Bir dosya açar. Dönüş: fd (≥ 3) ya da hata için -1 (u32 olarak).
pub fn open(name: &[u8], parent: u8, flags: u32) -> u32 {
    if name.is_empty() || name.len() > 28 {
        return u32::MAX;
    }
    let write = flags == O_WRONLY;

    let buf = if write {
        Vec::new()
    } else {
        let mut tmp = vec![0u8; fs::MAX_FILE_SIZE];
        match fs::read_file(name, parent, &mut tmp) {
            Ok(n) => {
                tmp.truncate(n);
                tmp
            }
            Err(_) => return u32::MAX,
        }
    };

    let tbl = table();
    for (i, e) in tbl.iter_mut().enumerate() {
        if e.is_none() {
            let mut nm = [0u8; 28];
            nm[..name.len()].copy_from_slice(name);
            *e = Some(OpenFile {
                write,
                parent,
                name: nm,
                name_len: name.len(),
                buf,
                pos: 0,
            });
            return FD_BASE + i as u32;
        }
    }
    u32::MAX // tablo dolu
}

/// fd'den `dst`'e okur. Dönüş: okunan bayt sayısı (geçersiz fd'de -1).
pub fn read(fd: u32, dst: &mut [u8]) -> u32 {
    let Some(f) = slot(fd) else { return u32::MAX };
    if f.write {
        return u32::MAX;
    }
    let remaining = f.buf.len() - f.pos;
    let n = core::cmp::min(remaining, dst.len());
    dst[..n].copy_from_slice(&f.buf[f.pos..f.pos + n]);
    f.pos += n;
    n as u32
}

/// `src`'i fd'ye yazar (tampona biriktirir). Dönüş: yazılan bayt (geçersiz: -1).
pub fn write(fd: u32, src: &[u8]) -> u32 {
    let Some(f) = slot(fd) else { return u32::MAX };
    if !f.write {
        return u32::MAX;
    }
    f.buf.extend_from_slice(src);
    src.len() as u32
}

/// fd'yi kapatır; yazma kipindeyse tamponu dosyaya yazar. Dönüş: 0 / -1.
pub fn close(fd: u32) -> u32 {
    if fd < FD_BASE {
        return u32::MAX;
    }
    let idx = (fd - FD_BASE) as usize;
    let tbl = table();
    let Some(slot) = tbl.get_mut(idx) else {
        return u32::MAX;
    };
    let Some(f) = slot.take() else {
        return u32::MAX;
    };
    if f.write {
        if fs::write_file(&f.name[..f.name_len], f.parent, &f.buf).is_err() {
            return u32::MAX;
        }
    }
    0
}
