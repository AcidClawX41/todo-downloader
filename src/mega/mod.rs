//! Motor nativo de MEGA.nz para ENLACES PÚBLICOS.
//!
//! Fuera de alcance a propósito: inicio de sesión, 2FA, navegación de la nube
//! personal, subidas y persistencia de sesión. Este motor no pide credenciales
//! y no guarda ninguna.
//!
//! Flujo del protocolo implementado:
//!
//! 1. Parsear el enlace y sacar handle y clave del FRAGMENTO (`#...`).
//! 2. `POST /cs` con `{"a":"g","g":1,"p":handle}` → tamaño, URL temporal de
//!    transferencia y atributos cifrados.
//! 3. Desempaquetar la clave de 32 bytes en clave AES + IV + MAC esperado.
//! 4. Descifrar los atributos (AES-128-CBC, IV cero) y validar el prefijo
//!    «MEGA» antes de fiarse del JSON.
//! 5. Descargar el texto cifrado y descifrarlo con AES-128-CTR sobre la marcha.
//! 6. Recalcular el MAC condensado del archivo completo y compararlo.
//! 7. Solo entonces, renombrar el `.part` al nombre definitivo.
//!
//! La clave nunca se envía a MEGA ni aparece en logs, errores o diagnósticos.

pub mod api;
pub mod crypto;
pub mod download;
pub mod error;
pub mod folder;
pub mod link;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub use download::{Callbacks, MegaFileInfo, MegaPhase};
pub use error::{MegaError, Result};
pub use link::{is_mega_url, parse, MegaFileLink, MegaLink};

/// Canonicaliza una URL de MEGA. Devuelve `None` si no es de MEGA o no es
/// válida, para que quien llame la deje pasar sin tocar.
///
/// Se aplica ANTES de deduplicar y de enrutar: así el formato moderno y el
/// antiguo del mismo archivo no producen dos filas en la cola.
pub fn canonicalize(url: &str) -> Option<String> {
    if !is_mega_url(url) {
        return None;
    }
    link::parse(url).ok().map(|l| l.canonical())
}

/// Máximo de ciclos «pedir URL de transferencia nueva y reanudar» por archivo.
/// Acotado a propósito: una URL caducada se renueva, pero un enlace muerto no
/// debe dejar la aplicación reintentando para siempre.
const MAX_TRANSFER_REFRESH: u32 = 3;
/// Espera base del backoff exponencial entre reintentos.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(800);

/// Espaciado mínimo entre peticiones a la API de MEGA.
///
/// Corto a propósito. El comando `g` es una petición barata y el listado de
/// carpeta está cacheado, así que el cuello de botella real debe ser el
/// semáforo de concurrencia de la aplicación, no una cola global.
const MEGA_MIN_GAP: Duration = Duration::from_millis(250);

/// Separa en el tiempo las peticiones a la API sin serializar la aplicación.
///
/// El candado se suelta ANTES de dormir: así diez tareas se escalonan 250 ms
/// cada una en vez de esperar todas a que la anterior termine su siesta con el
/// candado en la mano, que es lo que hace el `throttle()` global y lo que
/// convertía una carpeta grande en varios minutos de aparente cuelgue.
pub async fn gate() {
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    static G: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();

    let wait = {
        let g = G.get_or_init(|| Mutex::new(None));
        let mut last = g.lock().await;
        let now = std::time::Instant::now();
        let slot = match *last {
            Some(prev) if prev + MEGA_MIN_GAP > now => prev + MEGA_MIN_GAP,
            _ => now,
        };
        *last = Some(slot);
        slot.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

/// Espera troceada que atiende a la cancelación. Devuelve `true` si se canceló.
///
/// Dormir de un tirón hacía que Pausa pareciera no funcionar: la interfaz
/// marcaba la orden pero la tarea seguía dormida hasta agotar el backoff.
pub async fn sleep_cancellable(total: Duration, cancel: &AtomicBool) -> bool {
    use std::sync::atomic::Ordering;
    const STEP: Duration = Duration::from_millis(150);
    let mut left = total;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return true;
        }
        let step = STEP.min(left);
        tokio::time::sleep(step).await;
        left -= step;
    }
    cancel.load(Ordering::Relaxed)
}

