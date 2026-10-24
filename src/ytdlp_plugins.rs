//! Plugins de yt-dlp que la aplicación escribe en su carpeta de datos y activa
//! con `--plugin-dirs` solo para los sitios que los necesitan.
//!
//! POR QUÉ UN PLUGIN Y NO ESPERAR. El 22 de octubre de 2026 Bilibili cambió su
//! web: `www.bilibili.com/video/<BV>/` responde ahora con una carcasa de 10 KB
//! sin `window.__INITIAL_STATE__`, y es un JavaScript de esa carcasa el que
//! salta a `/zh-Hans/video/<BV>/`, donde sí está la página con los datos
//! (medido con Chromium: la primera respuesta es la carcasa, la segunda la
//! página de 200 KB). yt-dlp 2026.10.21 y la nightly del 23 piden la ruta
//! antigua y fallan con «Unable to extract initial state» para cualquier vídeo.
//! No es la huella TLS: ni `--impersonate` ni curl_cffi imitando Chrome,
//! Safari o Firefox lo arreglan; pedir `/zh-Hans/video/` sí, hasta con curl.
//!
//! El plugin sustituye al extractor de vídeos de Bilibili por una subclase que
//! pide esa ruta y acepta la URL con idioma al volver. Todo lo demás —formatos
//! por la API firmada, DASH, subtítulos— es el código de yt-dlp sin tocar.
//! Probado con el zipapp `yt-dlp` (el que se instala en Linux y macOS) y con el
//! binario autónomo (el mismo tipo que `yt-dlp.exe`): 1080p AVC + AAC,
//! fusionado por ffmpeg. Cuando yt-dlp lo arregle, pedir `/zh-Hans/` seguirá
//! funcionando; y si algún día el extractor cambia de nombre, yt-dlp ignora un
//! plugin que no puede importar y sigue con el suyo.

use std::path::{Path, PathBuf};

/// Nombre del paquete de plugins, que yt-dlp muestra como `BiliBili+todo-downloader`.
const PAQUETE: &str = "todo-downloader";

/// El plugin de Bilibili.
pub const BILIBILI: &str = r#"# Todo Downloader: Bilibili sirve desde el 22/10/2026 una carcasa sin datos en
# /video/<BV>/; la pagina con window.__INITIAL_STATE__ esta en
# /zh-Hans/video/<BV>/. Se pide esa y se acepta la URL con idioma al volver.
import re

from yt_dlp.extractor.bilibili import BiliBiliIE


class TodoDownloaderBiliBiliIE(BiliBiliIE, plugin_name='todo-downloader'):
    _VALID_URL = (r'https?://(?:www\.)?bilibili\.com/(?:[a-z]{2}(?:-[A-Za-z]{2,4})?/)?'
                  r'(?:video/|festival/[^/?#]+\?(?:[^#]*&)?bvid=)(?P<prefix>[aAbB][vV])(?P<id>[^/?#&]+)')

    def _download_webpage_handle(self, url_or_request, video_id, *args, **kwargs):
        if isinstance(url_or_request, str):
            url_or_request = re.sub(
                r'^(https?://(?:www\.)?bilibili\.com/)video/', r'\1zh-Hans/video/', url_or_request)
        return super()._download_webpage_handle(url_or_request, video_id, *args, **kwargs)
"#;

/// Escribe los plugins bajo `raiz` (si faltan o han cambiado) y devuelve la
/// carpeta que se pasa a `--plugin-dirs`.
///
/// La estructura la impone yt-dlp: `--plugin-dirs` busca PAQUETES, cada uno
/// con su espacio de nombres `yt_dlp_plugins/extractor/` dentro. Con el `.py`
/// un nivel más arriba no se carga y no dice nada (medido).
pub fn preparar(raiz: &Path) -> std::io::Result<PathBuf> {
    let carpeta = raiz.join(PAQUETE).join("yt_dlp_plugins").join("extractor");
    let archivo = carpeta.join("todo_downloader_bilibili.py");
    if std::fs::read_to_string(&archivo).ok().as_deref() != Some(BILIBILI) {
        std::fs::create_dir_all(&carpeta)?;
        std::fs::write(&archivo, BILIBILI)?;
    }
    Ok(raiz.to_path_buf())
}

/// Argumentos para yt-dlp: los plugins por defecto del usuario (`default`) y
/// los de la aplicación. Sin `default`, pasar una carpeta propia dejaría de
/// cargar los que el usuario tuviera en su configuración.
pub fn args(carpeta: &Path) -> Vec<String> {
    vec![
        "--plugin-dirs".into(),
        "default".into(),
        "--plugin-dirs".into(),
        carpeta.to_string_lossy().into_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La estructura de carpetas es lo que decide si yt-dlp lo carga, y
    /// reescribir solo cuando cambia evita tocar el disco en cada descarga.
    #[test]
    fn escribe_el_plugin_donde_yt_dlp_lo_busca() {
        let raiz = std::env::temp_dir().join(format!("td-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let dir = preparar(&raiz).unwrap();
        let archivo = dir.join("todo-downloader/yt_dlp_plugins/extractor/todo_downloader_bilibili.py");
        assert_eq!(std::fs::read_to_string(&archivo).unwrap(), BILIBILI);

        // Un archivo viejo o tocado a mano se sustituye por el bueno.
        std::fs::write(&archivo, "# viejo").unwrap();
        preparar(&raiz).unwrap();
        assert_eq!(std::fs::read_to_string(&archivo).unwrap(), BILIBILI);

        let a = args(&dir);
        assert_eq!(a[..3], ["--plugin-dirs", "default", "--plugin-dirs"]);
        assert_eq!(a[3], dir.to_string_lossy());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// El plugin tal como lo escribe la aplicación, cargado por un yt-dlp de
    /// verdad contra Bilibili. Necesita red y un yt-dlp:
    /// `TD_YTDLP=/ruta/yt-dlp cargo test -- --ignored plugin_bilibili_en_vivo`
    #[test]
    #[ignore]
    fn plugin_bilibili_en_vivo() {
        let Ok(ytdlp) = std::env::var("TD_YTDLP") else {
            eprintln!("sin TD_YTDLP: nada que probar");
            return;
        };
        let raiz = std::env::temp_dir().join(format!("td-plugins-vivo-{}", std::process::id()));
        let dir = preparar(&raiz).unwrap();
        let out = std::process::Command::new(ytdlp)
            .args(args(&dir))
            .args(["--simulate", "--no-playlist", "--print", "%(extractor)s|%(height)s"])
            .args(["--", "https://www.bilibili.com/video/BV1RbHK6AEei"])
            .output()
            .unwrap();
        let salida = String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_dir_all(&raiz);
        assert!(salida.contains("BiliBili+todo-downloader|"), "{salida}\n{}", String::from_utf8_lossy(&out.stderr));
    }

    /// Lo que hace el plugin: subclase con `plugin_name` (así sustituye al
    /// extractor en vez de añadir otro), la ruta con idioma en la petición y
    /// en el patrón de URL.
    #[test]
    fn el_plugin_sustituye_al_extractor_y_pide_la_ruta_con_idioma() {
        assert!(BILIBILI.contains("class TodoDownloaderBiliBiliIE(BiliBiliIE, plugin_name='todo-downloader')"));
        assert!(BILIBILI.contains(r"r'\1zh-Hans/video/'"));
        assert!(BILIBILI.contains(r"(?:[a-z]{2}(?:-[A-Za-z]{2,4})?/)?"));
        assert!(BILIBILI.is_ascii(), "sin acentos: el .py se lee con la codificación que toque");
    }
}
