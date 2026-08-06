//! Descarga en streaming, reanudación y verificación de integridad.
//!
//! Se sigue la misma arquitectura que el motor HTTP nativo de la aplicación:
//! archivo `.part`, peticiones Range, cancelación comprobada entre lecturas,
//! progreso real y renombrado atómico al final.
//!
//! DIFERENCIA CLAVE: aquí el `.part` guarda TEXTO CLARO. El descifrado ocurre
//! sobre la marcha, trozo a trozo, así que la memoria usada no depende del
//! tamaño del archivo. Como AES-CTR es posicionable, la longitud del `.part`
//! es directamente el desplazamiento por el que hay que seguir.
//!
//! POR QUÉ EL MAC SE COMPRUEBA ANTES DE RENOMBRAR: CTR no autentica. Sin esa
//! pasada, un archivo corrupto o truncado aparecería como completado. El
//! nombre definitivo es una promesa de que el contenido es correcto.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::crypto::{mac_equals, CtrCipher, FileKey, MacHasher};
use super::error::{MegaError, Result};

/// Fase visible en la interfaz. Sin esto, la verificación de integridad de un
/// archivo grande parecería un cuelgue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MegaPhase {
    /// Pidiendo metadatos a la API: tamaño, nombre y URL de transferencia.
    /// También se vuelve aquí cuando caduca la URL y hay que renovarla.
    FetchingMetadata,
    Downloading,
    VerifyingIntegrity,
    Completed,
}

/// Archivo público ya resuelto y listo para descargar.
pub struct MegaFileInfo {
    pub handle: String,
    /// Handle de la carpeta pública, si viene de una
    pub folder: Option<String>,
    pub name: String,
    pub size: u64,
    pub key: FileKey,
    pub transfer_url: String,
}

impl std::fmt::Debug for MegaFileInfo {
    /// Sin clave y sin URL de transferencia: la URL lleva un token de acceso.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MegaFileInfo")
            .field("handle", &self.handle)
            .field("name", &self.name)
            .field("size", &self.size)
            .field("key", &"[REDACTED]")
            .field("transfer_url", &"[REDACTED]")
            .finish()
    }
}

/// Callbacks hacia la interfaz. Se pasan como referencias a `Fn` para poder
/// cruzarlas por un `await` sin pelearse con el préstamo mutable.
pub struct Callbacks<'a> {
    /// (bytes completados, velocidad en B/s)
    pub progress: &'a (dyn Fn(u64, f64) + Send + Sync),
    pub phase: &'a (dyn Fn(MegaPhase) + Send + Sync),
}

/// Cada cuánto se emite progreso. Igual que el motor HTTP nativo.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
/// Tamaño del buffer de la pasada de verificación.
const VERIFY_BUF: usize = 256 * 1024;

