//! Çok basit bir yığın (heap) kurulumu.
//!
//! Ağ yığını (smoltcp) ve dinamik tamponlar için `alloc` gerekiyor. Statik bir
//! bellek bölgesini global ayırıcıya veriyoruz. Sayfalama kapalı olduğundan bu
//! bölge doğrudan fiziksel bellektir ve çekirdek imajının BSS'inde yer alır.

use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

// 4 MiB yığın. (QEMU'da 256 MiB RAM var; bol bol yeter.)
const HEAP_SIZE: usize = 4 * 1024 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

/// Yığını başlatır. `kernel_main` başında, her şeyden önce çağrılmalı.
pub fn init() {
    unsafe {
        let start = core::ptr::addr_of_mut!(HEAP) as *mut u8;
        ALLOCATOR.lock().init(start, HEAP_SIZE);
    }
}