/// Resuelve los metadatos de un archivo público: nombre original y tamaño.
///
/// El nombre que sale de aquí es el que venía cifrado en los atributos y NO
/// está saneado todavía: quien lo use debe pasarlo por el saneador de la
/// aplicación antes de tocar el disco.
pub async fn resolve_file(
    client: &reqwest::Client,
    link: &MegaFileLink,
    folder: Option<&str>,
) -> Result<MegaFileInfo> {
    let raw = crypto::base64url_decode(&link.key_b64)?;
    let key = crypto::FileKey::unpack(&raw)?;

    let t = api::get_public_file(client, &link.handle, folder).await?;
    let name = crypto::decrypt_attributes(&t.attrs_b64, &key)?;

    Ok(MegaFileInfo {
        handle: link.handle.clone(),
        folder: folder.map(|s| s.to_string()),
        name,
        size: t.size,
        key,
        transfer_url: t.transfer_url,
    })
}

/// Descarga completa: reanuda, renueva la URL si caduca y verifica antes de
/// renombrar.
///
/// `final_path` debe venir ya saneado por quien llama.
// Ocho parámetros, uno más del umbral de clippy. Agruparlos en una struct solo
// para contentar al lint escondería que cada uno viene de un sitio distinto:
// el enlace del usuario, los metadatos de la API, la ruta de destino, la señal
// de cancelación y los avisos hacia la interfaz.
#[allow(clippy::too_many_arguments)]
pub async fn download_file(
    client: &reqwest::Client,
    link: &MegaFileLink,
    folder: Option<&str>,
    info: &MegaFileInfo,
    part_path: &Path,
    final_path: &Path,
    cancel: &AtomicBool,
    cb: &Callbacks<'_>,
) -> Result<PathBuf> {
    let mut current = MegaFileInfo {
        handle: info.handle.clone(),
        folder: info.folder.clone(),
        name: info.name.clone(),
        size: info.size,
        key: info.key.clone(),
        transfer_url: info.transfer_url.clone(),
    };

    let mut refreshes = 0u32;
    loop {
        match download::stream_to_part(client, &current, part_path, cancel, cb).await {
            Ok(()) => break,
            Err(MegaError::Cancelled) => return Err(MegaError::Cancelled),
            Err(e) if e.is_retryable() && refreshes < MAX_TRANSFER_REFRESH => {
                refreshes += 1;
                // Backoff exponencial con algo de dispersión, para no sincronizar
                // varios reintentos a la vez contra el mismo servidor.
                let jitter = Duration::from_millis((refreshes as u64 * 137) % 400);
                let total = RETRY_BASE_DELAY * (1 << (refreshes - 1)) + jitter;

                // La espera es cancelable. Antes se dormía de un tirón y pulsar
                // Pausa no hacía nada visible hasta varios segundos después.
                if sleep_cancellable(total, cancel).await {
                    return Err(MegaError::Cancelled);
                }

                // Una URL caducada solo se arregla pidiendo otra al handle original
                if e.needs_fresh_transfer_url() || matches!(e, MegaError::RangeNotSupported) {
                    (cb.phase)(MegaPhase::FetchingMetadata);
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err(MegaError::Cancelled);
                    }
                    current = resolve_file(client, link, folder).await?;
                }
            }
            Err(e) => return Err(e),
        }
    }

    download::finish_and_verify(&current, part_path, final_path, cancel, cb).await?;
    Ok(final_path.to_path_buf())
}

// ================================ Tests ================================

#[cfg(test)]
mod tests {
    use super::crypto::*;
    use super::link::*;
    use super::*;

