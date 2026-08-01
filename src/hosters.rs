//! Resolvers nativos para hosters de archivos con API abierta (grupo 2) — By Eric V. Gramunt
//!
//! Convierten una URL de PÁGINA de un hoster (que no es un enlace directo) en
//! uno o varios enlaces directos a CDN. Esos enlaces los descarga después el
//! motor HTTP nativo de la app, que ya tiene reanudación por Range, reintentos
//! y referer por dominio.
//!
//! Se implementan en Rust puro (reqwest) — sin binarios externos ni Python —
//! porque estos hosters no cifran en cliente ni ponen captchas: basta con
//! pedirle a su API el enlace real. Cada resolver es defensivo: ante cualquier
//! cambio de formato devuelve un error claro en vez de romperse en silencio.
//!
//! Cobertura: Pixeldrain (archivos y listas), GoFile (carpetas y archivos),
//! MediaFire (archivos). Sitios que se defienden activamente de los scrapers
//! (Bunkr, Cyberdrop…) NO se resuelven aquí: para esos existe el motor
//! opcional cyberdrop-dl.

use regex::Regex;

/// Un enlace directo ya resuelto, listo para el motor HTTP.
pub struct Resolved {
    /// URL directa al archivo en el CDN
    pub url: String,
    /// Nombre de archivo sugerido (ya saneado por quien lo consume)
    pub filename: String,
    /// Cookie necesaria para la descarga (GoFile la exige); vacío si no aplica
    pub cookie: Option<String>,
}

/// ¿Es un hoster que resolvemos de forma nativa en Rust?
pub fn is_filehost(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("pixeldrain.com")
        || u.contains("gofile.io")
        || u.contains("mediafire.com")
}

/// Nombre legible del hoster, para mensajes
pub fn host_name(url: &str) -> &'static str {
    let u = url.to_ascii_lowercase();
    if u.contains("pixeldrain.com") {
        "Pixeldrain"
    } else if u.contains("gofile.io") {
        "GoFile"
    } else if u.contains("mediafire.com") {
        "MediaFire"
    } else {
        "hoster"
    }
}

