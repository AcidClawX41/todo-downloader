//! Carpetas públicas: enumeración de nodos sin iniciar sesión.
//!
//! Derivado de megalib 0.11.1 `src/public.rs::open_folder`,
//! `parse_public_node` y `decrypt_public_node_key`.
//!
//! El esquema: la clave del enlace de carpeta son 16 bytes (no 32 como en los
//! archivos sueltos). Con ella se descifra, por AES-128-ECB, la clave de cada
//! nodo que devuelve el comando `f`. Cada nodo trae después sus atributos
//! cifrados con su propia clave.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::json;

use super::crypto;
use super::error::{MegaError, Result};
use super::link::MegaFolderLink;

/// Tope de nodos que se aceptan de una respuesta. Una carpeta hostil o rota no
/// debe poder hacer crecer la memoria sin límite.
const MAX_NODES: usize = 100_000;

/// Un elemento de la carpeta, ya descifrado.
#[derive(Debug, Clone)]
pub struct MegaFolderEntry {
    pub handle: String,
    pub parent: Option<String>,
    /// Nombre saneado del propio nodo
    pub name: String,
    /// Ruta relativa completa dentro de la carpeta, ya saneada componente a
    /// componente. Nunca sale del directorio raíz.
    pub relative_path: PathBuf,
    pub size: u64,
    pub is_folder: bool,
    /// Clave del nodo. Para archivos son 32 bytes: los que necesita `FileKey`.
    key: Vec<u8>,
}

impl MegaFolderEntry {
    /// Clave del archivo en base64url, en el formato que espera `resolve_file`.
    pub fn key_b64(&self) -> String {
        crypto::base64url_encode(&self.key)
    }
}

/// Sanea un componente de ruta. Se rechaza todo lo que pueda escaparse del
/// directorio de destino: `..`, separadores, rutas absolutas y letras de
/// unidad. Un nombre que venga cifrado desde MEGA no es de fiar.
fn safe_component(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." {
        return None;
    }
    if n.contains('/') || n.contains('\\') || n.contains('\0') {
        return None;
    }
    // "C:" o similar
    if n.len() >= 2 && n.as_bytes()[1] == b':' {
        return None;
    }
    let cleaned: String = n
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .filter(|c| !c.is_control())
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.chars().take(120).collect())
}

/// Cuánto se considera fresco un listado de carpeta.
const CACHE_TTL: Duration = Duration::from_secs(600);

type Cache = tokio::sync::Mutex<HashMap<String, (Instant, Arc<Vec<MegaFolderEntry>>)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

/// Listado de carpeta con caché.
///
/// ESTO NO ES UNA OPTIMIZACIÓN, ES UN REQUISITO. Cada archivo de una carpeta
/// necesita su clave de nodo, que solo viene en el listado. Sin caché, encolar
/// una carpeta de 107 archivos lanzaba 107 peticiones `a:f` idénticas en pocos
/// segundos: MEGA lo corta por límite de ritmo y fallan TODAS las descargas.
/// Con caché es una sola petición para toda la carpeta.
///
/// El candado se mantiene durante la petición a propósito: si diez tareas
/// arrancan a la vez y todas fallan la caché, solo una consulta y las demás
/// esperan a su resultado en vez de repetir la llamada.
pub async fn list_cached(
    client: &reqwest::Client,
    link: &MegaFolderLink,
) -> Result<Arc<Vec<MegaFolderEntry>>> {
    let mut map = cache().lock().await;

    if let Some((when, entries)) = map.get(&link.handle) {
        if when.elapsed() < CACHE_TTL {
            return Ok(entries.clone());
        }
    }

    let fresh = Arc::new(list(client, link).await?);
    map.insert(link.handle.clone(), (Instant::now(), fresh.clone()));

    // Poda perezosa: sin esto, una sesión larga acumularía listados caducados
    map.retain(|_, (when, _)| when.elapsed() < CACHE_TTL);
    Ok(fresh)
}

/// Lista el contenido de una carpeta pública.
///
/// Devuelve los nodos con su ruta relativa ya construida y saneada.
pub async fn list(
    client: &reqwest::Client,
    link: &MegaFolderLink,
) -> Result<Vec<MegaFolderEntry>> {
    let raw = crypto::base64url_decode(&link.key_b64)?;
    if raw.len() != 16 {
        return Err(MegaError::InvalidKey);
    }
    let mut folder_key = [0u8; 16];
    folder_key.copy_from_slice(&raw);

    let resp = super::api::request(
        client,
        json!({ "a": "f", "c": 1, "r": 1 }),
        Some(&link.handle),
    )
    .await?;

    let nodes = resp
        .get("f")
        .and_then(|v| v.as_array())
        .ok_or(MegaError::MalformedResponse("la carpeta no trae nodos"))?;

    if nodes.len() > MAX_NODES {
        return Err(MegaError::MalformedResponse("demasiados nodos"));
    }

    // El primer nodo es la raíz: su clave compartida es la del enlace
    let mut share_keys: HashMap<String, [u8; 16]> = HashMap::new();
    if let Some(h) = nodes.first().and_then(|n| n.get("h")).and_then(|v| v.as_str()) {
        share_keys.insert(h.to_string(), folder_key);
    }

    let mut out: Vec<MegaFolderEntry> = Vec::new();
    let mut root_handle: Option<String> = None;

    for (idx, n) in nodes.iter().enumerate() {
        let Some(handle) = n.get("h").and_then(|v| v.as_str()) else { continue };
        let Some(t) = n.get("t").and_then(|v| v.as_i64()) else { continue };
        // t: 0 = archivo, 1 = carpeta, 2 = raíz. Lo demás no nos interesa.
        if t > 2 {
            continue;
        }
        let is_root = idx == 0;
        if is_root {
            root_handle = Some(handle.to_string());
        }

        let node_key: Vec<u8> = if is_root {
            folder_key.to_vec()
        } else {
            let k = n.get("k").and_then(|v| v.as_str()).unwrap_or("");
            match decrypt_node_key(k, &folder_key, &share_keys) {
                Some(k) => k,
                // Sin `k` utilizable se usa la clave de la carpeta, que es lo
                // habitual en carpetas públicas planas
                None => folder_key.to_vec(),
            }
        };

        let name = n
            .get("a")
            .and_then(|v| v.as_str())
            .and_then(|a| {
                crypto::node_attr_key(&node_key)
                    .and_then(|k| crypto::decrypt_attributes_with(a, &k).ok())
            })
            .unwrap_or_else(|| if is_root { "MEGA".into() } else { handle.to_string() });

        let Some(safe) = safe_component(&name) else {
            // Un nombre que no se puede sanear se descarta: mejor perder un
            // archivo que escribir fuera de la carpeta de destino.
            continue;
        };

        out.push(MegaFolderEntry {
            handle: handle.to_string(),
            parent: n.get("p").and_then(|v| v.as_str()).map(|s| s.to_string()),
            name: safe,
            relative_path: PathBuf::new(),
            size: n.get("s").and_then(|v| v.as_u64()).unwrap_or(0),
            is_folder: t != 0,
            key: node_key,
        });
    }

    build_paths(&mut out, root_handle.as_deref());
    Ok(out)
}

