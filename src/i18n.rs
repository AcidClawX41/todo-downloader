//! Sistema de idiomas de Todo Downloader — By Eric V. Gramunt
//!
//! Para añadir un idioma nuevo:
//!   1. Añade una variante al enum `Lang` y su entrada en `Lang::ALL` / `label()`.
//!   2. Añade la columna correspondiente en cada `entry!` de la tabla `t()`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Es,
    En,
}

impl Default for Lang {
    fn default() -> Self {
        Lang::Es
    }
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Es, Lang::En];

    /// Nombre mostrado en el selector
    pub fn label(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
        }
    }

    /// Detecta el idioma del sistema; por defecto español
    pub fn detect() -> Self {
        // Unix / entornos con LANG definido
        let raw = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LANGUAGE"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if raw.starts_with("en") {
            return Lang::En;
        }
        if raw.starts_with("es") {
            return Lang::Es;
        }

        // Windows: preguntar al sistema por el idioma de la UI
        #[cfg(windows)]
        {
            if let Ok(out) = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", "(Get-Culture).TwoLetterISOLanguageName"])
                .output()
            {
                let code = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
                if code == "es" {
                    return Lang::Es;
                }
                if !code.is_empty() {
                    // Cualquier otro idioma → inglés, que es más universal
                    return Lang::En;
                }
            }
        }

        Lang::default()
    }
}

