//! Receptor local estilo "Click'n'Load": un endpoint HTTP mínimo escuchando
//! SOLO en 127.0.0.1 que recibe los enlaces capturados por el script del
//! navegador y los mete directamente en la cola.
//!
//! Seguridad:
//!  - Bind exclusivo a 127.0.0.1 (nunca accesible desde la red).
//!  - Opt-in: se activa desde Ajustes.
//!  - Cuerpo limitado a 8 MiB; solo se aceptan URLs http(s).
//!  - No ejecuta nada de lo recibido: únicamente encola descargas.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_BODY: usize = 8 * 1024 * 1024;

/// Un enlace recibido del navegador
#[derive(Debug, Clone)]
pub struct Incoming {
    pub url: String,
    pub author: String,
    pub title: String,
    pub page_url: String,
    pub id: String,
    /// URL de la portada (opcional): se usa solo para la miniatura de la cola
    pub thumb: String,
}

/// A dónde quiere el script que vayan los enlaces que manda.
///
/// POR QUÉ LO DECIDE EL SCRIPT Y NO SOLO LA APP: capturar un perfil entero y
/// capturar un post suelto son gestos distintos. Del perfil quieres todo; del
/// post sueles querer elegir. El script sabe cuál de los dos hizo el usuario,
/// la aplicación no. `Auto` deja la última palabra al ajuste, que es lo que
/// reciben el texto plano y los scripts antiguos que no mandan nada.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Destino {
    /// Texto plano o scripts antiguos que no dicen nada: a la cola.
    #[default]
    Auto,
    /// Captura de un POST suelto. El script no dice a dónde va, solo QUÉ es;
    /// el destino lo decide el ajuste de la aplicación. Así cambiar de idea no
    /// obliga a reinstalar el userscript en el navegador.
    Post,
    /// Forzar cola
    Cola,
    /// Forzar rejilla de selección
    Seleccion,
}

/// Lo que el receptor puede entregarle a la aplicación.
pub enum Recibido {
    /// Enlaces capturados por el script del navegador
    Enlaces(Vec<Incoming>, Destino),
    /// User-Agent del navegador que visitó `/ua`.
    ///
    /// POR QUÉ AQUÍ: la aplicación no puede preguntarle su User-Agent al
    /// navegador, pero el navegador lo manda solo en CADA petición. Basta con
    /// abrirle una dirección propia y leer la cabecera. Sin adivinar versiones,
    /// sin leer archivos de configuración y sin depender del navegador ni del
    /// sistema operativo.
    UserAgent(String),
}

/// Arranca el receptor. `on_items` se invoca con cada cosa recibida.
pub fn spawn<F>(port: u16, enabled: Arc<AtomicBool>, on_items: F)
where
    F: Fn(Recibido) + Send + 'static,
{
    std::thread::spawn(move || {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(l) => l,
            Err(_) => return, // puerto ocupado: el resto de la app sigue funcionando
        };
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if !enabled.load(Ordering::Relaxed) {
                let _ = respond(stream, 503, "receiver disabled");
                continue;
            }
            match handle(stream) {
                Some(Recibido::Enlaces(items, _)) if items.is_empty() => {}
                Some(r) => on_items(r),
                None => {}
            }
        }
    });
}

/// Lee la petición, responde y devuelve los enlaces si era un POST válido
fn handle(mut stream: TcpStream) -> Option<Recibido> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut header_end = None;

    // Leer hasta el fin de cabeceras
    while header_end.is_none() && buf.len() < MAX_BODY {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                header_end = find_header_end(&buf);
            }
            Err(_) => break,
        }
    }
    let head_len = header_end?;
    let head = String::from_utf8_lossy(&buf[..head_len]).to_string();
    let first_line = head.lines().next().unwrap_or("").to_string();

    // Preflight CORS del navegador
    if first_line.starts_with("OPTIONS") {
        let _ = respond(stream, 204, "");
        return None;
    }
    if !first_line.starts_with("POST") {
        // GET /ua: el navegador viene a decirnos quién es. Su cabecera
        // User-Agent es exactamente la línea que Cloudflare asoció a la
        // cookie `cf_clearance`, así que es la que la aplicación debe repetir.
        if first_line.starts_with("GET /ua") {
            let ua = cabecera(&head, "user-agent").unwrap_or_default();
            let pagina = if ua.is_empty() {
                PAGINA_ERROR.to_string()
            } else {
                PAGINA_OK.replace("{UA}", &escapar_html(&ua))
            };
            let _ = responder_html(stream, &pagina);
            return if ua.is_empty() {
                None
            } else {
                Some(Recibido::UserAgent(ua))
            };
        }
        let _ = respond(stream, 200, "Todo Downloader receiver OK");
        return None;
    }

    // Leer el cuerpo completo según Content-Length
    let want = content_length(&head).unwrap_or(0).min(MAX_BODY);
    while buf.len() < head_len + want {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let body = String::from_utf8_lossy(&buf[head_len..]).to_string();

    let (items, destino) = parse_body(&body);
    let _ = respond(stream, 200, &format!("{} accepted", items.len()));
    Some(Recibido::Enlaces(items, destino))
}

