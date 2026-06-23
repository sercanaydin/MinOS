//! RFS — küçük, öğretici bir dosya sistemi.
//!
//! Disk düzeni (her blok = 1 sektör = 512 bayt):
//!   Blok 0          : Superblock (sihirli sayı, sürüm, toplam blok)
//!   Blok 1          : Blok ayırma haritası (bitmap) — 1 bit = 1 blok
//!   Blok 2..10      : Dizin (8 blok × 4 girdi = 32 dosya)
//!   Blok 10..       : Veri blokları
//!
//! Dizin girdisi (128 bayt):
//!   ad[28] + used(1) + kind(1) + parent+1(1) + ayrılmış(1) + size(4)
//!   + block_count(4) + blocks[22]*4.
//! Her dosya en çok 22 doğrudan bloğa (≈ 11 KiB) sahip olabilir.
//!
//! Dizin desteği "üst işaretçisi" modeliyle: her girdi hangi dizinin içinde
//! olduğunu `parent` ile tutar. Kök dizin = ROOT. `kind` 0 ise dosya, 1 ise
//! dizindir. (parent baytı diskte +1 saklanır; 0 = kök. Böylece kind/parent=0
//! olan eski biçimlenmiş diskler otomatik olarak köke düşer — geriye uyumlu.)

use crate::ata::{self, SECTOR_SIZE};

const MAGIC: u32 = 0x52465321; // "RFS!"
const VERSION: u32 = 1;

const BITMAP_BLOCK: u32 = 1;
const DIR_START: u32 = 2;
const DIR_BLOCKS: u32 = 8;
const DATA_START: u32 = DIR_START + DIR_BLOCKS; // = 10

const ENTRY_SIZE: usize = 128;
const ENTRIES_PER_BLOCK: usize = SECTOR_SIZE / ENTRY_SIZE; // 4
pub const MAX_FILES: usize = (DIR_BLOCKS as usize) * ENTRIES_PER_BLOCK; // 32
const MAX_NAME: usize = 28;
const MAX_BLOCKS_PER_FILE: usize = 22;
pub const MAX_FILE_SIZE: usize = MAX_BLOCKS_PER_FILE * SECTOR_SIZE; // 11264

/// Mantıksal kök dizin kimliği.
pub const ROOT: u8 = 0xFF;
/// Girdi türleri.
pub const KIND_FILE: u8 = 0;
pub const KIND_DIR: u8 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFormatted,
    NotFound,
    TooBig,
    NoSpace,
    DiskError,
    NameTooLong,
    DirFull,
    NotEmpty,
    Exists,
    NotDir,
}

impl FsError {
    pub fn message(self) -> &'static str {
        match self {
            FsError::NotFormatted => "disk biçimlenmemiş (önce 'format' çalıştırın)",
            FsError::NotFound => "bulunamadı",
            FsError::TooBig => "dosya çok büyük (en fazla ~11 KiB)",
            FsError::NoSpace => "diskte yer yok",
            FsError::DiskError => "disk G/Ç hatası",
            FsError::NameTooLong => "ad çok uzun (en fazla 28 karakter)",
            FsError::DirFull => "girdi tablosu dolu (en fazla 32 girdi)",
            FsError::NotEmpty => "dizin boş değil",
            FsError::Exists => "bu ad zaten kullanımda",
            FsError::NotDir => "bir dizin değil",
        }
    }
}

static mut TOTAL_BLOCKS: u32 = 0;

// Oku-değiştir-yaz işlemleri için paylaşılan geçici tampon.
static mut SCRATCH: [u8; MAX_FILE_SIZE] = [0; MAX_FILE_SIZE];

pub struct FileInfo {
    pub name: [u8; MAX_NAME],
    pub size: u32,
    pub kind: u8,
    pub parent: u8,
}

impl FileInfo {
    pub fn is_dir(&self) -> bool {
        self.kind == KIND_DIR
    }
}

struct Entry {
    name: [u8; MAX_NAME],
    used: bool,
    kind: u8,
    parent: u8,
    size: u32,
    block_count: u32,
    blocks: [u32; MAX_BLOCKS_PER_FILE],
}

impl Entry {
    const EMPTY: Entry = Entry {
        name: [0; MAX_NAME],
        used: false,
        kind: KIND_FILE,
        parent: ROOT,
        size: 0,
        block_count: 0,
        blocks: [0; MAX_BLOCKS_PER_FILE],
    };
}

// --- Düşük seviyeli yardımcılar ---