/// Traduce una clave. Si la clave no existe, devuelve la propia clave (fallo visible pero no fatal).
pub fn t(lang: Lang, key: &'static str) -> &'static str {
    macro_rules! entry {
        ($es:expr, $en:expr) => {
            match lang {
                Lang::Es => $es,
                Lang::En => $en,
            }
        };
    }

    match key {
        // ---------- Navegación ----------
        "nav.downloads" => entry!("Descargas", "Downloads"),
        "nav.profile" => entry!("Perfil", "Profile"),
        "nav.completed" => entry!("Completadas", "Completed"),
        "nav.errors" => entry!("Errores", "Errors"),
        "nav.capture" => entry!("Capturar", "Capture"),
        "nav.settings" => entry!("Ajustes", "Settings"),

        // ---------- Vista Capturar ----------
        "cap.title" => entry!("Capturar desde el navegador", "Capture from the browser"),
        "cap.subtitle" => entry!(
            "Para lo que yt-dlp no puede (perfiles de Douyin, contenido con sesión): el script corre en la pestaña del perfil y envía los enlaces directamente a la cola.",
            "For what yt-dlp cannot do (Douyin profiles, session-gated content): the script runs in the profile tab and sends the links straight to the queue."
        ),
        "cap.listening" => entry!("● Receptor escuchando en", "● Receiver listening on"),
        "cap.off" => entry!("● Receptor desactivado", "● Receiver disabled"),
        "cap.enable" => entry!("Activar receptor", "Enable receiver"),
        "cap.note_restart" => entry!(
            "Solo escucha en 127.0.0.1 (nunca desde la red). Cambiar el puerto requiere reiniciar la app.",
            "Listens on 127.0.0.1 only (never from the network). Changing the port requires restarting the app."
        ),
        "cap.site" => entry!("SITIO", "SITE"),
        "cap.step1" => entry!(
            "1. Abre el perfil en el navegador y pulsa F12 → pestaña «Consola».",
            "1. Open the profile in your browser and press F12 → «Console» tab."
        ),
        "cap.step2" => entry!(
            "2. Copia el script con el botón de abajo y pégalo en la consola. Pulsa Enter.",
            "2. Copy the script with the button below, paste it into the console and press Enter."
        ),
        "cap.step3" => entry!(
            "3. Espera a que termine: los enlaces aparecerán solos en «Descargas».",
            "3. Wait for it to finish: the links will appear by themselves under «Downloads»."
        ),
        "cap.copy" => entry!("📋  Copiar script", "📋  Copy script"),
        "cap.save" => entry!("💾 Guardar como .js", "💾 Save as .js"),
        "cap.copied" => entry!(
            "Script copiado — pégalo en la consola del navegador (F12)",
            "Script copied — paste it into the browser console (F12)"
        ),
        "cap.preview" => entry!("VISTA PREVIA DEL SCRIPT", "SCRIPT PREVIEW"),
        "set.receiver" => entry!("RECEPTOR LOCAL (CAPTURA DESDE EL NAVEGADOR)", "LOCAL RECEIVER (BROWSER CAPTURE)"),
        "set.receiver_port" => entry!("Puerto (requiere reiniciar):", "Port (requires restart):"),

        // ---------- Estados ----------
        "status.queued" => entry!("En cola", "Queued"),
        "status.waiting" => entry!("Esperando", "Waiting"),
        "status.downloading" => entry!("Descargando", "Downloading"),
        "status.resolving" => entry!("yt-dlp", "yt-dlp"),
        "status.paused" => entry!("Pausado", "Paused"),
        "status.done" => entry!("Completado", "Completed"),
        "status.error" => entry!("Error", "Error"),
        "label.gallery" => entry!("(galería completa)", "(full gallery)"),
        "tip.error_hint" => entry!(
            "Pasa el ratón para ver el mensaje completo",
            "Hover to see the full message"
        ),

        // ---------- Tarjetas de estadísticas ----------
        "stat.total" => entry!("EN TOTAL", "TOTAL"),
        "stat.active" => entry!("ACTIVAS", "ACTIVE"),
        "stat.completed" => entry!("COMPLETADAS", "COMPLETED"),
        "stat.errors" => entry!("ERRORES", "ERRORS"),
        "stat.speed" => entry!("VELOCIDAD", "SPEED"),

        // ---------- Acciones ----------
        "btn.start_all" => entry!("▶  Iniciar todo", "▶  Start all"),
        "btn.pause_all" => entry!("⏸  Pausar todo", "⏸  Pause all"),
        "btn.add_links" => entry!("➕  Añadir enlaces", "➕  Add links"),
        "btn.import" => entry!("📂  Importar TXT/JSON", "📂  Import TXT/JSON"),
        "btn.clean_completed" => entry!("🧹  Limpiar completados", "🧹  Clear completed"),
        "btn.clear_all" => entry!("🗑  Vaciar cola", "🗑  Empty queue"),
        "btn.open_dest" => entry!("📁  Abrir carpeta destino", "📁  Open destination folder"),
        "btn.retry_all" => entry!("🔁  Reintentar todos", "🔁  Retry all"),
        "btn.search_hint" => entry!("🔍  Buscar…", "🔍  Search…"),
        "btn.paste" => entry!("📋 Pegar", "📋 Paste"),
        "btn.analyze" => entry!("🔍  Analizar perfil", "🔍  Analyze profile"),
        "btn.all" => entry!("Todos", "All"),
        "btn.none" => entry!("Ninguno", "None"),
        "btn.browse" => entry!("Examinar…", "Browse…"),
        "btn.open" => entry!("📁 Abrir", "📁 Open"),
        "btn.update_latest" => entry!("🔄 Actualizar a la última versión", "🔄 Update to latest version"),
        "btn.cancel" => entry!("Cancelar", "Cancel"),

        // ---------- Tabla ----------
        "col.file" => entry!("ARCHIVO", "FILE"),
        "col.size" => entry!("TAMAÑO", "SIZE"),
        "col.progress" => entry!("PROGRESO", "PROGRESS"),
        "col.speed" => entry!("VELOCIDAD", "SPEED"),
        "col.status" => entry!("ESTADO", "STATUS"),

        // ---------- Tooltips de fila ----------
        "tip.pause" => entry!("Pausar", "Pause"),
        "tip.start" => entry!("Iniciar", "Start"),
        "tip.open_folder" => entry!("Abrir carpeta", "Open folder"),
        "tip.retry" => entry!("Reintentar", "Retry"),
        "tip.remove" => entry!("Quitar", "Remove"),

        // ---------- Estados vacíos ----------
        "empty.done" => entry!("Aún no hay descargas completadas", "No completed downloads yet"),
        "empty.failed" => entry!("Ningún error — todo en orden", "No errors — all good"),
        "empty.queue" => entry!(
            "Arrastra un TXT/JSON exportado, analiza un perfil en la pestaña «Perfil»,\no copia URLs (TikTok, YouTube, Instagram…) — el LinkGrabber las capturará solo",
            "Drop an exported TXT/JSON, analyze a profile in the «Profile» tab,\nor copy URLs (TikTok, YouTube, Instagram…) — the LinkGrabber will catch them"
        ),

        // ---------- Ventana Añadir enlaces ----------
        "add.title" => entry!("Añadir enlaces", "Add links"),
        "add.hint" => entry!(
            "Uno por línea — directos CDN o tiktok.com/@user/video/…",
            "One per line — direct CDN links or tiktok.com/@user/video/…"
        ),
        "add.confirm" => entry!("Añadir a la cola", "Add to queue"),

        // ---------- Vista Perfil ----------
        "profile.title" => entry!("Descargar un perfil completo", "Download a full profile"),
        "profile.subtitle" => entry!(
            "Perfiles de TikTok: analiza y elige qué descargar. Instagram/Pinterest: descarga completa con gallery-dl. Douyin: no soportado (usa el script de consola).",
            "TikTok profiles: analyze and pick what to download. Instagram/Pinterest: full download via gallery-dl. Douyin: unsupported (use the console script)."
        ),
        "profile.url_label" => entry!("URL DEL PERFIL", "PROFILE URL"),
        "profile.want" => entry!("Quiero descargar:", "I want to download:"),
        "profile.videos" => entry!("🎬 Vídeos", "🎬 Videos"),
        "profile.images" => entry!("🖼 Imágenes", "🖼 Images"),
        "profile.cookies_inline" => entry!(
            "Usar cookies del navegador (necesario para Douyin y perfiles privados)",
            "Use browser cookies (required for Douyin and private profiles)"
        ),
        "profile.analyzing" => entry!(
            "Analizando perfil… puede tardar un poco en perfiles grandes",
            "Analyzing profile… this may take a while on large profiles"
        ),
        "profile.need_url" => entry!("Pega primero la URL del perfil", "Paste the profile URL first"),
        "profile.gallery_queued" => entry!(
            "📸 Descargando el perfil entero con gallery-dl (imágenes y vídeos). Puede tardar: no hay lista previa.",
            "📸 Downloading the whole profile with gallery-dl (images and videos). It may take a while: there is no preview list."
        ),
        "profile.douyin_unsupported" => entry!(
            "❌ Douyin no permite listar perfiles: ni yt-dlp ni gallery-dl tienen extractor de perfiles. Usa el script de consola «douyin_batch_downloader.js» y luego Importar TXT/JSON.",
            "❌ Douyin profiles cannot be listed: neither yt-dlp nor gallery-dl has a profile extractor. Use the «douyin_batch_downloader.js» console script, then Import TXT/JSON."
        ),
        "profile.douyin_note" => entry!(
            "❌ Douyin: los PERFILES no se pueden analizar (no existe extractor). Vídeos sueltos sí funcionan, pero exigen cookies.txt. Para el perfil entero usa el script de consola y luego «Importar TXT/JSON».",
            "❌ Douyin: PROFILES cannot be analyzed (no extractor exists). Single videos do work, but require cookies.txt. For a whole profile use the console script, then «Import TXT/JSON»."
        ),
        "profile.need_galdl" => entry!(
            "Instala primero gallery-dl en Ajustes (un clic)",
            "Install gallery-dl first in Settings (one click)"
        ),
        "profile.gallery_note" => entry!(
            "ℹ Instagram / Pinterest y similares se descargan enteros con gallery-dl (no hay lista previa). Instagram EXIGE sesión: usa un archivo cookies.txt en Ajustes.",
            "ℹ Instagram / Pinterest and similar are downloaded whole with gallery-dl (no preview list). Instagram REQUIRES a session: use a cookies.txt file in Settings."
        ),
        "profile.need_ytdlp" => entry!(
            "Instala primero yt-dlp en Ajustes (un clic)",
            "Install yt-dlp first in Settings (one click)"
        ),

        // ---------- Ajustes ----------
        "set.title" => entry!("Ajustes", "Settings"),
        "set.folder" => entry!("CARPETA DE DESCARGA", "DOWNLOAD FOLDER"),
        "set.per_author" => entry!("Crear subcarpeta por autor", "Create a subfolder per author"),
        "set.downloads" => entry!("DESCARGAS", "DOWNLOADS"),
        "set.concurrency" => entry!("Descargas simultáneas:", "Simultaneous downloads:"),
        "set.language" => entry!("IDIOMA / LANGUAGE", "LANGUAGE / IDIOMA"),
        "set.language_label" => entry!("Idioma de la interfaz:", "Interface language:"),
        "set.linkgrabber" => entry!("LINKGRABBER (PORTAPAPELES)", "LINKGRABBER (CLIPBOARD)"),
        "set.clip_watch" => entry!(
            "Capturar automáticamente URLs copiadas (TikTok, YouTube, Instagram, X…)",
            "Automatically capture copied URLs (TikTok, YouTube, Instagram, X…)"
        ),
        "set.clip_autostart" => entry!(
            "Iniciar la descarga automáticamente al capturar",
            "Start downloading automatically when captured"
        ),
        "set.clip_any" => entry!(
            "Capturar CUALQUIER URL, no solo sitios de vídeo conocidos",
            "Capture ANY URL, not just known video sites"
        ),
        "set.cookies" => entry!("COOKIES DEL NAVEGADOR", "BROWSER COOKIES"),
        "set.cookies_use" => entry!(
            "Usar la sesión de mi navegador en yt-dlp y gallery-dl",
            "Use my browser session in yt-dlp and gallery-dl"
        ),
        "set.cookies_browser" => entry!("Navegador:", "Browser:"),
        "set.cookies_note" => entry!(
            "Usa la sesión que ya tienes iniciada en ese navegador (TikTok, Douyin, YouTube…). Necesario para Douyin, perfiles privados o contenido con restricción de edad.",
            "Uses the session already signed in on that browser (TikTok, Douyin, YouTube…). Required for Douyin, private profiles or age-restricted content."
        ),
        "set.cookies_warn" => entry!(
            "⚠ Chrome 127+ (y Edge/Brave/Opera) cifran sus cookies con App-Bound Encryption: NINGUNA herramienta externa puede leerlas, ni con el navegador cerrado. Usa Firefox, o mejor el archivo cookies.txt de abajo.",
            "⚠ Chrome 127+ (and Edge/Brave/Opera) encrypt cookies with App-Bound Encryption: NO external tool can read them, even with the browser closed. Use Firefox, or better, the cookies.txt file below."
        ),

        // ---------- Motores ----------
        "eng.ytdlp" => entry!("MOTOR YT-DLP (INTEGRADO)", "YT-DLP ENGINE (BUILT-IN)"),
        "eng.galdl" => entry!("MOTOR GALLERY-DL (IMÁGENES, INTEGRADO)", "GALLERY-DL ENGINE (IMAGES, BUILT-IN)"),
        "eng.ytdlp_downloading" => entry!(
            "Descargando yt-dlp desde GitHub Releases…",
            "Downloading yt-dlp from GitHub Releases…"
        ),
        "eng.galdl_downloading" => entry!(
            "Descargando gallery-dl desde GitHub Releases…",
            "Downloading gallery-dl from GitHub Releases…"
        ),
        "eng.ytdlp_ok" => entry!("yt-dlp: instalado ✓", "yt-dlp: installed ✓"),
        "eng.galdl_ok" => entry!("gallery-dl: instalado ✓", "gallery-dl: installed ✓"),
        "eng.ytdlp_missing" => entry!(
            "yt-dlp: no instalado — necesario para enlaces de página y CDN caducados",
            "yt-dlp: not installed — required for page links and expired CDN links"
        ),
        "eng.galdl_missing" => entry!(
            "gallery-dl: no instalado — necesario para posts de imágenes (TikTok /photo/, Douyin /note/)",
            "gallery-dl: not installed — required for image posts (TikTok /photo/, Douyin /note/)"
        ),
        "eng.install_ytdlp" => entry!(
            "⬇  Instalar yt-dlp automáticamente",
            "⬇  Install yt-dlp automatically"
        ),
        "eng.install_galdl" => entry!(
            "⬇  Instalar gallery-dl automáticamente",
            "⬇  Install gallery-dl automatically"
        ),
        "eng.ytdlp_note" => entry!(
            "Se descarga el binario oficial de github.com/yt-dlp y se guarda junto a la app. Sin Python ni pip.",
            "Downloads the official binary from github.com/yt-dlp and stores it next to the app. No Python, no pip."
        ),
        "eng.galdl_note" => entry!(
            "Se descarga el binario oficial de github.com/mikf/gallery-dl. Sin Python ni pip.",
            "Downloads the official binary from github.com/mikf/gallery-dl. No Python, no pip."
        ),

        // ---------- ffmpeg ----------
        "eng.ffmpeg" => entry!("FFMPEG (INTEGRADO)", "FFMPEG (BUILT-IN)"),
        "eng.ffmpeg_ok" => entry!("ffmpeg: instalado ✓", "ffmpeg: installed ✓"),
        "eng.ffmpeg_quality_on" => entry!(
            "▲ Máxima calidad activada: se pueden fusionar vídeo y audio separados (1080p+ en YouTube y similares).",
            "▲ Maximum quality enabled: separate video and audio streams can be merged (1080p+ on YouTube and similar)."
        ),
        "eng.ffmpeg_missing" => entry!(
            "ffmpeg: no instalado — sin él la calidad se limita a los archivos ya fusionados",
            "ffmpeg: not installed — without it quality is limited to pre-merged files"
        ),
        "eng.install_ffmpeg" => entry!(
            "⬇  Instalar ffmpeg automáticamente",
            "⬇  Install ffmpeg automatically"
        ),
        "eng.ffmpeg_downloading" => entry!(
            "Descargando y extrayendo ffmpeg (~160 MB, tarda un poco)…",
            "Downloading and extracting ffmpeg (~160 MB, takes a while)…"
        ),
        "eng.ffmpeg_note" => entry!(
            "Build oficial del equipo de yt-dlp (github.com/yt-dlp/FFmpeg-Builds). Solo se extraen ffmpeg y ffprobe.",
            "Official build by the yt-dlp team (github.com/yt-dlp/FFmpeg-Builds). Only ffmpeg and ffprobe are extracted."
        ),
        "side.ffmpeg_active" => entry!("● ffmpeg activo", "● ffmpeg active"),
        "side.ffmpeg_missing" => entry!("● ffmpeg ausente", "● ffmpeg missing"),
        "side.ffmpeg_tip" => entry!(
            "Sin ffmpeg no se puede fusionar vídeo+audio: la calidad máxima queda limitada. Instálalo en Ajustes.",
            "Without ffmpeg, video+audio cannot be merged: maximum quality is limited. Install it in Settings."
        ),

        // ---------- Sidebar / estado ----------
        "side.ytdlp_active" => entry!("● yt-dlp activo", "● yt-dlp active"),
        "side.ytdlp_missing" => entry!("● yt-dlp ausente", "● yt-dlp missing"),
        "side.galdl_active" => entry!("● gallery-dl activo", "● gallery-dl active"),
        "side.galdl_missing" => entry!("● gallery-dl ausente", "● gallery-dl missing"),
        "side.grabber_active" => entry!("● LinkGrabber activo", "● LinkGrabber active"),
        "side.ytdlp_tip" => entry!(
            "Instálalo con un clic en Ajustes — necesario para enlaces de página y CDN caducados",
            "Install it with one click in Settings — required for page links and expired CDN links"
        ),
        "side.galdl_tip" => entry!(
            "Motor de imágenes (TikTok /photo/, Douyin /note/) — instálalo en Ajustes",
            "Image engine (TikTok /photo/, Douyin /note/) — install it in Settings"
        ),

        // ---------- Avisos ----------
        "toast.cookie_fallback" => entry!(
            "⚠ No se pudo leer las cookies del navegador. Reintentando sin ellas…",
            "⚠ Could not read the browser cookies. Retrying without them…"
        ),
        "toast.cookies_disabled" => entry!(
            "🍪 Cookies del navegador desactivadas automáticamente (no son legibles). Usa un archivo cookies.txt si necesitas sesión.",
            "🍪 Browser cookies disabled automatically (unreadable). Use a cookies.txt file if you need a session."
        ),
        "set.cookies_file" => entry!("Archivo cookies.txt (alternativa):", "cookies.txt file (alternative):"),
        "set.cookies_file_note" => entry!(
            "Exporta tus cookies con una extensión tipo «Get cookies.txt» y selecciona el archivo aquí. No requiere cerrar el navegador y tiene prioridad sobre la opción anterior.",
            "Export your cookies with an extension like «Get cookies.txt» and pick the file here. It doesn't require closing the browser and takes priority over the option above."
        ),
        "btn.pick_file" => entry!("Seleccionar…", "Pick file…"),
        "btn.clear" => entry!("Quitar", "Clear"),

        // ---------- Acerca de ----------
        "about" => entry!("ACERCA DE", "ABOUT"),
        "about.tech" => entry!(
            "Rust + egui · tokio · reqwest. Sin bloatware.",
            "Rust + egui · tokio · reqwest. No bloatware."
        ),

        _ => key,
    }
}

// ---------- Cadenas con formato ----------

pub fn imported_json(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("Importados {n} vídeos desde JSON"),
        Lang::En => format!("Imported {n} videos from JSON"),
    }
}

