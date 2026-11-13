fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winres::WindowsResource::new();
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
            && std::env::var("HOST").ok() != std::env::var("TARGET").ok()
        {
            resource.set_windres_path("x86_64-w64-mingw32-windres");
            resource.set_ar_path("x86_64-w64-mingw32-ar");
        }
        resource.set_icon("assets/app.ico");
        resource.set("ProductName", "Speaker Scene Director");
        resource.set("FileDescription", "Speaker Scene Director");
        resource.compile().expect("Unable to embed Windows icon");
        // GNU linkers may discard resource-only static archives; link the object explicitly.
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
            let out = std::env::var("OUT_DIR").expect("Missing OUT_DIR");
            println!("cargo:rustc-link-arg={out}/resource.o");
        }
    }
}
