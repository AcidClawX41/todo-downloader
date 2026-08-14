//! Sistema de idiomas de Todo Downloader — By Eric V. Gramunt
//!
//! Para añadir un idioma nuevo:
//!   1. Añade una variante al enum `Lang` y su entrada en `Lang::ALL` / `label()`.
//!   2. Añade la columna correspondiente en cada `entry!` de la tabla `t()`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// Idioma actual de la interfaz, accesible desde cualquier parte.
///
/// POR QUÉ UN GLOBAL: los mensajes de error se generan dentro de tareas
/// asíncronas —el motor de MEGA, el de V2PH, los resolutores de hosters— que
/// no reciben ni deben recibir un puntero a los ajustes. La alternativa era
/// arrastrar un parámetro `Lang` por una docena de firmas que no tienen nada
/// que ver con la interfaz, o dejar los mensajes en un solo idioma. Esto
/// último es lo que había, y es un fallo: un usuario con la aplicación en
/// inglés recibía instrucciones en castellano.
///
/// La aplicación tiene un único idioma a la vez, así que un valor global es
/// una representación fiel de la realidad, no un atajo.
static IDIOMA: AtomicU8 = AtomicU8::new(0);

/// Fija el idioma que usarán los mensajes generados fuera de la interfaz.
pub fn set_lang(l: Lang) {
    IDIOMA.store(match l { Lang::Es => 0, Lang::En => 1 }, Ordering::Relaxed);
}