pub fn imported_txt(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("Importados {n} enlaces desde TXT"),
        Lang::En => format!("Imported {n} links from TXT"),
    }
}

pub fn cleared(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("🗑 {n} elementos quitados de la cola"),
        Lang::En => format!("🗑 {n} items removed from the queue"),
    }
}

pub fn received(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("🧲 {n} enlaces recibidos del navegador"),
        Lang::En => format!("🧲 {n} links received from the browser"),
    }
}

pub fn saved_to(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Es => format!("Guardado en {path}"),
        Lang::En => format!("Saved to {path}"),
    }
}

pub fn added_links(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("Añadidos {n} enlaces"),
        Lang::En => format!("Added {n} links"),
    }
}

pub fn starting(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("Iniciando {n} descargas"),
        Lang::En => format!("Starting {n} downloads"),
    }
}

pub fn clip_captured(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("📋 {n} enlaces capturados del portapapeles"),
        Lang::En => format!("📋 {n} links captured from the clipboard"),
    }
}

pub fn profile_analyzed(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("Perfil analizado: {n} publicaciones encontradas"),
        Lang::En => format!("Profile analyzed: {n} posts found"),
    }
}

pub fn profile_error(lang: Lang, e: &str) -> String {
    match lang {
        Lang::Es => format!("Error analizando perfil: {e}"),
        Lang::En => format!("Error analyzing profile: {e}"),
    }
}

