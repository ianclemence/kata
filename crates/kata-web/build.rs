// Give the Windows executable Kata's icon and version details.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/kata.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/kata.ico");
        res.set("ProductName", "Kata");
        res.set("FileDescription", "Kata: cut, print, compose and review");
        res.set("LegalCopyright", "Ian Clemence. MIT OR Apache-2.0");
        if let Err(error) = res.compile() {
            // A missing resource compiler must not block a build; the binary just has no icon.
            println!("cargo:warning=no Windows icon embedded: {error}");
        }
    }
}