/// Idioma actual, para código que no tiene acceso a los ajustes.
pub fn lang() -> Lang {
    if IDIOMA.load(Ordering::Relaxed) == 1 { Lang::En } else { Lang::Es }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum Lang {
    #[default]
    Es,
    En,
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
            // CREATE_NO_WINDOW: la aplicación es `windows_subsystem = "windows"`
            // y no tiene consola, así que sin este flag Windows le regala una
            // ventana nueva al proceso hijo. Se veía parpadear en el PRIMER
            // arranque, que es el único momento en que se detecta el idioma.
            use std::os::windows::process::CommandExt;
            let mut ps = std::process::Command::new("powershell");
            ps.args(["-NoProfile", "-Command", "(Get-Culture).TwoLetterISOLanguageName"])
                .creation_flags(0x0800_0000);
            if let Ok(out) = ps.output() {
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
            "Para lo que ningún extractor puede (perfiles de Douyin, Threads, contenido con sesión): el script corre en la pestaña del perfil, lee lo que la propia página recibe y te lo trae a la aplicación.",
            "For what no extractor can reach (Douyin profiles, Threads, session-gated content): the script runs in the profile tab, reads what the page itself receives, and brings it into the application."
        ),
        "cap.listening" => entry!("● Receptor escuchando en", "● Receiver listening on"),
        "cap.off" => entry!("● Receptor desactivado", "● Receiver disabled"),
        "cap.enable" => entry!("Activar receptor", "Enable receiver"),
        "cap.bind_failed" => entry!("● El receptor NO ha podido abrir el puerto", "● The receiver could NOT open its port"),
        "cap.bind_help" => entry!(
            "Lo captura todo pero no llega nada: el script dirá «App no encontrada» y caerá al botón 💾 Guardar JSON. Casi siempre es otra copia de Todo Downloader abierta, que se quedó con el puerto. Cierra la otra y reinicia esta, o cambia el puerto aquí abajo y reinicia.",
            "Everything gets captured and nothing arrives: the script will say «App not found» and fall back to the 💾 Save JSON button. It is almost always another copy of Todo Downloader that took the port. Close the other one and restart this, or change the port below and restart."
        ),
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
            "3. Espera a que termine. TikTok, Douyin y V2PH van a «Descargas»; Threads va a «Perfil», con vista previa para elegir qué bajas.",
            "3. Wait for it to finish. TikTok, Douyin and V2PH go to «Downloads»; Threads goes to «Profile», with previews so you can pick what to fetch."
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
            "Perfiles de TikTok, Weibo, Bilibili y V2PH: analiza y elige qué descargar. Instagram, X, Facebook, Pinterest y Bluesky: descarga completa con gallery-dl, y casi todos necesitan cookies. Douyin y Threads: desde la pestaña Capturar; lo de Threads vuelve aquí, a la rejilla.",
            "TikTok, Weibo, Bilibili and V2PH profiles: analyze and pick what to download. Instagram, X, Facebook, Pinterest and Bluesky: full download via gallery-dl, and most of them need cookies. Douyin and Threads: from the Capture tab — and Threads comes back here, to the grid."
        ),
        "profile.url_label" => entry!("URL DEL PERFIL", "PROFILE URL"),
        "profile.want" => entry!("Quiero descargar:", "I want to download:"),
        "profile.videos" => entry!("🎬 Vídeos (del análisis)", "🎬 Videos (from analysis)"),
        "profile.images" => entry!("🖼 Imágenes (del análisis)", "🖼 Images (from analysis)"),
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
        "profile.threads_unsupported" => entry!(
            "Threads no lo lista ningún extractor. Te he abierto la pestaña Capturar con su script: instálalo como userscript, abre el perfil y pulsa el botón. Lo capturado vuelve aquí, a la rejilla.",
            "No extractor lists Threads. I have opened the Capture tab on its script: install it as a userscript, open the profile and press the button. What it captures comes back here, to the grid."
        ),
        "profile.threads_note" => entry!(
            "▸ Threads se captura desde el navegador: no existe extractor, y Meta firma los enlaces de su CDN, así que la URL del original solo está dentro de la respuesta que recibe la propia página. Pulsa Analizar y te llevo al script.",
            "▸ Threads is captured from the browser: no extractor exists, and Meta signs its CDN links, so the original's URL only lives inside the response the page itself receives. Press Analyze and I will take you to the script."
        ),
        "profile.need_galdl" => entry!(
            "Instala primero gallery-dl en Ajustes (un clic)",
            "Install gallery-dl first in Settings (one click)"
        ),
        "profile.gallery_note" => entry!(
            "ℹ Instagram, X, Facebook, Weibo y Bluesky se exploran con vista previa: analiza y elige qué bajar, con su resolución. Pinterest y similares se descargan enteros. Instagram, X y Facebook EXIGEN sesión para listar: cookies del navegador o un cookies.txt en Ajustes.\n⏳ Facebook lista MUY despacio, y no es cosa de la app: su extractor abre la página completa de CADA foto, una por una, y no sabe saltar. Llegan de 24 en 24 y se traen tres tandas solas, hasta 72; a partir de ahí, «Cargar más». Cada tanda tarda más que la anterior porque vuelve a recorrer lo ya visto, y eso cuesta igual lo pida la app o lo pidas tú. Para un perfil grande sale más a cuenta pegar la URL en Descargas y bajarlo entero.",
            "ℹ Instagram, X, Facebook, Weibo and Bluesky are browsed with previews: analyze, then pick what to download, resolution included. Pinterest and similar are downloaded whole. Instagram, X and Facebook REQUIRE a session to list: browser cookies or a cookies.txt in Settings.\n⏳ Facebook lists VERY slowly, and that is not the application: its extractor opens the full page of EVERY photo, one at a time, and cannot skip ahead. They arrive 24 at a time and three batches are fetched on their own, up to 72; after that, «Load more». Each batch takes longer than the last because it walks everything already seen, and that costs the same whether the app asks or you do. For a large profile it is quicker to paste the URL into Downloads and get the whole thing."
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
        "set.prefer_br" => entry!("Priorizar bitrate sobre eficiencia de códec",
                                  "Prefer bitrate over codec efficiency"),
        "set.prefer_br_note" => entry!(
            "Misma resolución y mismos fps, pero de los formatos disponibles se coge el de más bitrate: normalmente H.264, que pesa más y lo reproduce todo. Desactivado se usan códecs modernos (AV1/VP9), con archivos bastante más pequeños.",
            "Same resolution and same fps, but of the available formats the one with the highest bitrate is taken: usually H.264, which is larger and plays everywhere. Turned off, modern codecs (AV1/VP9) are used, with considerably smaller files."),
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
            "⚠ EN WINDOWS, Chrome 127+ (y Edge/Brave/Opera) cifran sus cookies con App-Bound Encryption y ninguna herramienta externa puede leerlas. Usa Firefox, o el archivo cookies.txt de abajo. En Linux y macOS sí se pueden leer (macOS pedirá permiso del Llavero).",
            "⚠ ON WINDOWS, Chrome 127+ (and Edge/Brave/Opera) encrypt cookies with App-Bound Encryption and no external tool can read them. Use Firefox, or the cookies.txt file below. On Linux and macOS they can be read (macOS will ask for Keychain permission)."
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
        // ---------- Historial de gallery-dl ----------
        "set.history" => entry!("HISTORIAL DE GALERÍAS", "GALLERY HISTORY"),
        "set.history_note" => entry!(
            "gallery-dl anota lo ya descargado para que «Reintentar» continúe donde se cortó \
             en vez de empezar de cero. Vacíalo si quieres volver a bajar algo que ya tenías.",
            "gallery-dl records what it already downloaded so that “Retry” resumes where it \
             stopped instead of starting over. Clear it if you want to re-download something."
        ),
        "btn.clear_history" => entry!("🧹  Vaciar historial", "🧹  Clear history"),
        "toast.history_cleared" => entry!("Historial vaciado", "History cleared"),
        "toast.history_empty" => entry!("El historial ya estaba vacío", "History was already empty"),

        // ---------- Errores con explicación ----------
        "err.instagram_login" => entry!(
            "Instagram exige sesión para listar el perfil entero: activa las cookies en Ajustes \
             (Firefox, o un archivo cookies.txt) y pulsa Reintentar",
            "Instagram requires a signed-in session to list a full profile: enable cookies in \
             Settings (Firefox, or a cookies.txt file) and press Retry"
        ),

        "side.cookies_on" => entry!("● cookies activas", "● cookies enabled"),
        "side.cookies_off" => entry!("○ sin cookies", "○ no cookies"),
        "side.cookies_off_tip" => entry!(
            "Sin sesión del navegador solo funcionan con garantía los enlaces directos y \
             YouTube público.\n\nInstagram, Weibo, Douyin, TikTok privado, V2PH y la mayoría \
             de galerías necesitan cookies: sin ellas el sitio responde 401 o 403, o entrega \
             una versión recortada.\n\nActívalas en Ajustes → Cookies del navegador, o \
             indica un cookies.txt exportado con una extensión.",
            "Without a browser session, only direct links and public YouTube are reliable.\
             \n\nInstagram, Weibo, Douyin, private TikTok, V2PH and most galleries need \
             cookies: without them the site answers 401 or 403, or serves a cut-down \
             version.\n\nTurn them on in Settings → Browser cookies, or point to a \
             cookies.txt exported with an extension."),
        "profile.sites" => entry!("SITIOS SOPORTADOS", "SUPPORTED SITES"),
        "profile.sites_grid" => entry!(
            "▸ Con vista previa y selección, viendo resolución y tipo de cada archivo:  TikTok · Instagram · X (Twitter) · Facebook · Weibo · Bilibili · Bluesky · V2PH",
            "▸ With preview and selection, showing each file's resolution and type:  TikTok · Instagram · X (Twitter) · Facebook · Weibo · Bilibili · Bluesky · V2PH"),
        "profile.sites_whole" => entry!(
            "▸ Se descargan enteros, sin listado previo:  Pinterest · Tumblr · DeviantArt · Flickr · ArtStation · boorus (Danbooru, Gelbooru, Safebooru, e621, Konachan, yande.re, rule34…)",
            "▸ Downloaded whole, with no listing first:  Pinterest · Tumblr · DeviantArt · Flickr · ArtStation · boorus (Danbooru, Gelbooru, Safebooru, e621, Konachan, yande.re, rule34…)"),
        "profile.sites_cookies" => entry!(
            "▸ Necesitan tu sesión:  Instagram, X, Facebook, Weibo, Douyin, Threads y cualquier perfil privado o con restricción de edad. Actívala en Ajustes → Cookies del navegador, o carga un cookies.txt. En Threads la sesión es la del propio navegador, no hace falta exportar nada.",
            "▸ Need your session:  Instagram, X, Facebook, Weibo, Douyin, Threads and any private or age-restricted profile. Turn it on in Settings → Browser cookies, or load a cookies.txt. On Threads the session is the browser's own — nothing to export."),
        "profile.sites_capture" => entry!(
            "▸ Solo desde la pestaña Capturar:  Threads y los perfiles de Douyin. No los lista ningún extractor, así que se leen desde el navegador con la sesión que ya tienes abierta. Lo de Threads acaba igualmente aquí, en la rejilla, con su resolución. Para un post suelto de Douyin o TikTok, el capturador de posts es más rápido que analizar el perfil entero.",
            "▸ From the Capture tab only:  Threads, and Douyin profiles. No extractor lists them, so they are read from the browser using the session you already have open. Threads still ends up here, in the grid, with its resolution. For a single Douyin or TikTok post, the post capturer is quicker than analyzing the whole profile."),
        "profile.sites_no" => entry!(
            "▸ No soportado:  los servicios de streaming con DRM (Netflix, Disney+, Prime Video y similares). No es una limitación que se pueda enrutar desde aquí: el contenido va cifrado y saltárselo sería eludir una medida técnica de protección.",
            "▸ Not supported:  DRM-protected streaming services (Netflix, Disney+, Prime Video and the like). This is not something that can be routed from here: the content is encrypted, and getting around that would be circumventing a technological protection measure."),
        "profile.sites_note" => entry!(
            "Los demás sitios con vídeo —YouTube, X, Reddit, Twitch, Vimeo y otros mil— funcionan pegando el enlace directamente en Descargas.",
            "Any other video site — YouTube, X, Reddit, Twitch, Vimeo and a thousand more — works by pasting the link straight into Downloads."),
        "cap.post_title" => entry!("BOTÓN EN LA PROPIA PÁGINA (RECOMENDADO PARA THREADS)", "A BUTTON ON THE PAGE ITSELF (RECOMMENDED FOR THREADS)"),
        "cap.post_help" => entry!(
            "Se instala una vez y aparece un botón flotante: «⬇ Capturar este post» en cada publicación de Douyin y TikTok, y «⬇ Capturar este perfil» en Threads. En Threads es la mejor opción y no solo por comodidad: engancha antes de que la página pida nada, así que no se pierde lo que cargue mientras tanto, y usa GM_xmlhttpRequest, que sí alcanza a la aplicación en Chrome y Vivaldi.",
            "You install it once and a floating button appears: «⬇ Capture this post» on every Douyin and TikTok publication, and «⬇ Capture this profile» on Threads. On Threads this is the better option, and not only for convenience: it hooks in before the page requests anything, so nothing loaded meanwhile is lost, and it uses GM_xmlhttpRequest, which does reach the application on Chrome and Vivaldi."),
        "cap.post_us" => entry!("Copiar userscript", "Copy userscript"),
        "cap.post_us_save" => entry!("Guardar .user.js", "Save .user.js"),
        "cap.post_bm" => entry!("Copiar bookmarklet", "Copy bookmarklet"),
        "cap.post_us_note" => entry!(
            "USERSCRIPT — necesita Tampermonkey o Violentmonkey. Es la opción recomendada en Chrome y Vivaldi: usa GM_xmlhttpRequest, que llega a la aplicación sin tropezar con el bloqueo de red privada del navegador.",
            "USERSCRIPT — needs Tampermonkey or Violentmonkey. This is the recommended option on Chrome and Vivaldi: it uses GM_xmlhttpRequest, which reaches the application without hitting the browser's private-network block."),
        "cap.post_bm_note" => entry!(
            "BOOKMARKLET — sin instalar nada: pega el texto copiado como DIRECCIÓN de un marcador nuevo y arrástralo a la barra. Ojo: al correr dentro de la página, en Chrome y Vivaldi no puede hablar con la aplicación y cae al respaldo de guardar JSON. En Firefox llega directo.",
            "BOOKMARKLET — nothing to install: paste the copied text as the ADDRESS of a new bookmark and drag it to the bar. Careful: running inside the page, on Chrome and Vivaldi it cannot reach the application and falls back to saving a JSON. On Firefox it arrives directly."),
        "cap.post_copied" => entry!("Copiado. Ahora pégalo donde toque.", "Copied. Now paste it where it belongs."),
        "set.post_grid" => entry!(
            "Los posts sueltos van a la rejilla de selección",
            "Single posts go to the selection grid"),
        "set.post_grid_note" => entry!(
            "Activado, capturar un post lo abre en Perfil con miniaturas y casillas para que elijas qué bajar. Desactivado, entra entero en la cola. La captura de un PERFIL completo no se ve afectada: esa siempre va a la rejilla.",
            "Turned on, capturing a post opens it in Profile with thumbnails and checkboxes so you can pick what to download. Turned off, it goes straight into the queue in full. Capturing a whole PROFILE is unaffected: that always goes to the grid."),
        "queue.no_session" => entry!(
            "⚠ Este fallo parece falta de sesión y ahora mismo no se están enviando cookies. \
             Actívalas en Ajustes → Cookies del navegador, o carga un cookies.txt, y pulsa Reintentar.",
            "⚠ This failure looks like a missing session, and no cookies are being sent right \
             now. Turn them on in Settings → Browser cookies, or load a cookies.txt, then press Retry."),
        "side.cyberdrop_active" => entry!("● cyberdrop-dl activo", "● cyberdrop-dl active"),

        "status.resolving_host" => entry!("Resolviendo…", "Resolving…"),
        "status.resolving_mega" => entry!("resolviendo MEGA", "resolving MEGA"),
        "gal.listing" => entry!("Listando publicaciones (sin descargar)…", "Listing posts (nothing downloaded)…"),
        "gal.images" => entry!("Imágenes", "Images"),
        "gal.videos" => entry!("Vídeos", "Videos"),
        "gal.select_all" => entry!("Marcar todo", "Select all"),
        "gal.select_none" => entry!("Desmarcar", "Select none"),
        "gal.queue_selected" => entry!("Añadir seleccionados a la cola", "Add selected to queue"),
        "gal.more" => entry!("Cargar más", "Load more"),
        "gal.prefetching" => entry!("trayendo más en segundo plano…", "fetching more in the background…"),
        "gal.stop" => entry!("■ Detener", "■ Stop"),
        "gal.stopped" => entry!("Exploración detenida", "Search stopped"),
        "gal.no_more" => entry!("No hay más publicaciones", "No more posts"),
        "gal.empty" => entry!("gallery-dl no devolvió nada. Suele ser falta de sesión: comprueba que la tienes abierta en el navegador elegido en Ajustes, o usa un cookies.txt.",
                               "gallery-dl returned nothing. This is usually a missing session: check that you are logged in on the browser selected in Settings, or use a cookies.txt file."),
        "gal.reason" => entry!("Lo que dijo gallery-dl:", "What gallery-dl said:"),
        "gal.expiry_note" => entry!("Los enlaces caducan en horas: descarga pronto lo que marques",
                                    "Links expire within hours: download your selection soon"),
        "status.verifying" => entry!("verificando integridad", "verifying integrity"),
        "set.mega" => entry!("MEGA: enlaces publicos activos (nativo, sin cuenta)",
                             "MEGA: public links active (native, no account)"),
        "side.mega_active" => entry!("● MEGA activo", "● MEGA active"),

        // ---------- BitTorrent ----------
        "nav.torrents" => entry!("Torrent", "Torrent"),
        "torrent.title" => entry!("Descargas por Torrent", "Torrent downloads"),
        "torrent.subtitle" => entry!(
            "Pega un enlace magnet o un archivo .torrent. Motor BitTorrent nativo (DHT, sin proceso externo).",
            "Paste a magnet link or a .torrent file. Native BitTorrent engine (DHT, no external process)."
        ),
        "torrent.add_label" => entry!("MAGNET O ARCHIVO .TORRENT", "MAGNET OR .TORRENT FILE"),
        "torrent.pick_file" => entry!("📄 Archivo…", "📄 File…"),
        "torrent.add_btn" => entry!("➕ Añadir torrent", "➕ Add torrent"),
        "torrent.adding" => entry!("Iniciando sesión BitTorrent…", "Starting BitTorrent session…"),
        "torrent.added" => entry!("Torrent añadido", "Torrent added"),
        "torrent.empty" => entry!("No hay torrents activos", "No active torrents"),
        "torrent.remove" => entry!("Quitar de la lista (conserva archivos)", "Remove from list (keeps files)"),
        "torrent.state_init" => entry!("Conectando…", "Connecting…"),
        "torrent.state_down" => entry!("Descargando", "Downloading"),
        "torrent.state_seeding" => entry!("Compartiendo", "Seeding"),
        "torrent.legal" => entry!(
            "⚠ Al descargar por torrent también compartes (subes) el contenido. Úsalo solo con material legal.",
            "⚠ Downloading via torrent also shares (uploads) the content. Use only with legal material."
        ),
        // ---------- Manejador de magnet ----------
        // ---------- Booru Browser ----------
        "nav.booru" => entry!("Booru", "Booru"),
        "booru.title" => entry!("Buscador de boorus", "Booru browser"),
        "booru.subtitle" => entry!(
            "Busca por etiquetas, revisa las miniaturas y descarga solo lo que elijas, siempre en calidad original.",
            "Search by tags, review the thumbnails and download only what you pick — always the original quality."
        ),
        "booru.tags" => entry!("ETIQUETAS (separadas por espacios)", "TAGS (space separated)"),
        "booru.search" => entry!("🔍  Buscar", "🔍  Search"),
        "booru.samples" => entry!("✨  Ejemplos…", "✨  Examples…"),
        "booru.min_width" => entry!("Ancho mínimo:", "Min width:"),
        "booru.rating" => entry!("Clasificación:", "Rating:"),
        "booru.rating_all" => entry!("Todo", "All"),
        "booru.rating_safe" => entry!("General", "General"),
        "booru.rating_sensitive" => entry!("Sensible", "Sensitive"),
        "booru.next" => entry!("Página siguiente", "Next page"),
        "booru.prev" => entry!("Página anterior", "Previous page"),
        "booru.needs_auth" => entry!(
            "⚠ Este sitio exige credenciales de API: añádelas en Ajustes → Cuentas de booru.",
            "⚠ This site requires API credentials: add them in Settings → Booru accounts."
        ),
        "set.ua" => entry!("USER-AGENT (AVANZADO)", "USER-AGENT (ADVANCED)"),
        "set.ua_clear" => entry!("Por defecto", "Default"),
        "set.ua_detect" => entry!("🔎  Detectar desde mi navegador", "🔎  Detect from my browser"),
        "set.ua_detected" => entry!("✓ User-Agent detectado y guardado", "✓ User-Agent detected and saved"),
        "set.ua_need_receiver" => entry!(
            "Activa antes «Receptor local» en esta misma pantalla: es quien lee el dato del navegador.",
            "Enable «Local receiver» above first: it is what reads the value from the browser."
        ),
        "set.ua_note" => entry!(
            "Déjalo vacío salvo que un sitio protegido por Cloudflare te rechace pese a tener un cookies.txt válido. La cookie «cf_clearance» que certifica que superaste la verificación está atada a tu IP Y a tu User-Agent: si no coincide, se descarta. El botón abre una página local en tu navegador predeterminado y lee el dato de la propia petición. Si quieres el de OTRO navegador, pega esa dirección en él.",
            "Leave empty unless a Cloudflare-protected site rejects you despite a valid cookies.txt. The «cf_clearance» cookie proving you passed the check is tied to your IP AND your User-Agent: a mismatch makes it worthless. The button opens a local page in your default browser and reads the value from the request itself. For a DIFFERENT browser, paste that address into it."
        ),
        "set.v2ph" => entry!("CUENTA DE V2PH", "V2PH ACCOUNT"),
        "set.v2ph_user" => entry!("Usuario:", "Username:"),
        "set.v2ph_pass" => entry!("Contraseña:", "Password:"),
        "set.v2ph_login" => entry!("Iniciar sesión", "Sign in"),
        "set.v2ph_logout" => entry!("Cerrar sesión", "Sign out"),
        "set.v2ph_note" => entry!(
            "V2PH solo enseña las 10 primeras fotos de un álbum a quien no ha entrado. La contraseña se envía a V2PH y NO se guarda en ningún sitio: solo se conserva la credencial de sesión que devuelve el sitio, igual que haría un navegador. Cerrar sesión la borra.",
            "V2PH only shows the first 10 photos of an album to visitors. Your password is sent to V2PH and is NOT stored anywhere: only the session credential the site returns is kept, exactly as a browser would. Signing out deletes it."
        ),
        "v2ph.ok" => entry!("Sesión de V2PH iniciada y comprobada", "Signed in to V2PH and verified"),
        "v2ph.out" => entry!("Sesión de V2PH cerrada", "Signed out of V2PH"),
        "set.booru" => entry!("CUENTAS DE BOORU", "BOORU ACCOUNTS"),
        "set.booru_user" => entry!("Usuario / user-id:", "Username / user-id:"),
        "set.booru_key" => entry!("Clave de API:", "API key:"),
        "set.booru_note" => entry!(
            "Solo hacen falta para Gelbooru (obligatorias) y para funciones de cuenta en Danbooru. Se guardan en tu configuración local y se pasan al motor sin aparecer en registros.",
            "Only needed for Gelbooru (mandatory) and for account features on Danbooru. Stored in your local settings and passed to the engine without appearing in logs."
        ),

        "gal.analyzing" => entry!("Analizando…", "Analyzing…"),
        "gal.current" => entry!("Descargando ahora:", "Downloading now:"),

        // ---------- Pestaña «Tip my Work» ----------
        "nav.tip" => entry!("Invítame a algo", "Tip my Work"),
        "tip.title" => entry!("¡Invítame a un café o a una cerveza!", "Buy me a coffee or a Beer!"),
        "tip.subtitle" => entry!(
            "Gratis, abierto y sin anuncios — y quiero que siga siendo así.",
            "Free, open and ad-free — and I'd like to keep it that way."
        ),
        "tip.msg1" => entry!(
            "Todo Downloader y mis otras herramientas son gratuitas, de código abierto, sin anuncios y sin telemetría, y me gustaría que siguieran siéndolo.",
            "Todo Downloader and my other software tools are free, open source, ad-free and telemetry-free — and I would like to keep them that way."
        ),
        "tip.msg2" => entry!(
            "Las desarrollo y mantengo después del trabajo, en mi tiempo libre. Cada cambio en una web, cada extractor que se rompe y cada fallo propio de una plataforma lleva su tiempo de investigar y arreglar.",
            "I develop and maintain them after work and during my free time. Every website change, broken extractor and platform-specific bug takes time to investigate and fix."
        ),
        "tip.msg3" => entry!(
            "Si Todo Downloader te ha ahorrado horas de descargas manuales, invitarme a un café o a una cerveza es una forma sencilla de ayudar a que el proyecto siga vivo y bien mantenido.",
            "If Todo Downloader has saved you hours of manual downloading, buying me a coffee or a beer is a small way to help keep the project alive and actively maintained."
        ),
        "tip.msg4" => entry!(
            "El apoyo es totalmente opcional y no hay ninguna función bloqueada tras una donación.",
            "Support is completely optional, and no features are locked behind donations."
        ),
        "tip.thanks" => entry!("¡Gracias! ;)", "Thank you! ;)"),
        "tip.help" => entry!(
            "Ayuda a mantener mis proyectos de código abierto.",
            "Help maintain my open-source projects."
        ),
        "tip.no_links" => entry!(
            "Aún no hay enlaces configurados: edita KOFI_URL, PAYPAL_URL y SPONSORS_URL al principio de src/main.rs.",
            "No links configured yet: edit KOFI_URL, PAYPAL_URL and SPONSORS_URL at the top of src/main.rs."
        ),

        // ---------- Apoyo al proyecto ----------

        "support.title" => entry!("❤  APOYAR EL PROYECTO", "❤  SUPPORT THIS PROJECT"),
        "support.body" => entry!(
            "Todo Downloader es gratuito, de código abierto, sin anuncios y sin telemetría. Si te ahorra tiempo, puedes apoyar su desarrollo.",
            "Todo Downloader is free, open source, ad-free and telemetry-free. If it saves you time, you can support its development."
        ),
        "support.kofi" => entry!("☕  Ko-fi", "☕  Ko-fi"),
        "support.paypal" => entry!("PayPal", "PayPal"),
        "support.sponsors" => entry!("GitHub Sponsors", "GitHub Sponsors"),
        "support.optional" => entry!(
            "Totalmente opcional: no hay ninguna función bloqueada. Los botones solo abren el navegador; la aplicación nunca ve pagos ni datos bancarios.",
            "Entirely optional — no feature is ever locked. The buttons just open your browser; the app never sees payments or banking details."
        ),

        "set.theme" => entry!("TEMA", "THEME"),
        "theme.classic" => entry!("Clásico", "Classic"),
        "theme.sober" => entry!("Sobrio", "Sober"),
        "set.bg_image" => entry!("FONDO PERSONALIZADO", "CUSTOM BACKGROUND"),
        "set.bg_pick" => entry!("🖼  Elegir imagen…", "🖼  Choose image…"),
        "set.bg_clear" => entry!("Quitar fondo", "Remove background"),
        "set.bg_opacity" => entry!("Intensidad:", "Strength:"),
        "set.bg_blur" => entry!("Desenfoque:", "Blur:"),
        "set.bg_blur_off" => entry!("nítido", "sharp"),
        "set.bg_note" => entry!(
            "Se aplica solo al panel principal; la barra lateral sigue sólida para que el menú se lea siempre. Baja la intensidad si molesta al leer.",
            "Applied to the main panel only; the sidebar stays solid so the menu is always readable. Lower the strength if it hurts legibility."
        ),
        "set.theme_note" => entry!(
            "Clásico: oscuro con acento rosa. Sobrio: gris pizarra, discreto. Hot Pink: rosa intenso con halos difuminados de fondo.",
            "Classic: dark with pink accent. Sober: slate grey, understated. Hot Pink: vivid pink with soft background glows."
        ),

        "btn.clear_list" => entry!("🗑  Vaciar lista", "🗑  Clear list"),
        "btn.remove_selected" => entry!("🗑  Quitar marcados", "🗑  Remove checked"),

        "set.magnet" => entry!("ENLACES MAGNET", "MAGNET LINKS"),
        "set.magnet_on" => entry!(
            "Todo Downloader abre los enlaces magnet ✓",
            "Todo Downloader opens magnet links ✓"
        ),
        "set.magnet_off" => entry!(
            "Otro programa (qBittorrent, µTorrent…) abre los enlaces magnet",
            "Another program (qBittorrent, µTorrent…) opens magnet links"
        ),
        "set.magnet_register" => entry!("🧲  Abrir los magnet con Todo Downloader", "🧲  Open magnet links with Todo Downloader"),
        "set.magnet_unregister" => entry!("Dejar de gestionar los magnet", "Stop handling magnet links"),
        "set.magnet_done" => entry!("Listo: los magnet abrirán Todo Downloader", "Done: magnet links will open Todo Downloader"),
        "set.magnet_removed" => entry!("Registro eliminado", "Registration removed"),
        "set.magnet_note" => entry!(
            "Se registra solo para tu usuario (HKCU), sin permisos de administrador. Si la app ya está abierta, el enlace va a esa ventana en vez de abrir otra.",
            "Registered for your user only (HKCU), no admin rights needed. If the app is already running, the link goes to that window instead of opening another."
        ),
        "set.magnet_userchoice" => entry!(
            "⚠ Si ya tienes otro cliente por defecto (qBittorrent…), Windows lo protege con «UserChoice» y ninguna app puede cambiarlo por código. Ve a Configuración → Aplicaciones → Aplicaciones predeterminadas → Elegir predeterminados por tipo de vínculo → MAGNET, y selecciona Todo Downloader.",
            "⚠ If another client is already the default (qBittorrent…), Windows protects it with “UserChoice” and no app can change it programmatically. Go to Settings → Apps → Default apps → Choose defaults by link type → MAGNET, and pick Todo Downloader."
        ),
        "set.magnet_open_settings" => entry!("⚙  Abrir Configuración de Windows", "⚙  Open Windows Settings"),

        "torrent.options" => entry!("⚙  Carpeta y velocidad", "⚙  Folder & speed"),
        "torrent.folder_label" => entry!("CARPETA DE DESTINO", "DOWNLOAD FOLDER"),
        "torrent.limits_label" => entry!("LÍMITES DE VELOCIDAD", "SPEED LIMITS"),
        "torrent.down_limit" => entry!("Descarga:", "Download:"),
        "torrent.up_limit" => entry!("Subida:", "Upload:"),
        "torrent.limit_zero" => entry!("0 = sin límite", "0 = unlimited"),
        "torrent.peers_tip" => entry!(
            "Peers conectados ahora mismo (BitTorrent no separa seeders de leechers a nivel agregado)",
            "Peers connected right now (BitTorrent doesn't split seeders from leechers at aggregate level)"
        ),
        "torrent.limit_restart" => entry!(
            "Los límites se aplican al iniciar la sesión; si ya hay torrents, reinicia la app para cambiarlos.",
            "Limits apply when the session starts; if torrents are already running, restart the app to change them."
        ),

        // ---------- Hosters de archivos ----------
        "err.need_cyberdrop" => entry!(
            "Este hoster necesita el motor cyberdrop-dl: instálalo en Ajustes (requiere Python)",
            "This host needs the cyberdrop-dl engine: install it in Settings (requires Python)"
        ),
        "eng.cyberdrop" => entry!("MOTOR CYBERDROP-DL (HOSTERS DIFÍCILES)", "CYBERDROP-DL ENGINE (HARD HOSTS)"),
        "eng.cyberdrop_ok" => entry!("cyberdrop-dl: instalado ✓", "cyberdrop-dl: installed ✓"),
        "eng.cyberdrop_missing" => entry!(
            "cyberdrop-dl: no instalado — opcional, para Bunkr, Cyberdrop y similares",
            "cyberdrop-dl: not installed — optional, for Bunkr, Cyberdrop and similar"
        ),
        "eng.install_cyberdrop" => entry!("⬇  Instalar cyberdrop-dl", "⬇  Install cyberdrop-dl"),
        "eng.cyberdrop_downloading" => entry!(
            "Instalando cyberdrop-dl vía uv (descarga Python, tarda un poco)…",
            "Installing cyberdrop-dl via uv (downloads Python, takes a while)…"
        ),
        "eng.cyberdrop_note" => entry!(
            "Opcional. Usa Python gestionado por uv (~astral.sh). Solo hace falta para hosters que se defienden de los scrapers; Pixeldrain, GoFile y MediaFire NO lo necesitan.",
            "Optional. Uses Python managed by uv (~astral.sh). Only needed for hosts that fight scrapers; Pixeldrain, GoFile and MediaFire do NOT need it."
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

pub fn v2ph_signed_in(lang: Lang, usuario: &str) -> String {
    match lang {
        Lang::Es => format!("Sesión iniciada como {usuario}"),
        Lang::En => format!("Signed in as {usuario}"),
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

/// Recuento de archivos completados de una galería. No hay total conocido:
/// gallery-dl no lo sabe hasta terminar.
pub fn files_done(lang: Lang, n: u64) -> String {
    match lang {
        Lang::Es => format!("{n} archivos"),
        Lang::En => format!("{n} files"),
    }
}

pub fn list_cleared(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("🗑 {n} elemento(s) quitados de la lista"),
        Lang::En => format!("🗑 {n} item(s) removed from the list"),
    }
}

pub fn host_resolved(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("🔗 {n} archivo(s) resueltos del hoster"),
        Lang::En => format!("🔗 {n} file(s) resolved from the host"),
    }
}

pub fn booru_summary(lang: Lang, shown: usize, sel: usize) -> String {
    match lang {
        Lang::Es => format!("{shown} mostrados · {sel} seleccionados"),
        Lang::En => format!("{shown} shown · {sel} selected"),
    }
}

pub fn booru_add(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("➕  Añadir {n} a la cola"),
        Lang::En => format!("➕  Add {n} to queue"),
    }
}

pub fn booru_found(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("🔍 {n} resultados"),
        Lang::En => format!("🔍 {n} results"),
    }
}

pub fn booru_error(lang: Lang, e: &str) -> String {
    match lang {
        Lang::Es => format!("Búsqueda fallida: {e}"),
        Lang::En => format!("Search failed: {e}"),
    }
}

pub fn torrent_error(lang: Lang, e: &str) -> String {
    match lang {
        Lang::Es => format!("Error de torrent: {e}"),
        Lang::En => format!("Torrent error: {e}"),
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

/// Cuántos elementos esconde el filtro de imágenes/vídeos.
pub fn hidden_by_filter(lang: Lang, n: usize) -> String {
    match lang {
        Lang::Es => format!("({n} ocultos por el filtro)"),
        _ => format!("({n} hidden by the filter)"),
    }
}