    // ---------------------------- Enlaces ----------------------------

    #[test]
    fn parsea_enlace_moderno_de_archivo() {
        let l = parse("https://mega.nz/file/ABC12345#0123456789abcdef").unwrap();
        match &l {
            MegaLink::File(f) => {
                assert_eq!(f.handle, "ABC12345");
                assert_eq!(f.key_b64, "0123456789abcdef");
            }
            _ => panic!("debería ser un archivo"),
        }
    }

    #[test]
    fn parsea_enlace_moderno_de_carpeta_con_nodo() {
        let l = parse("https://mega.nz/folder/FOLDER01#KEYKEYKEY/file/NODE0001").unwrap();
        match &l {
            MegaLink::Folder(d) => {
                assert_eq!(d.handle, "FOLDER01");
                assert_eq!(d.key_b64, "KEYKEYKEY");
                assert_eq!(d.node.as_deref(), Some("NODE0001"));
            }
            _ => panic!("debería ser una carpeta"),
        }
    }

    /// REGRESIÓN: el bucle infinito de exploración de carpetas.
    ///
    /// Al expandir una carpeta, cada archivo se encola como
    /// `/folder/H#K/file/NODE`, que TAMBIÉN parsea como `Folder`. Tratarlo como
    /// «carpeta a expandir» hacía que cada fila volviera a listar la carpeta y
    /// se re-expandiera a sí misma: las filas se borraban y recreaban sin
    /// descargar nunca. La distinción es `node`, y no puede perderse.
    #[test]
    fn una_carpeta_con_nodo_designa_un_archivo_no_una_carpeta() {
        let carpeta = parse("https://mega.nz/folder/FOLDER01#LLAVE").unwrap();
        let archivo = parse("https://mega.nz/folder/FOLDER01#LLAVE/file/NODE0001").unwrap();

        match (&carpeta, &archivo) {
            (MegaLink::Folder(a), MegaLink::Folder(b)) => {
                assert!(a.node.is_none(), "una carpeta suelta no lleva nodo");
                assert!(b.node.is_some(), "un archivo dentro de carpeta sí lleva nodo");
            }
            _ => panic!("los dos deben parsear como carpeta"),
        }
        // Y no pueden colapsar en la misma entrada de la cola
        assert_ne!(carpeta.canonical(), archivo.canonical());
    }

    #[test]
    fn parsea_formatos_antiguos() {
        let f = parse("https://mega.nz/#!ABC12345!LLAVE").unwrap();
        assert!(matches!(f, MegaLink::File(_)));
        let d = parse("https://mega.nz/#F!ABC12345!LLAVE").unwrap();
        assert!(matches!(d, MegaLink::Folder(_)));
    }

    #[test]
    fn moderno_y_antiguo_canonicalizan_igual() {
        // Sin esto, el mismo archivo se encolaría dos veces
        let a = parse("https://mega.nz/file/ABC12345#LLAVE").unwrap();
        let b = parse("https://mega.nz/#!ABC12345!LLAVE").unwrap();
        assert_eq!(a.canonical(), b.canonical());
    }

    #[test]
    fn rechaza_dominios_enganosos() {
        // El fallo clásico de comprobar con contains("mega.nz")
        assert!(parse("https://mega.nz.atacante.example/file/A#K").is_err());
        assert!(parse("https://notmega.nz/file/A#K").is_err());
        assert!(parse("https://mega.nz.evil.co/file/A#K").is_err());
        assert!(!is_mega_url("https://mega.nz.atacante.example/file/A#K"));
        // Subdominios legítimos sí
        assert!(is_mega_url("https://www.mega.nz/file/A#K"));
        assert!(is_mega_url("https://mega.co.nz/file/A#K"));
    }