// Blok aygıtı arka ucu seçimi.
//   0 = ATA/IDE (kalıcı)   1 = RAM diski (kalıcı değil)   2 = NVMe (kalıcı)
use core::sync::atomic::{AtomicU8, Ordering};
static BACKEND: AtomicU8 = AtomicU8::new(0);

pub const BACKEND_ATA: u8 = 0;
pub const BACKEND_RAM: u8 = 1;
pub const BACKEND_NVME: u8 = 2;

/// Blok aygıtı arka ucunu seçer.
pub fn set_backend(b: u8) {
    BACKEND.store(b, Ordering::Relaxed);
}

/// Geçerli arka uç.
pub fn backend() -> u8 {
    BACKEND.load(Ordering::Relaxed)
}

fn rd(lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), FsError> {
    let ok = match backend() {
        BACKEND_RAM => crate::ramdisk::read_sector(lba, buf),
        BACKEND_NVME => crate::nvme::read_sector(lba, buf),
        _ => ata::read_sector(lba, buf),
    };
    if ok {
        Ok(())
    } else {
        Err(FsError::DiskError)
    }
}

fn wr(lba: u32, buf: &[u8; SECTOR_SIZE]) -> Result<(), FsError> {
    let ok = match backend() {
        BACKEND_RAM => crate::ramdisk::write_sector(lba, buf),
        BACKEND_NVME => crate::nvme::write_sector(lba, buf),
        _ => ata::write_sector(lba, buf),
    };
    if ok {
        Ok(())
    } else {
        Err(FsError::DiskError)
    }
}

fn get_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

// --- Superblock / bağlama ---

/// Diskte geçerli bir RFS olup olmadığını kontrol eder ve bağlar.
pub fn mount() -> Result<(), FsError> {
    let mut sb = [0u8; SECTOR_SIZE];
    rd(0, &mut sb)?;
    if get_u32(&sb, 0) != MAGIC {
        return Err(FsError::NotFormatted);
    }
    unsafe { TOTAL_BLOCKS = get_u32(&sb, 8) };
    Ok(())
}

/// Diski sıfırdan biçimlendirir (TÜM veri silinir).
pub fn format(total_blocks: u32) -> Result<(), FsError> {
    // Superblock
    let mut sb = [0u8; SECTOR_SIZE];
    put_u32(&mut sb, 0, MAGIC);
    put_u32(&mut sb, 4, VERSION);
    put_u32(&mut sb, 8, total_blocks);
    wr(0, &sb)?;

    // Bitmap: sistem bloklarını (0..DATA_START) dolu işaretle, gerisi boş.
    let mut bm = [0u8; SECTOR_SIZE];
    for blk in 0..DATA_START {
        bm[(blk / 8) as usize] |= 1 << (blk % 8);
    }
    wr(BITMAP_BLOCK, &bm)?;

    // Dizin bloklarını sıfırla.
    let zero = [0u8; SECTOR_SIZE];
    for i in 0..DIR_BLOCKS {
        wr(DIR_START + i, &zero)?;
    }

    unsafe { TOTAL_BLOCKS = total_blocks };
    Ok(())
}

// --- Bitmap (blok ayırma) ---

fn alloc_block() -> Result<u32, FsError> {
    let mut bm = [0u8; SECTOR_SIZE];
    rd(BITMAP_BLOCK, &mut bm)?;
    let total = unsafe { TOTAL_BLOCKS };
    for blk in DATA_START..total {
        let byte = (blk / 8) as usize;
        let bit = (blk % 8) as u8;
        if bm[byte] & (1 << bit) == 0 {
            bm[byte] |= 1 << bit;
            wr(BITMAP_BLOCK, &bm)?;
            return Ok(blk);
        }
    }
    Err(FsError::NoSpace)
}

fn free_block(blk: u32) -> Result<(), FsError> {
    let mut bm = [0u8; SECTOR_SIZE];
    rd(BITMAP_BLOCK, &mut bm)?;
    bm[(blk / 8) as usize] &= !(1 << (blk % 8));
    wr(BITMAP_BLOCK, &bm)
}

/// (kullanılan blok, toplam blok) döndürür.
pub fn usage() -> Result<(u32, u32), FsError> {
    let mut bm = [0u8; SECTOR_SIZE];
    rd(BITMAP_BLOCK, &mut bm)?;
    let total = unsafe { TOTAL_BLOCKS };
    let mut used = 0;
    for blk in 0..total {
        if bm[(blk / 8) as usize] & (1 << (blk % 8)) != 0 {
            used += 1;
        }
    }
    Ok((used, total))
}

// --- Dizin girdileri ---

fn entry_loc(index: usize) -> (u32, usize) {
    let block = DIR_START + (index / ENTRIES_PER_BLOCK) as u32;
    let offset = (index % ENTRIES_PER_BLOCK) * ENTRY_SIZE;
    (block, offset)
}