/// Valor de una cabecera, sin distinguir mayúsculas.
fn cabecera(head: &str, nombre: &str) -> Option<String> {
    let n = format!("{}:", nombre.to_ascii_lowercase());
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with(&n))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// El User-Agent se pinta en una página: hay que escaparlo aunque venga de la
/// máquina del propio usuario. Una cabecera es entrada externa, siempre.
fn escapar_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGINA_OK: &str = r#"<!DOCTYPE html><html lang="es"><head><meta charset="utf-8">
<title>Todo Downloader</title><style>
body{background:#12141c;color:#e6e8ef;font-family:system-ui,sans-serif;
display:flex;align-items:center;justify-content:center;height:100vh;margin:0}
div{max-width:640px;padding:32px;text-align:center}
h1{color:#4ade80;font-size:20px;margin:0 0 12px}
code{display:block;background:#1c2030;padding:12px;border-radius:8px;
font-size:12px;word-break:break-all;margin:16px 0;color:#9aa4bf}
p{color:#8b93a8;font-size:13px;margin:0}
</style></head><body><div>
<h1>&#10003; User-Agent detectado</h1>
<code>{UA}</code>
<p>Ya está guardado en Ajustes. Puedes cerrar esta pestaña.</p>
</div></body></html>"#;

const PAGINA_ERROR: &str = r#"<!DOCTYPE html><html lang="es"><head><meta charset="utf-8">
<title>Todo Downloader</title></head><body>
<p>Tu navegador no ha enviado ningun User-Agent. Escribelo a mano en Ajustes.</p>
</body></html>"#;

/// Respuesta HTML para la página que ve el usuario en su navegador.
fn responder_html(mut stream: TcpStream, body: &str) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Cache-Control: no-store\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn content_length(head: &str) -> Option<usize> {
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
}

/// Acepta JSON `{items:[{url,author,title,pageUrl,id}], mode:"select"|"queue"}`
/// o texto plano con una URL por línea.
fn parse_body(body: &str) -> (Vec<Incoming>, Destino) {
    let mut out = Vec::new();

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let destino = match v.get("mode").and_then(|x| x.as_str()) {
            Some("post") => Destino::Post,
            Some("select") => Destino::Seleccion,
            Some("queue") => Destino::Cola,
            _ => Destino::Auto,
        };
        if let Some(arr) = v.get("items").and_then(|x| x.as_array()) {
            for it in arr {
                let g = |k: &str| it.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let url = g("url");
                if !is_http(&url) {
                    continue;
                }
                // La miniatura también debe ser http(s); si no, se descarta
                let thumb = {
                    let t = g("thumb");
                    if is_http(&t) { t } else { String::new() }
                };
                out.push(Incoming {
                    url,
                    author: g("author"),
                    title: g("title"),
                    page_url: g("pageUrl"),
                    id: g("id"),
                    thumb,
                });
            }
            return (out, destino);
        }
    }

    // Texto plano
    for line in body.lines() {
        let l = line.trim();
        if is_http(l) {
            out.push(Incoming {
                url: l.to_string(),
                author: String::new(),
                title: String::new(),
                page_url: String::new(),
                id: String::new(),
                thumb: String::new(),
            });
        }
    }
    (out, Destino::Auto)
}

/// Enlaces aceptados: http(s) para descargas normales y `magnet:` para
/// torrents (el clic en un magnet del navegador llega por aquí cuando ya hay
/// una instancia abierta). Cualquier otro esquema se descarta.
fn is_http(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("magnet:")
}

fn respond(mut stream: TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        503 => "Service Unavailable",
        _ => "OK",
    };
    // CORS abierto: el receptor solo escucha en localhost y solo encola URLs.
    //
    // `Access-Control-Allow-Private-Network` es imprescindible: Chrome aplica
    // "Private Network Access", que bloquea las peticiones de una web pública
    // (tiktok.com) hacia una dirección de red local (127.0.0.1) salvo que el
    // servidor lo autorice explícitamente en el preflight. Sin esta cabecera el
    // script capturaba los enlaces pero no podía entregárselos a la app.
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Access-Control-Allow-Private-Network: true\r\n\
         Access-Control-Max-Age: 600\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}