    #[test]
    fn rechaza_enlaces_incompletos_o_corruptos() {
        assert_eq!(parse("https://mega.nz/file/ABC12345"), Err(MegaError::MissingKey));
        assert_eq!(parse("https://mega.nz/file/#LLAVE").unwrap_err(), MegaError::InvalidUrl("falta el identificador"));
        assert!(parse("https://mega.nz/file/ABC12345#llave con espacios").is_err());
        assert!(parse("https://mega.nz/file/ABC12345#llave/rara$$").is_err());
        assert!(parse("no-es-una-url").is_err());
        assert!(parse("ftp://mega.nz/file/A#K").is_err());
        // Componente absurdamente largo
        let largo = "A".repeat(500);
        assert!(parse(&format!("https://mega.nz/file/ABC12345#{largo}")).is_err());
    }

    #[test]
    fn tolera_userinfo_puerto_y_query() {
        assert!(is_mega_url("https://user:pw@mega.nz:443/file/A#K"));
        let l = parse("https://mega.nz/file/ABC12345?utm=x#LLAVE").unwrap();
        assert_eq!(l.handle(), "ABC12345");
    }

    #[test]
    fn la_forma_redactada_no_lleva_la_clave() {
        let l = parse("https://mega.nz/file/ABC12345#SECRETOSECRETO").unwrap();
        let r = l.redacted();
        assert!(!r.contains("SECRETO"), "la clave se ha filtrado: {r}");
        assert!(r.contains("ABC12345"));
    }

    // ---------------------------- Base64 ----------------------------

