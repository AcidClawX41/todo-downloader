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

/// Arranca el receptor. `on_items` se invoca con cada lote recibido.
pub fn spawn<F>(port: u16, enabled: Arc<AtomicBool>, on_items: F)
where
    F: Fn(Vec<Incoming>) + Send + 'static,
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
                Some(items) if !items.is_empty() => on_items(items),
                _ => {}
            }
        }
    });
}

/// Lee la petición, responde y devuelve los enlaces si era un POST válido
fn handle(mut stream: TcpStream) -> Option<Vec<Incoming>> {
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

    let items = parse_body(&body);
    let _ = respond(stream, 200, &format!("{} accepted", items.len()));
    Some(items)
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

/// Acepta JSON `{items:[{url,author,title,pageUrl,id}]}` o texto plano con una URL por línea
fn parse_body(body: &str) -> Vec<Incoming> {
    let mut out = Vec::new();

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
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
            return out;
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
    out
}

fn is_http(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn respond(mut stream: TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        503 => "Service Unavailable",
        _ => "OK",
    };
    // CORS abierto: el receptor solo escucha en localhost y solo encola URLs
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()
}
