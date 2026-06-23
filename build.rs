use std::env;
use std::process::Command;

// boot.asm dosyasını NASM ile derler ve linker'a hem bu nesneyi
// hem de özel linker betiğimizi (linker.ld) iletir.
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let boot_obj = format!("{out_dir}/boot.o");

    let status = Command::new("nasm")
        .args(["-f", "elf32"])
        .arg(format!("{manifest}/src/boot.asm"))
        .args(["-o", &boot_obj])
        .status()
        .expect("nasm çalıştırılamadı — kurulu mu? (brew install nasm)");
    assert!(status.success(), "boot.asm derlenemedi");

    println!("cargo:rustc-link-arg={boot_obj}");
    println!("cargo:rustc-link-arg=-T{manifest}/linker.ld");
    println!("cargo:rerun-if-changed=src/boot.asm");
    println!("cargo:rerun-if-changed=linker.ld");
}