    #[test]
    fn base64url_ida_y_vuelta() {
        for len in [1usize, 2, 3, 15, 16, 32] {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + 1) as u8).collect();
            let enc = base64url_encode(&data);
            assert!(!enc.contains('='), "MEGA no usa relleno: {enc}");
            assert_eq!(base64url_decode(&enc).unwrap(), data, "len={len}");
        }
    }

    #[test]
    fn base64url_rechaza_caracteres_invalidos() {
        assert!(base64url_decode("abc$def").is_err());
        assert!(base64url_decode("abc def").is_err());
    }

    // ---------------------------- Claves ----------------------------

    #[test]
    fn desempaqueta_la_clave_de_32_bytes() {
        // aes = mitad1 XOR mitad2, iv = bytes 16..24, mac = bytes 24..32
        let raw: Vec<u8> = (0u8..32).collect();
        let k = FileKey::unpack(&raw).unwrap();
        for i in 0..16 {
            assert_eq!(k.aes_key[i], (i as u8) ^ (i as u8 + 16));
        }
        assert_eq!(k.iv, [16, 17, 18, 19, 20, 21, 22, 23]);
        assert_eq!(k.mac, [24, 25, 26, 27, 28, 29, 30, 31]);
    }

    #[test]
    fn rechaza_claves_de_longitud_incorrecta() {
        assert!(FileKey::unpack(&[0u8; 16]).is_err());
        assert!(FileKey::unpack(&[0u8; 31]).is_err());
        assert!(FileKey::unpack(&[0u8; 33]).is_err());
    }

    #[test]
    fn el_debug_de_la_clave_esta_redactado() {
        let k = FileKey::unpack(&(0u8..32).collect::<Vec<_>>()).unwrap();
        let s = format!("{k:?}");
        assert!(s.contains("REDACTED"));
        assert!(!s.contains("16"), "no debe filtrar bytes: {s}");
    }

    // ---------------------------- AES ----------------------------

    #[test]
    fn aes128_coincide_con_el_vector_oficial_fips197() {
        // FIPS-197 apéndice C.1. Valida el primitivo y su uso, no solo que
        // nuestro cifrado sea consistente consigo mismo.
        use aes::cipher::generic_array::GenericArray;
        use aes::cipher::{BlockEncrypt, KeyInit};
        use aes::Aes128;

        let key: [u8; 16] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let mut block = GenericArray::clone_from_slice(&[
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        Aes128::new(GenericArray::from_slice(&key)).encrypt_block(&mut block);
        assert_eq!(
            block.as_slice(),
            &[
                0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4,
                0xc5, 0x5a
            ]
        );
    }

    fn clave_de_prueba() -> FileKey {
        FileKey::unpack(&(0u8..32).map(|i| i.wrapping_mul(7).wrapping_add(3)).collect::<Vec<_>>())
            .unwrap()
    }

    #[test]
    fn ctr_es_involutivo() {
        let k = clave_de_prueba();
        let c = CtrCipher::new(&k);
        let original: Vec<u8> = (0..5000).map(|i| (i % 251) as u8).collect();
        let mut buf = original.clone();
        c.apply(&mut buf, 0);
        assert_ne!(buf, original, "no ha cifrado nada");
        c.apply(&mut buf, 0);
        assert_eq!(buf, original);
    }

    /// EL TEST QUE JUSTIFICA LA REANUDACIÓN.
    ///
    /// Descifrar entero debe dar lo mismo que descifrar hasta `offset` y
    /// continuar desde ahí. Si la construcción del contador estuviera mal, esto
    /// falla en todos los offsets que no sean múltiplo de 16 — que son
    /// exactamente los que aparecen al reanudar de verdad.
    #[test]
    fn reanudar_en_cualquier_offset_da_el_mismo_resultado() {
        let k = clave_de_prueba();
        let c = CtrCipher::new(&k);
        let n = 100_000usize;
        let cifrado: Vec<u8> = (0..n).map(|i| (i * 31 % 256) as u8).collect();

        let mut entero = cifrado.clone();
        c.apply(&mut entero, 0);

        for off in [0usize, 1, 15, 16, 17, 4096, 8191, 65_537, n - 1, n] {
            let mut a = cifrado[..off].to_vec();
            c.apply(&mut a, 0);
            let mut b = cifrado[off..].to_vec();
            c.apply(&mut b, off as u64);
            a.extend_from_slice(&b);
            assert_eq!(a, entero, "la reanudación falla en el offset {off}");
        }
    }

    // ---------------------------- MAC ----------------------------

    fn mac_de(data: &[u8], k: &FileKey) -> [u8; 8] {
        let mut h = MacHasher::new(k);
        h.update(data);
        h.finalize()
    }

    #[test]
    fn el_mac_no_depende_del_troceado() {
        // Se alimenta desde la red en trozos arbitrarios: el resultado no puede
        // cambiar según cómo caigan los paquetes.
        let k = clave_de_prueba();
        let data: Vec<u8> = (0..300_000).map(|i| (i % 253) as u8).collect();
        let de_una = mac_de(&data, &k);

        for trozo in [1usize, 7, 16, 1000, 131_072] {
            let mut h = MacHasher::new(&k);
            for c in data.chunks(trozo) {
                h.update(c);
            }
            assert_eq!(h.finalize(), de_una, "difiere troceando de {trozo} en {trozo}");
        }
    }

    #[test]
    fn el_mac_detecta_corrupcion() {
        let k = clave_de_prueba();
        let data: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
        let bueno = mac_de(&data, &k);

        // Un solo byte cambiado
        let mut alterado = data.clone();
        alterado[123_456] ^= 0x01;
        assert_ne!(mac_de(&alterado, &k), bueno, "un byte cambiado debe fallar");

        // Truncado
        assert_ne!(mac_de(&data[..data.len() - 1], &k), bueno, "truncado debe fallar");

        // Bloques reordenados
        let mut reordenado = data.clone();
        for i in 0..16 {
            reordenado.swap(1000 + i, 2000 + i);
        }
        assert_ne!(mac_de(&reordenado, &k), bueno, "reordenar debe fallar");
    }

    #[test]
    fn el_mac_detecta_clave_incorrecta() {
        let data: Vec<u8> = (0..70_000).map(|i| (i % 199) as u8).collect();
        let k1 = clave_de_prueba();
        let k2 = FileKey::unpack(&(0u8..32).map(|i| i.wrapping_mul(11)).collect::<Vec<_>>()).unwrap();
        assert_ne!(mac_de(&data, &k1), mac_de(&data, &k2));
    }

    #[test]
    fn el_mac_cruza_varios_trozos() {
        // Más de 128 KiB obliga a cerrar trozo y a que crezca el siguiente:
        // es donde se rompería un port descuidado del algoritmo.
        let k = clave_de_prueba();
        let pequeno: Vec<u8> = vec![0xAB; 100];
        let grande: Vec<u8> = vec![0xAB; 400_000];
        assert_ne!(mac_de(&pequeno, &k), mac_de(&grande, &k));
        // Y es determinista
        assert_eq!(mac_de(&grande, &k), mac_de(&grande, &k));
    }

    #[test]
    fn mac_equals_compara_bien() {
        assert!(mac_equals(&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 3, 4, 5, 6, 7, 8]));
        assert!(!mac_equals(&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 3, 4, 5, 6, 7, 9]));
    }

    // ---------------------------- Atributos ----------------------------

    #[test]
    fn los_atributos_exigen_el_prefijo_mega() {
        // Sin la comprobación del prefijo, una clave equivocada podría producir
        // basura que se colase como nombre de archivo.
        let k = clave_de_prueba();
        let basura = base64url_encode(&[0u8; 32]);
        assert_eq!(decrypt_attributes(&basura, &k).unwrap_err(), MegaError::InvalidAttributes);
    }

    #[test]
    fn los_atributos_rechazan_entradas_cortas() {
        let k = clave_de_prueba();
        assert!(decrypt_attributes("AAAA", &k).is_err());
        assert!(decrypt_attributes("", &k).is_err());
    }

    // ---------------------------- Errores ----------------------------

    #[test]
    fn mapea_los_codigos_de_la_api() {
        assert_eq!(MegaError::from_api_code(-9), MegaError::NotFound);
        assert_eq!(MegaError::from_api_code(-3), MegaError::TemporaryUnavailable);
        assert_eq!(MegaError::from_api_code(-4), MegaError::RateLimited);
        assert_eq!(MegaError::from_api_code(-17), MegaError::TransferQuotaExceeded);
        assert_eq!(MegaError::from_api_code(-11), MegaError::AccessDenied);
    }

    #[test]
    fn solo_se_reintenta_lo_reintentable() {
        assert!(MegaError::TemporaryUnavailable.is_retryable());
        assert!(MegaError::ExpiredTransferUrl.is_retryable());
        // Estos no deben entrar nunca en un bucle de reintentos
        assert!(!MegaError::IntegrityMismatch.is_retryable());
        assert!(!MegaError::InvalidKey.is_retryable());
        assert!(!MegaError::Cancelled.is_retryable());
        assert!(!MegaError::TransferQuotaExceeded.is_retryable());
    }

    #[test]
    fn ningun_mensaje_de_error_filtra_secretos() {
        // Se comprueba que no aparezca material de clave real, no que no haya
        // un '#': hay mensajes que legítimamente explican «la parte tras #».
        const SECRETO: &str = "SUPERSECRETKEYMATERIAL";
        let l = parse(&format!("https://mega.nz/file/ABC12345#{SECRETO}")).unwrap();
        assert!(!l.redacted().contains(SECRETO));
        assert!(!format!("{:?}", MegaError::InvalidKey).contains(SECRETO));

        for e in [
            MegaError::InvalidKey,
            MegaError::MissingKey,
            MegaError::IntegrityMismatch,
            MegaError::ExpiredTransferUrl,
            MegaError::from_api_code(-9),
        ] {
            let s = e.to_string();
            assert!(!s.is_empty());
            assert!(!s.contains(SECRETO), "el mensaje filtra la clave: {s}");
        }
    }
}