fn read_entry(index: usize) -> Result<Entry, FsError> {
    let (block, off) = entry_loc(index);
    let mut buf = [0u8; SECTOR_SIZE];
    rd(block, &mut buf)?;
    let e = &buf[off..off + ENTRY_SIZE];

    let mut name = [0u8; MAX_NAME];
    name.copy_from_slice(&e[0..MAX_NAME]);
    let used = e[28] != 0;
    let kind = e[29];
    // parent diskte +1 saklanır; 0 = kök.
    let parent = if e[30] == 0 { ROOT } else { e[30] - 1 };
    let size = get_u32(e, 32);
    let block_count = get_u32(e, 36);
    let mut blocks = [0u32; MAX_BLOCKS_PER_FILE];
    for (i, b) in blocks.iter_mut().enumerate() {
        *b = get_u32(e, 40 + i * 4);
    }
    Ok(Entry {
        name,
        used,
        kind,
        parent,
        size,
        block_count,
        blocks,
    })
}

fn write_entry(index: usize, entry: &Entry) -> Result<(), FsError> {
    let (block, off) = entry_loc(index);
    let mut buf = [0u8; SECTOR_SIZE];
    rd(block, &mut buf)?;
    let e = &mut buf[off..off + ENTRY_SIZE];

    e[0..MAX_NAME].copy_from_slice(&entry.name);
    e[28] = entry.used as u8;
    e[29] = entry.kind;
    e[30] = if entry.parent == ROOT { 0 } else { entry.parent + 1 };
    e[31] = 0;
    put_u32(e, 32, entry.size);
    put_u32(e, 36, entry.block_count);
    for (i, b) in entry.blocks.iter().enumerate() {
        put_u32(e, 40 + i * 4, *b);
    }
    wr(block, &buf)
}

fn name_eq(stored: &[u8; MAX_NAME], query: &[u8]) -> bool {
    // Saklanan ad null ile doldurulmuştur.
    let stored_len = stored.iter().position(|&c| c == 0).unwrap_or(MAX_NAME);
    stored_len == query.len() && &stored[..stored_len] == query
}