/// Resuelve una URL de hoster a sus enlaces directos.
pub async fn resolve(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Resolved>> {
    let u = url.to_ascii_lowercase();
    let items = if u.contains("pixeldrain.com") {
        pixeldrain(client, url).await?
    } else if u.contains("gofile.io") {
        gofile(client, url).await?
    } else if u.contains("mediafire.com") {
        mediafire(client, url).await?
    } else {
        anyhow::bail!("hoster no soportado de forma nativa");
    };
    if items.is_empty() {
        anyhow::bail!("no se encontró ningún archivo descargable en el enlace");
    }
    Ok(items)
}

// ============================= Pixeldrain =============================
//
// API pública y estable, sin autenticación.
//   Archivo:  pixeldrain.com/u/{id}        → GET /api/file/{id}/info  → {name}
//             descarga directa:               /api/file/{id}?download
//   Lista:    pixeldrain.com/l/{id}        → GET /api/list/{id}       → {files:[{id,name}]}

#[derive(serde::Deserialize)]
struct PdInfo {
    name: Option<String>,
}
#[derive(serde::Deserialize)]
struct PdListItem {
    id: String,
    name: Option<String>,
}
#[derive(serde::Deserialize)]
struct PdList {
    files: Option<Vec<PdListItem>>,
}

fn pd_direct(id: &str) -> String {
    format!("https://pixeldrain.com/api/file/{id}?download")
}

async fn pixeldrain(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Resolved>> {
    // ¿Lista (/l/) o archivo (/u/, /api/file/)?
    if let Some(id) = capture(r"pixeldrain\.com/l/([a-zA-Z0-9]+)", url) {
        let api = format!("https://pixeldrain.com/api/list/{id}");
        let list: PdList = client.get(&api).send().await?.json().await?;
        let files = list.files.unwrap_or_default();
        let mut out = Vec::new();
        for (n, f) in files.iter().enumerate() {
            out.push(Resolved {
                url: pd_direct(&f.id),
                filename: f.name.clone().unwrap_or_else(|| format!("pixeldrain_{n}")),
                cookie: None,
            });
        }
        return Ok(out);
    }

    let id = capture(r"pixeldrain\.com/(?:u|api/file)/([a-zA-Z0-9]+)", url)
        .ok_or_else(|| anyhow::anyhow!("no se reconoció el ID de Pixeldrain"))?;
    let info: PdInfo = client
        .get(format!("https://pixeldrain.com/api/file/{id}/info"))
        .send()
        .await?
        .json()
        .await
        .unwrap_or(PdInfo { name: None });
    Ok(vec![Resolved {
        url: pd_direct(&id),
        filename: info.name.unwrap_or_else(|| format!("pixeldrain_{id}")),
        cookie: None,
    }])
}

// ============================= GoFile =============================
//
// Requiere un token de invitado y un "website token" (wt) que va incrustado en
// su global.js. La descarga necesita mandar el token como cookie.
//   POST /accounts                          → {data:{token}}
//   GET  /contents/{code}?wt={wt}  (Bearer) → {data:{children:{..:{name,link}}}}

#[derive(serde::Deserialize)]
struct GfAccounts {
    data: Option<GfToken>,
}
#[derive(serde::Deserialize)]
struct GfToken {
    token: Option<String>,
}

async fn gofile_token(client: &reqwest::Client) -> anyhow::Result<String> {
    let r: GfAccounts = client
        .post("https://api.gofile.io/accounts")
        .send()
        .await?
        .json()
        .await?;
    r.data
        .and_then(|d| d.token)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| anyhow::anyhow!("GoFile no devolvió token de invitado"))
}

/// El website token cambia; se extrae de global.js. Si falla, se usa el último
/// valor conocido como respaldo.
async fn gofile_wt(client: &reqwest::Client) -> String {
    const FALLBACK: &str = "4fd6sg89d7s6";
    let Ok(js) = client
        .get("https://gofile.io/dist/js/global.js")
        .send()
        .await
    else {
        return FALLBACK.into();
    };
    let Ok(body) = js.text().await else { return FALLBACK.into() };
    // Formatos vistos:  wt: "xxxx"  |  appdata.wt = "xxxx"
    for pat in [r#"wt\s*[:=]\s*"([a-zA-Z0-9]{6,})""#, r#"\.wt\s*=\s*"([a-zA-Z0-9]{6,})""#] {
        if let Some(v) = capture(pat, &body) {
            return v;
        }
    }
    FALLBACK.into()
}

async fn gofile(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Resolved>> {
    let code = capture(r"gofile\.io/d/([a-zA-Z0-9]+)", url)
        .ok_or_else(|| anyhow::anyhow!("no se reconoció el código de GoFile"))?;

    let token = gofile_token(client).await?;
    let wt = gofile_wt(client).await;
    let cookie = format!("accountToken={token}");

    let api = format!("https://api.gofile.io/contents/{code}?wt={wt}");
    let resp = client
        .get(&api)
        .bearer_auth(&token)
        .send()
        .await?;
    let v: serde_json::Value = resp.json().await?;

    if v.get("status").and_then(|s| s.as_str()) != Some("ok") {
        let st = v.get("status").and_then(|s| s.as_str()).unwrap_or("desconocido");
        // Caso típico reciente: listar carpetas de invitado exige premium
        if st.contains("notPremium") {
            anyhow::bail!("GoFile exige cuenta premium para listar esta carpeta");
        }
        anyhow::bail!("GoFile respondió: {st}");
    }

    // children es un objeto {id: {type, name, link}}
    let mut out = Vec::new();
    let empty = serde_json::Map::new();
    let children = v
        .pointer("/data/children")
        .and_then(|c| c.as_object())
        .unwrap_or(&empty);
    for child in children.values() {
        if child.get("type").and_then(|t| t.as_str()) != Some("file") {
            continue;
        }
        let Some(link) = child.get("link").and_then(|l| l.as_str()) else { continue };
        let name = child
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("gofile")
            .to_string();
        out.push(Resolved {
            url: link.to_string(),
            filename: name,
            cookie: Some(cookie.clone()),
        });
    }
    Ok(out)
}

// ============================= MediaFire =============================
//
// Sin API: el enlace directo está en el HTML de la página de descarga, ya sea
// en el href del botón o en un atributo "scrambled" codificado en base64.

async fn mediafire(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Resolved>> {
    let html = client.get(url).send().await?.text().await?;

    // 1) href directo del botón de descarga
    let mut direct = capture(
        r#"href="(https://download[0-9]+\.mediafire\.com/[^"]+)""#,
        &html,
    );

    // 2) atributo scrambled (base64 del enlace real)
    if direct.is_none() {
        if let Some(b64) = capture(r#"data-scrambled-url="([^"]+)""#, &html) {
            use base64_lite::decode;
            if let Some(dec) = decode(&b64) {
                if dec.starts_with("http") {
                    direct = Some(dec);
                }
            }
        }
    }

    let direct = direct.ok_or_else(|| {
        anyhow::anyhow!("no se encontró el enlace directo en la página de MediaFire")
    })?;

    // Nombre: de la URL de la página (.../file/{id}/{name}) o del propio enlace
    let filename = capture(r"mediafire\.com/file/[^/]+/([^/?#]+)", url)
        .or_else(|| {
            direct
                .split(['?', '#'])
                .next()
                .and_then(|s| s.rsplit('/').next())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "mediafire".into());

    Ok(vec![Resolved {
        url: direct,
        filename: urldecode(&filename),
        cookie: None,
    }])
}

// ============================= utilidades =============================

/// Primer grupo de captura de `pat` sobre `text`, si coincide.
fn capture(pat: &str, text: &str) -> Option<String> {
    Regex::new(pat).ok()?.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

/// Decodifica los %XX de un nombre extraído de una URL (suficiente para nombres)
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Base64 mínimo (solo decode estándar), para el enlace scrambled de MediaFire.
/// Evita añadir una dependencia solo para esto.
mod base64_lite {
    pub fn decode(input: &str) -> Option<String> {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut lut = [255u8; 256];
        for (i, &c) in T.iter().enumerate() {
            lut[c as usize] = i as u8;
        }
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0;
        for &c in input.trim().as_bytes() {
            if c == b'=' {
                break;
            }
            let v = lut[c as usize];
            if v == 255 {
                continue; // ignora saltos de línea u otros
            }
            buf = (buf << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        String::from_utf8(out).ok()
    }
}
