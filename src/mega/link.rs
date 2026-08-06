//! Análisis y canonicalización de enlaces públicos de MEGA.
//!
//! Se parsea la URL de verdad, con host exacto. Nada de `contains("mega.nz")`:
//! así es como `mega.nz.atacante.example` acaba tratado como MEGA. (megalib
//! 0.11.1 usa `url.contains("/file/")` en `parse_mega_link`; aquí no.)

use super::error::{MegaError, Result};

/// Hosts que sirven enlaces públicos de MEGA. Coincidencia exacta o subdominio.
const MEGA_HOSTS: &[&str] = &["mega.nz", "mega.co.nz", "mega.io"];

/// Longitud máxima aceptable de handle y clave. Los handles reales son de 8
/// caracteres y las claves de 43 (32 bytes en base64url sin relleno); el margen
/// es para variantes, no para aceptar cualquier cosa.
const MAX_HANDLE: usize = 32;
const MAX_KEY: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegaFileLink {
    pub handle: String,
    /// Clave en base64url. Secreto: nunca se imprime.
    pub key_b64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MegaFolderLink {
    pub handle: String,
    pub key_b64: String,
    /// Nodo concreto dentro de la carpeta (`/folder/H#K/file/NODE`)
    pub node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MegaLink {
    File(MegaFileLink),
    Folder(MegaFolderLink),
}

impl MegaLink {
    /// Forma canónica para deduplicar la cola. Los formatos moderno y antiguo
    /// del mismo archivo producen la misma cadena, así que no se encolan dos
    /// veces. Incluye la clave porque sin ella el enlace no sirve de nada.
    pub fn canonical(&self) -> String {
        match self {
            MegaLink::File(f) => format!("https://mega.nz/file/{}#{}", f.handle, f.key_b64),
            MegaLink::Folder(d) => match &d.node {
                Some(n) => format!("https://mega.nz/folder/{}#{}/file/{}", d.handle, d.key_b64, n),
                None => format!("https://mega.nz/folder/{}#{}", d.handle, d.key_b64),
            },
        }
    }

    /// Forma segura para logs, errores y diagnósticos: sin clave.
    pub fn redacted(&self) -> String {
        match self {
            MegaLink::File(f) => format!("https://mega.nz/file/{}#[REDACTED]", f.handle),
            MegaLink::Folder(d) => format!("https://mega.nz/folder/{}#[REDACTED]", d.handle),
        }
    }

    pub fn handle(&self) -> &str {
        match self {
            MegaLink::File(f) => &f.handle,
            MegaLink::Folder(d) => &d.handle,
        }
    }
}

/// ¿El host es MEGA? Comparación estructural: exacto o subdominio real.
fn is_mega_host(host: &str) -> bool {
    MEGA_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Caracteres admisibles en un handle o en una clave base64url.
fn is_b64url(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '=')
}

/// ¿Es una URL de MEGA, aunque no sepamos aún si es válida? Sirve para enrutar
/// sin intentar parsear a fondo.
pub fn is_mega_url(url: &str) -> bool {
    split_url(url).map(|(h, _, _)| is_mega_host(&h)).unwrap_or(false)
}

/// Descompone en (host, ruta, fragmento) validando el esquema.
fn split_url(url: &str) -> Option<(String, String, String)> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;

    let (authority, after) = match rest.find(['/', '#', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    // userinfo y puerto fuera
    let authority = authority.rsplit('@').next()?;
    let host = authority.split(':').next()?;
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }

    let (path_and_query, fragment) = match after.find('#') {
        Some(i) => (&after[..i], &after[i + 1..]),
        None => (after, ""),
    };
    let path = path_and_query.split('?').next().unwrap_or("");
    Some((host, path.to_string(), fragment.to_string()))
}

/// Parsea un enlace público de MEGA.
///
/// Formatos admitidos, ambos verificados contra implementaciones reales:
///   moderno: https://mega.nz/file/HANDLE#KEY
///            https://mega.nz/folder/HANDLE#KEY[/file/NODE]
///   antiguo: https://mega.nz/#!HANDLE!KEY
///            https://mega.nz/#F!HANDLE!KEY
pub fn parse(url: &str) -> Result<MegaLink> {
    let (host, path, fragment) = split_url(url).ok_or(MegaError::InvalidUrl("esquema o host"))?;

    if !is_mega_host(&host) {
        return Err(MegaError::InvalidUrl("el host no es de MEGA"));
    }

    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // ---- Formato moderno: /file/H#K  y  /folder/H#K[/file/N] ----
    if let Some(kind) = segs.first() {
        if *kind == "file" || *kind == "folder" {
            let handle = segs.get(1).copied().unwrap_or("");
            if handle.is_empty() {
                return Err(MegaError::InvalidUrl("falta el identificador"));
            }
            if fragment.is_empty() {
                return Err(MegaError::MissingKey);
            }
            let is_folder = *kind == "folder";
            // Solo las carpetas admiten sufijo en el fragmento: KEY/file/NODE.
            // En un enlace de archivo, cualquier '/' sobra — y aceptarlo
            // descartando lo que venga detrás dejaba pasar basura en silencio.
            let (key, node) = match fragment.split_once('/') {
                Some(_) if !is_folder => {
                    return Err(MegaError::InvalidKey);
                }
                Some((k, rest)) => match rest.strip_prefix("file/") {
                    Some(n) => (k, Some(n.to_string())),
                    // Sufijo desconocido: mejor rechazar que ignorarlo
                    None => return Err(MegaError::UnsupportedLinkType),
                },
                None => (fragment.as_str(), None),
            };
            return build(is_folder, handle, key, node);
        }
    }

    // ---- Formato antiguo: /#!H!K  y  /#F!H!K ----
    if !fragment.is_empty() {
        let (is_folder, body) = if let Some(b) = fragment.strip_prefix("F!") {
            (true, b)
        } else if let Some(b) = fragment.strip_prefix('!') {
            (false, b)
        } else {
            return Err(MegaError::UnsupportedLinkType);
        };
        let mut parts = body.splitn(3, '!');
        let handle = parts.next().unwrap_or("");
        let key = parts.next().unwrap_or("");
        let node = parts.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
        if handle.is_empty() {
            return Err(MegaError::InvalidUrl("falta el identificador"));
        }
        if key.is_empty() {
            return Err(MegaError::MissingKey);
        }
        return build(is_folder, handle, key, node);
    }

    Err(MegaError::UnsupportedLinkType)
}

fn build(is_folder: bool, handle: &str, key: &str, node: Option<String>) -> Result<MegaLink> {
    if handle.len() > MAX_HANDLE || !is_b64url(handle) {
        return Err(MegaError::InvalidUrl("identificador con formato inesperado"));
    }
    if key.is_empty() {
        return Err(MegaError::MissingKey);
    }
    if key.len() > MAX_KEY || !is_b64url(key) {
        return Err(MegaError::InvalidKey);
    }
    if let Some(n) = &node {
        if n.len() > MAX_HANDLE || !is_b64url(n) {
            return Err(MegaError::InvalidUrl("nodo con formato inesperado"));
        }
    }
    if is_folder {
        Ok(MegaLink::Folder(MegaFolderLink {
            handle: handle.to_string(),
            key_b64: key.to_string(),
            node,
        }))
    } else {
        Ok(MegaLink::File(MegaFileLink {
            handle: handle.to_string(),
            key_b64: key.to_string(),
        }))
    }
}
