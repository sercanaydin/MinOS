use std::env;
use std::process::Command;

// boot.asm dosyasını NASM ile derler ve linker'a hem bu nesneyi
// hem de özel linker betiğimizi (linker.ld) iletir.
fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();

    // NASM ile derlenecek assembly kaynakları.
    for src in ["boot.asm", "isr.asm"] {
        let obj = format!("{out_dir}/{}.o", src.trim_end_matches(".asm"));
        let status = Command::new("nasm")
            .args(["-f", "elf32"])
            .arg(format!("{manifest}/src/{src}"))
            .args(["-o", &obj])
            .status()
            .expect("nasm çalıştırılamadı — kurulu mu? (brew install nasm)");
        assert!(status.success(), "{src} derlenemedi");
        println!("cargo:rustc-link-arg={obj}");
        println!("cargo:rerun-if-changed=src/{src}");
    }

    println!("cargo:rustc-link-arg=-T{manifest}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");

    // --- Kullanıcı (ring 3) programını ayrı bir ELF olarak derle ---
    // nasm ile elf32 nesne, sonra rust-lld (gnu kipi) ile elf_i386 yürütülebilir.
    // Üretilen ELF, çekirdeğe include_bytes! ile gömülür (src/user.rs).
    let user_obj = format!("{out_dir}/userprog.o");
    let user_elf = format!("{out_dir}/userprog.elf");

    let status = Command::new("nasm")
        .args(["-f", "elf32"])
        .arg(format!("{manifest}/src/userprog.asm"))
        .args(["-o", &user_obj])
        .status()
        .expect("nasm çalıştırılamadı");
    assert!(status.success(), "userprog.asm derlenemedi");

    // rust-lld yolunu rustc'nin sysroot'undan bul.
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let host = env::var("HOST").unwrap();
    let sysroot = String::from_utf8(
        Command::new(&rustc)
            .args(["--print", "sysroot"])
            .output()
            .expect("rustc --print sysroot çalıştırılamadı")
            .stdout,
    )
    .unwrap();
    let sysroot = sysroot.trim();
    let lld = format!("{sysroot}/lib/rustlib/{host}/bin/rust-lld");

    let status = Command::new(&lld)
        .args(["-flavor", "gnu", "-m", "elf_i386"])
        .arg("-o")
        .arg(&user_elf)
        .arg("-T")
        .arg(format!("{manifest}/src/user.ld"))
        .arg(&user_obj)
        .status()
        .expect("rust-lld çalıştırılamadı");
    assert!(status.success(), "userprog linklenemedi");

    println!("cargo:rerun-if-changed=src/userprog.asm");
    println!("cargo:rerun-if-changed=src/user.ld");

    // --- C kullanıcı programlarını clang + rust-lld ile derle ---
    // i686-elf-gcc gerektirmez; clang zaten bir çapraz derleyicidir. Paylaşılan
    // mini-libc (crt0+syscalls+libminos) bir kez derlenir; her uygulama ayrı bir
    // ELF olarak linklenir (-ffunction-sections + --gc-sections ile kullanılmayan
    // kod atılır, -s ile sembol/strip → diske sığacak kadar küçük). Her ELF
    // çekirdeğe include_bytes! ile gömülür (user.rs). clang yoksa boş yer tutucu.
    let cflags: &[&str] = &[
        "--target=i386-unknown-none-elf",
        "-m32",
        "-ffreestanding",
        "-fno-pic",
        "-fno-stack-protector",
        "-fno-builtin",
        "-ffunction-sections",
        "-fdata-sections",
        "-Os",
    ];
    let user_dir = format!("{manifest}/user");

    let compile = |src: &str| -> Option<String> {
        let obj = format!("{out_dir}/{}.o", src.trim_end_matches(".c"));
        let ok = Command::new("clang")
            .args(cflags)
            .args(["-I", &user_dir, "-c"])
            .arg(format!("{user_dir}/{src}"))
            .args(["-o", &obj])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok { Some(obj) } else { None }
    };

    // Paylaşılan mini-libc nesneleri (bir kez).
    let mut shared: Vec<String> = Vec::new();
    let mut toolchain_ok = true;
    for src in ["crt0.c", "syscalls.c", "libminos.c"] {
        match compile(src) {
            Some(o) => shared.push(o),
            None => {
                toolchain_ok = false;
                break;
            }
        }
    }

    // (kaynak dosyası, çıktı ELF adı) — her biri ayrı bir uygulama.
    let programs = [
        ("hello.c", "hello_c.elf"),
        ("calc.c", "calc.elf"),
        ("paint.c", "paint.elf"),
        ("c4.c", "c4.elf"),
    ];

    for (src, elf_name) in programs {
        let elf = format!("{out_dir}/{elf_name}");
        let mut ok = toolchain_ok;
        if ok {
            if let Some(prog_obj) = compile(src) {
                let mut cmd = Command::new(&lld);
                cmd.args(["-flavor", "gnu", "-m", "elf_i386", "--gc-sections", "-s"])
                    .arg("-o")
                    .arg(&elf)
                    .arg("-T")
                    .arg(format!("{manifest}/src/user.ld"));
                for o in &shared {
                    cmd.arg(o);
                }
                cmd.arg(&prog_obj);
                ok = cmd.status().map(|s| s.success()).unwrap_or(false);
            } else {
                ok = false;
            }
        }
        if !ok {
            std::fs::write(&elf, b"").unwrap();
            println!("cargo:warning=C programi '{src}' derlenemedi (clang kurulu mu?)");
        }
        println!("cargo:rerun-if-changed=user/{src}");
    }

    println!("cargo:rerun-if-changed=user/c4.c");
    println!("cargo:rerun-if-changed=user/crt0.c");
    println!("cargo:rerun-if-changed=user/syscalls.c");
    println!("cargo:rerun-if-changed=user/libminos.c");
    println!("cargo:rerun-if-changed=user/minos.h");
    println!("cargo:rerun-if-changed=user/stdio.h");
    println!("cargo:rerun-if-changed=user/stdlib.h");
    println!("cargo:rerun-if-changed=user/string.h");
    println!("cargo:rerun-if-changed=user/ctype.h");
}
