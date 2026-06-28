//! Yığın (heap) kurulumu.
//!
//! Ağ yığını (smoltcp), TLS ve dinamik tamponlar için `alloc` gerekiyor. Heap'in
//! yerleşeceği bölgeyi artık `mem` modülü belirler: sayfalama açıldıktan sonra
//! Multiboot bellek haritasından bulunan gerçek RAM'in büyük bir bölümü (statik
//! 4 MiB dizi yerine) global ayırıcıya verilir. Birebir (identity) eşleme
//! sayesinde bu bölge doğrudan fiziksel bellektir.

use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Yığını `mem::init`'in döndürdüğü bölgeyle başlatır. `kernel_main` başında,
/// `mem::init`'ten hemen sonra ve diğer her şeyden önce çağrılmalı.
pub fn init(start: usize, size: usize) {
    unsafe {
        ALLOCATOR.lock().init(start as *mut u8, size);
    }
}

/// Yığının toplam boyutu (bayt).
pub fn heap_size() -> usize {
    ALLOCATOR.lock().size()
}

/// Yığında o an kullanımda olan bayt sayısı.
pub fn heap_used() -> usize {
    ALLOCATOR.lock().used()
}
