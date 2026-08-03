//! Script de compilación: incrusta el icono y los metadatos en el ejecutable
//! de Windows. Sin esto, el .exe sale con el icono genérico del sistema y sin
//! información de versión en las propiedades del archivo.
//!
//! Solo hace algo en Windows; en Linux y macOS el icono va por otra vía (el
//! propio programa lo carga en la ventana en tiempo de ejecución).

fn main() {
    #[cfg(windows)]
    {
        // Recompilar si cambia el icono
        println!("cargo:rerun-if-changed=assets/icon.ico");

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Todo Downloader");
        res.set("FileDescription", "Todo Downloader - gestor de descargas");
        res.set("CompanyName", "Eric V. Gramunt");
        res.set("LegalCopyright", "MIT License (c) 2026 Eric V. Gramunt");
        // No abortar la compilación si el recurso falla (p. ej. cross-compiling)
        if let Err(e) = res.compile() {
            eprintln!("aviso: no se pudo incrustar el icono: {e}");
        }
    }
}