/// Belirli bir dizinin (parent) içinde verilen ada sahip girdiyi arar.
fn find(name: &[u8], parent: u8) -> Result<Option<usize>, FsError> {
    for i in 0..MAX_FILES {
        let e = read_entry(i)?;
        if e.used && e.parent == parent && name_eq(&e.name, name) {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

/// Bir dizinin (entry indeksi) içinde hiç girdi var mı?
fn dir_has_children(dir_index: u8) -> Result<bool, FsError> {
    for i in 0..MAX_FILES {
        let e = read_entry(i)?;
        if e.used && e.parent == dir_index {
            return Ok(true);
        }
    }
    Ok(false)
}

fn free_index() -> Result<Option<usize>, FsError> {
    for i in 0..MAX_FILES {
        let e = read_entry(i)?;
        if !e.used {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

// --- Genel dosya işlemleri ---

/// Dizindeki `index` numaralı girdinin bilgisini döndürür (yoksa None).
pub fn entry_info(index: usize) -> Result<Option<FileInfo>, FsError> {
    let e = read_entry(index)?;
    if e.used {
        Ok(Some(FileInfo {
            name: e.name,
            size: e.size,
            kind: e.kind,
            parent: e.parent,
        }))
    } else {
        Ok(None)
    }
}

/// Bir dizin oluşturur (verilen üst dizinin içinde).
pub fn mkdir(name: &[u8], parent: u8) -> Result<(), FsError> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(FsError::NameTooLong);
    }
    if find(name, parent)?.is_some() {
        return Err(FsError::Exists);
    }
    let index = free_index()?.ok_or(FsError::DirFull)?;
    let mut entry = Entry::EMPTY;
    let mut nm = [0u8; MAX_NAME];
    nm[..name.len()].copy_from_slice(name);
    entry.name = nm;
    entry.used = true;
    entry.kind = KIND_DIR;
    entry.parent = parent;
    entry.size = 0;
    entry.block_count = 0;
    write_entry(index, &entry)
}

/// Bir dosyayı oluşturur veya üzerine yazar (verilen dizin içinde).
pub fn write_file(name: &[u8], parent: u8, data: &[u8]) -> Result<(), FsError> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err(FsError::NameTooLong);
    }
    if data.len() > MAX_FILE_SIZE {
        return Err(FsError::TooBig);
    }

    // Var olan girdiyi bul ya da boş bir girdi al.
    let (index, mut entry) = match find(name, parent)? {
        Some(i) => {
            let e = read_entry(i)?;
            if e.kind == KIND_DIR {
                return Err(FsError::Exists);
            }
            (i, e)
        }
        None => {
            let i = free_index()?.ok_or(FsError::DirFull)?;
            (i, Entry::EMPTY)
        }
    };

    // Eski blokları serbest bırak.
    for b in 0..entry.block_count as usize {
        free_block(entry.blocks[b])?;
    }

    let needed = data.len().div_ceil(SECTOR_SIZE);

    // Yeni blokları ayır.
    let mut blocks = [0u32; MAX_BLOCKS_PER_FILE];
    for (i, slot) in blocks.iter_mut().enumerate().take(needed) {
        match alloc_block() {
            Ok(b) => *slot = b,
            Err(e) => {
                // Hata: o ana kadar ayrılanları geri ver.
                for blk in blocks.iter().take(i) {
                    let _ = free_block(*blk);
                }
                return Err(e);
            }
        }
    }

    // Veriyi bloklara yaz (son blok sıfırla doldurulur).
    for (i, blk) in blocks.iter().enumerate().take(needed) {
        let mut sec = [0u8; SECTOR_SIZE];
        let start = i * SECTOR_SIZE;
        let end = core::cmp::min(start + SECTOR_SIZE, data.len());
        sec[..end - start].copy_from_slice(&data[start..end]);
        wr(*blk, &sec)?;
    }

    // Girdiyi güncelle.
    let mut nm = [0u8; MAX_NAME];
    nm[..name.len()].copy_from_slice(name);
    entry.name = nm;
    entry.used = true;
    entry.kind = KIND_FILE;
    entry.parent = parent;
    entry.size = data.len() as u32;
    entry.block_count = needed as u32;
    entry.blocks = blocks;
    write_entry(index, &entry)
}

/// Bir Entry'nin veri bloklarını `buf` içine okur ve bayt sayısını döndürür.
fn read_blocks(e: &Entry, buf: &mut [u8]) -> Result<usize, FsError> {
    let size = e.size as usize;
    for i in 0..e.block_count as usize {
        let mut sec = [0u8; SECTOR_SIZE];
        rd(e.blocks[i], &mut sec)?;
        let start = i * SECTOR_SIZE;
        let end = core::cmp::min(start + SECTOR_SIZE, size);
        if start < size {
            buf[start..end].copy_from_slice(&sec[..end - start]);
        }
    }
    Ok(size)
}

/// Bir dosyanın içeriğini (verilen dizinde) `buf` içine okur.
pub fn read_file(name: &[u8], parent: u8, buf: &mut [u8]) -> Result<usize, FsError> {
    let index = find(name, parent)?.ok_or(FsError::NotFound)?;
    let e = read_entry(index)?;
    if e.kind == KIND_DIR {
        return Err(FsError::NotFound);
    }
    read_blocks(&e, buf)
}

/// `index` numaralı girdinin içeriğini doğrudan okur (GUI gibi indekse erişenler).
pub fn read_index(index: usize, buf: &mut [u8]) -> Result<usize, FsError> {
    let e = read_entry(index)?;
    if !e.used || e.kind == KIND_DIR {
        return Err(FsError::NotFound);
    }
    read_blocks(&e, buf)
}

/// Bir dosyanın sonuna veri ekler (verilen dizinde).
pub fn append_file(name: &[u8], parent: u8, extra: &[u8]) -> Result<(), FsError> {
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) };
    let old = match find(name, parent)? {
        Some(_) => read_file(name, parent, buf)?,
        None => 0,
    };
    if old + extra.len() > MAX_FILE_SIZE {
        return Err(FsError::TooBig);
    }
    buf[old..old + extra.len()].copy_from_slice(extra);
    write_file(name, parent, &buf[..old + extra.len()])
}

/// Bir dosyayı veya BOŞ bir dizini siler (verilen dizinde).
pub fn delete(name: &[u8], parent: u8) -> Result<(), FsError> {
    let index = find(name, parent)?.ok_or(FsError::NotFound)?;
    let e = read_entry(index)?;
    if e.kind == KIND_DIR && dir_has_children(index as u8)? {
        return Err(FsError::NotEmpty);
    }
    for b in 0..e.block_count as usize {
        free_block(e.blocks[b])?;
    }
    write_entry(index, &Entry::EMPTY)
}

/// Bir dizin girdisinin (entry indeksi) üst dizinini döndürür.
pub fn parent_of(index: u8) -> Result<u8, FsError> {
    Ok(read_entry(index as usize)?.parent)
}