pub fn install_error(lang: Lang, tool: &str, e: &str) -> String {
    match lang {
        Lang::Es => format!("Error instalando {tool}: {e}"),
        Lang::En => format!("Error installing {tool}: {e}"),
    }
}

pub fn added_to_queue(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("{n} publicaciones añadidas a la cola"),
        Lang::En => format!("{n} posts added to the queue"),
    }
}

pub fn add_selected(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("➕  Añadir {n} a la cola"),
        Lang::En => format!("➕  Add {n} to queue"),
    }
}

pub fn posts_summary(lang: Lang, total: usize, vids: usize, imgs: usize) -> String {
    match lang {
        Lang::Es => format!("{total} publicaciones · 🎬 {vids} vídeos · 🖼 {imgs} de imágenes"),
        Lang::En => format!("{total} posts · 🎬 {vids} videos · 🖼 {imgs} image posts"),
    }
}

pub fn read_error(lang: Lang, path: &str) -> String {
    match lang {
        Lang::Es => format!("No se pudo leer {path}"),
        Lang::En => format!("Could not read {path}"),
    }
}

pub fn invalid_json(lang: Lang, e: &str) -> String {
    match lang {
        Lang::Es => format!("JSON inválido: {e}"),
        Lang::En => format!("Invalid JSON: {e}"),
    }
}