/// Descifra la clave de un nodo. `k` tiene la forma `handle:claveBase64`,
/// posiblemente varias separadas por `/`.
fn decrypt_node_key(
    k: &str,
    folder_key: &[u8; 16],
    share_keys: &HashMap<String, [u8; 16]>,
) -> Option<Vec<u8>> {
    for part in k.split('/') {
        let (key_handle, enc) = part.split_once(':')?;
        let dec_key = share_keys.get(key_handle).unwrap_or(folder_key);
        if let Ok(bytes) = crypto::base64url_decode(enc) {
            if bytes.len() >= 16 {
                return Some(crypto::aes128_ecb_decrypt(&bytes, dec_key));
            }
        }
    }
    None
}

/// Reconstruye la ruta relativa de cada nodo siguiendo la cadena de padres.
///
/// La profundidad está acotada: una respuesta con un ciclo de padres colgaría
/// el bucle si no lo estuviera.
fn build_paths(entries: &mut [MegaFolderEntry], root: Option<&str>) {
    const MAX_DEPTH: usize = 32;

    let by_handle: HashMap<String, (Option<String>, String)> = entries
        .iter()
        .map(|e| (e.handle.clone(), (e.parent.clone(), e.name.clone())))
        .collect();

    for e in entries.iter_mut() {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = e.parent.clone();
        let mut depth = 0;
        while let Some(h) = cur {
            if Some(h.as_str()) == root || depth >= MAX_DEPTH {
                break;
            }
            match by_handle.get(&h) {
                Some((p, name)) => {
                    parts.push(name.clone());
                    cur = p.clone();
                }
                None => break,
            }
            depth += 1;
        }
        parts.reverse();
        let mut path = PathBuf::new();
        for p in parts {
            path.push(p);
        }
        path.push(&e.name);
        e.relative_path = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_component_bloquea_travesias() {
        assert_eq!(safe_component(".."), None);
        assert_eq!(safe_component("."), None);
        assert_eq!(safe_component("a/b"), None);
        assert_eq!(safe_component("a\\b"), None);
        assert_eq!(safe_component("C:algo"), None);
        assert_eq!(safe_component(""), None);
        assert_eq!(safe_component("   "), None);
        assert_eq!(safe_component("con\0trol"), None);
    }

    #[test]
    fn safe_component_limpia_sin_romper_nombres_normales() {
        assert_eq!(safe_component("video.mp4").as_deref(), Some("video.mp4"));
        assert_eq!(safe_component(" foto .jpg ").as_deref(), Some("foto .jpg"));
        assert_eq!(safe_component("a<b>c").as_deref(), Some("abc"));
        assert_eq!(safe_component("nombre.").as_deref(), Some("nombre"));
        // Nombre absurdamente largo se recorta, no se rechaza
        let largo = "x".repeat(400);
        assert_eq!(safe_component(&largo).unwrap().len(), 120);
    }

    fn entry(h: &str, p: Option<&str>, name: &str, folder: bool) -> MegaFolderEntry {
        MegaFolderEntry {
            handle: h.into(),
            parent: p.map(|s| s.into()),
            name: name.into(),
            relative_path: PathBuf::new(),
            size: 0,
            is_folder: folder,
            key: vec![0u8; 32],
        }
    }

    #[test]
    fn reconstruye_rutas_anidadas() {
        let mut v = vec![
            entry("ROOT", None, "raiz", true),
            entry("SUB", Some("ROOT"), "sub", true),
            entry("FILE", Some("SUB"), "a.mp4", false),
        ];
        build_paths(&mut v, Some("ROOT"));
        assert_eq!(v[2].relative_path, PathBuf::from("sub").join("a.mp4"));
        assert_eq!(v[1].relative_path, PathBuf::from("sub"));
    }

    #[test]
    fn un_ciclo_de_padres_no_cuelga() {
        // Respuesta hostil: A es padre de B y B de A
        let mut v = vec![
            entry("A", Some("B"), "a", true),
            entry("B", Some("A"), "b", true),
        ];
        build_paths(&mut v, None);
        assert!(v[0].relative_path.components().count() <= 33);
    }
}
