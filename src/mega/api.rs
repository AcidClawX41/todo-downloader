//! Cliente mínimo de la API pública de MEGA (`/cs`).
//!
//! Reutiliza el `reqwest::Client` de la aplicación: no abre un segundo pool de
//! conexiones ni una segunda pila TLS.
//!
//! La API acepta un array de comandos y responde con un array de resultados, o
//! con un número suelto si todo el lote falló. Confirmado en megalib
//! (`src/api`) y mega-rs (`src/http`).

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};

use super::error::{MegaError, Result};

const API_ORIGIN: &str = "https://g.api.mega.co.nz/cs";
/// Tope de la respuesta de metadatos. Sin esto, una respuesta hostil o rota
/// podría hacer crecer la memoria sin límite.
const MAX_META_BYTES: usize = 8 * 1024 * 1024;

/// Contador de idempotencia que MEGA espera en `?id=`
fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Envía un comando y devuelve su resultado.
///
/// `folder_handle` va en `?n=` y es obligatorio para operar dentro de una
/// carpeta pública.
pub async fn request(
    client: &reqwest::Client,
    command: Value,
    folder_handle: Option<&str>,
) -> Result<Value> {
    // `v=3` fija la versión del formato de respuesta. No es cosmético: el
    // parseo de nodos de carpeta está portado de megalib, que envía este
    // parámetro en todas sus peticiones (`src/api/client.rs:156`), así que sin
    // él MEGA podría devolver una forma distinta a la que espera el parser.
    let mut url = format!("{API_ORIGIN}?id={}&v=3", next_id());
    if let Some(n) = folder_handle {
        url.push_str("&n=");
        url.push_str(n);
    }

    let resp = client
        .post(&url)
        .json(&json!([command]))
        .send()
        .await
        .map_err(|e| MegaError::Http(e.to_string()))?;

    let status = resp.status();
    if status.as_u16() == 509 {
        return Err(MegaError::TransferQuotaExceeded);
    }
    if !status.is_success() {
        return Err(MegaError::Http(format!("HTTP {}", status.as_u16())));
    }

    let bytes = resp.bytes().await.map_err(|e| MegaError::Http(e.to_string()))?;
    if bytes.len() > MAX_META_BYTES {
        return Err(MegaError::MalformedResponse("respuesta demasiado grande"));
    }

    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|_| MegaError::MalformedResponse("no es JSON válido"))?;

    // Fallo global del lote: la API responde un número suelto
    if let Some(code) = v.as_i64() {
        return Err(MegaError::from_api_code(code));
    }
    let arr = v.as_array().ok_or(MegaError::MalformedResponse("se esperaba un array"))?;
    let first = arr.first().ok_or(MegaError::MalformedResponse("array vacío"))?;
    // Fallo del comando concreto
    if let Some(code) = first.as_i64() {
        return Err(MegaError::from_api_code(code));
    }
    Ok(first.clone())
}

/// Respuesta del comando `g`: metadatos y URL temporal de transferencia.
pub struct TransferInfo {
    pub size: u64,
    pub transfer_url: String,
    pub attrs_b64: String,
}

/// Pide los datos de descarga de un archivo público.
///
/// `g:1` pide la URL de transferencia. `p` es el handle de un enlace público
/// suelto; dentro de una carpeta pública el nodo va en `n` y el handle de la
/// carpeta en el query `?n=`.
///
/// `ssl:2` PIDE LA URL POR HTTPS, y no es opcional para esta aplicación.
///
/// Por defecto MEGA entrega las URL de almacenamiento en HTTP plano: como el
/// contenido ya va cifrado de extremo a extremo, para ellos el transporte es
/// indiferente. Para nosotros no: `SECURITY.md` promete que nada viaja en
/// claro, y sin este parámetro la descarga saldría por el puerto 80.
/// Referencia: mega-rs `src/lib.rs:1139`, que usa `ssl: 2` cuando el cliente
/// tiene HTTPS activado y `ssl: 0` cuando no.
pub async fn get_public_file(
    client: &reqwest::Client,
    handle: &str,
    folder: Option<&str>,
) -> Result<TransferInfo> {
    let command = if folder.is_some() {
        json!({ "a": "g", "g": 1, "ssl": 2, "n": handle })
    } else {
        json!({ "a": "g", "g": 1, "ssl": 2, "p": handle })
    };
    let r = request(client, command, folder).await?;

    // Algunos errores llegan dentro del objeto en vez de como número
    if let Some(code) = r.get("e").and_then(|e| e.as_i64()) {
        return Err(MegaError::from_api_code(code));
    }

    let size = r
        .get("s")
        .and_then(|v| v.as_u64())
        .ok_or(MegaError::MalformedResponse("falta el tamaño"))?;
    let transfer_url = r
        .get("g")
        .and_then(|v| v.as_str())
        .ok_or(MegaError::MalformedResponse("falta la URL de transferencia"))?
        .to_string();
    let attrs_b64 = r
        .get("at")
        .and_then(|v| v.as_str())
        .ok_or(MegaError::MalformedResponse("faltan los atributos"))?
        .to_string();

    Ok(TransferInfo { size, transfer_url: force_https(&transfer_url)?, attrs_b64 })
}

/// Garantiza que la transferencia salga por TLS.
///
/// Con `ssl:2` la URL ya debería llegar en HTTPS, pero MEGA no lo garantiza en
/// todos los nodos de almacenamiento. Rechazar sin más era demasiado estricto y
/// rompía descargas perfectamente válidas: los mismos servidores sirven TLS en
/// el mismo host, así que basta con reescribir el esquema — exactamente lo que
/// la aplicación ya hace con cualquier enlace `http://` que entra en la cola.
///
/// Lo que sí se rechaza es cualquier cosa que no sea HTTP(S): un esquema
/// inesperado en una respuesta de la API no se «arregla», se descarta.
fn force_https(url: &str) -> Result<String> {
    if url.starts_with("https://") {
        return Ok(url.to_string());
    }
    if let Some(rest) = url.strip_prefix("http://") {
        return Ok(format!("https://{rest}"));
    }
    Err(MegaError::MalformedResponse(
        "la URL de transferencia no es HTTP(S)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_https_promociona_http_y_respeta_https() {
        // El caso real que rompía: MEGA entrega la URL en HTTP plano
        assert_eq!(
            force_https("http://gfs270n123.userstorage.mega.co.nz/dl/abc").unwrap(),
            "https://gfs270n123.userstorage.mega.co.nz/dl/abc"
        );
        assert_eq!(
            force_https("https://gfs270n123.userstorage.mega.co.nz/dl/abc").unwrap(),
            "https://gfs270n123.userstorage.mega.co.nz/dl/abc"
        );
    }

    #[test]
    fn force_https_rechaza_esquemas_inesperados() {
        assert!(force_https("ftp://x/y").is_err());
        assert!(force_https("file:///etc/passwd").is_err());
        assert!(force_https("javascript:alert(1)").is_err());
        assert!(force_https("").is_err());
        assert!(force_https("//sin-esquema/x").is_err());
    }
}