/// Descarga y descifra hasta completar el archivo, reanudando si hay `.part`.
///
/// No verifica ni renombra: de eso se encarga `finish_and_verify`, para que la
/// verificación pueda ejecutarse una sola vez aunque haya habido varios
/// intentos con URLs de transferencia distintas.
pub async fn stream_to_part(
    client: &reqwest::Client,
    info: &MegaFileInfo,
    part_path: &Path,
    cancel: &AtomicBool,
    cb: &Callbacks<'_>,
) -> Result<()> {
    let mut offset = tokio::fs::metadata(part_path).await.map(|m| m.len()).unwrap_or(0);

    // Un `.part` más largo que el archivo solo puede venir de un desastre
    // anterior: es más seguro empezar de cero que intentar recortarlo.
    if offset > info.size {
        let _ = tokio::fs::remove_file(part_path).await;
        offset = 0;
    }
    if offset == info.size && info.size > 0 {
        return Ok(()); // ya estaba entero; queda verificarlo
    }

    let mut req = client.get(&info.transfer_url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let resp = req.send().await.map_err(|e| MegaError::Http(e.to_string()))?;
    let status = resp.status().as_u16();

    match status {
        509 => return Err(MegaError::TransferQuotaExceeded),
        // MEGA caduca las URL de transferencia: hay que pedir otra, no rendirse
        403 | 404 | 410 => return Err(MegaError::ExpiredTransferUrl),
        429 => return Err(MegaError::RateLimited),
        500..=599 => return Err(MegaError::TemporaryUnavailable),
        _ => {}
    }

    if offset > 0 {
        // Si se pidió un rango y el servidor devuelve 200, está mandando el
        // archivo ENTERO. Añadirlo al `.part` lo corrompería en silencio.
        if status != 206 {
            let _ = tokio::fs::remove_file(part_path).await;
            return Err(MegaError::RangeNotSupported);
        }
        let ok = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains(&format!("bytes {offset}-")))
            .unwrap_or(false);
        if !ok {
            return Err(MegaError::InvalidContentRange);
        }
    } else if !(200..300).contains(&status) {
        return Err(MegaError::Http(format!("HTTP {status}")));
    }

    let mut file = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part_path)
            .await
            .map_err(|e| MegaError::Io(e.to_string()))?
    } else {
        if let Some(dir) = part_path.parent() {
            let _ = tokio::fs::create_dir_all(dir).await;
        }
        tokio::fs::File::create(part_path)
            .await
            .map_err(|e| MegaError::Io(e.to_string()))?
    };

    let cipher = CtrCipher::new(&info.key);
    let mut written = offset;
    let start = Instant::now();
    let session_start = offset;
    let mut last_emit = Instant::now();

    (cb.phase)(MegaPhase::Downloading);

    let mut resp = resp;
    loop {
        // Cancelación comprobada entre lecturas: pausar debe notarse enseguida
        if cancel.load(Ordering::Relaxed) {
            file.flush().await.ok();
            return Err(MegaError::Cancelled);
        }
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                file.flush().await.ok();
                return Err(MegaError::Http(e.to_string()));
            }
        };
        if chunk.is_empty() {
            continue;
        }
        // Descifrar en el sitio. El desplazamiento es el del archivo completo,
        // no el de esta petición: el contador CTR depende de la posición real.
        let mut buf = chunk.to_vec();
        cipher.apply(&mut buf, written);

        file.write_all(&buf).await.map_err(|e| MegaError::Io(e.to_string()))?;
        written += buf.len() as u64;

        if last_emit.elapsed() >= PROGRESS_INTERVAL {
            let secs = start.elapsed().as_secs_f64().max(0.001);
            (cb.progress)(written, (written - session_start) as f64 / secs);
            last_emit = Instant::now();
        }
    }

    file.flush().await.map_err(|e| MegaError::Io(e.to_string()))?;
    drop(file);
    (cb.progress)(written, 0.0);

    // Una respuesta truncada deja el `.part` corto. Se conserva para reanudar.
    if written < info.size {
        return Err(MegaError::Http(format!(
            "respuesta truncada en {written} de {} bytes",
            info.size
        )));
    }
    if written > info.size {
        return Err(MegaError::SizeMismatch { expected: info.size, got: written });
    }
    Ok(())
}

/// Verifica el MAC del archivo completo y solo entonces lo renombra.
///
/// Se elige releer el `.part` entero en vez de persistir el estado del MAC
/// entre reanudaciones. Cuesta unos segundos de disco en archivos grandes y a
/// cambio no hay ningún estado intermedio que pueda quedar mal guardado y dar
/// por bueno un archivo corrupto.
pub async fn finish_and_verify(
    info: &MegaFileInfo,
    part_path: &Path,
    final_path: &Path,
    cancel: &AtomicBool,
    cb: &Callbacks<'_>,
) -> Result<()> {
    (cb.phase)(MegaPhase::VerifyingIntegrity);

    let len = tokio::fs::metadata(part_path)
        .await
        .map(|m| m.len())
        .map_err(|e| MegaError::Io(e.to_string()))?;
    if len != info.size {
        return Err(MegaError::SizeMismatch { expected: info.size, got: len });
    }

    let mut f = tokio::fs::File::open(part_path)
        .await
        .map_err(|e| MegaError::Io(e.to_string()))?;
    let mut hasher = MacHasher::new(&info.key);
    let mut buf = vec![0u8; VERIFY_BUF];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(MegaError::Cancelled);
        }
        let n = f.read(&mut buf).await.map_err(|e| MegaError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    drop(f);

    if !mac_equals(&hasher.finalize(), &info.key.mac) {
        // El archivo NO puede aparecer con su nombre definitivo. Se marca para
        // que el usuario vea que existe y por qué no sirve.
        let bad = part_path.with_extension("part.corrupt");
        let _ = tokio::fs::rename(part_path, &bad).await;
        return Err(MegaError::IntegrityMismatch);
    }

    tokio::fs::rename(part_path, final_path)
        .await
        .map_err(|e| MegaError::Io(e.to_string()))?;
    (cb.phase)(MegaPhase::Completed);
    Ok(())
}
