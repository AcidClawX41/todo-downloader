//! Criptografía del protocolo público de MEGA.
//!
//! ORIGEN DE CADA SUPUESTO. Nada aquí está escrito de memoria. Cada operación
//! se contrastó contra DOS implementaciones independientes que coinciden:
//!
//!  - megalib 0.11.1 — `src/public.rs::download_public_file_data`,
//!    `src/public.rs::decrypt_public_attrs`, `src/crypto/aes.rs::aes128_ctr_decrypt`
//!  - mega-rs 0.8.0 — `src/lib.rs:635` (desempaquetado de la clave),
//!    `src/fingerprint.rs::compute_condensed_mac` (MAC condensado)
//!
//! POR QUÉ LA CLAVE NO SALE DE AQUÍ: MEGA almacena texto cifrado que no puede
//! leer. La clave viaja únicamente en el fragmento de la URL, y los fragmentos
//! no se transmiten al servidor. Enviarla a la API rompería la única garantía
//! que ofrece el sistema. Por eso `FileKey` no implementa Debug de verdad.
//!
//! POR QUÉ CTR NO BASTA: AES-CTR da confidencialidad, no autenticidad. Un byte
//! cambiado en tránsito produce un byte cambiado en el archivo, sin error. Por
//! eso el MAC del protocolo es obligatorio antes de renombrar el `.part`.

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;

use super::error::{MegaError, Result};

// ============================ Base64 de MEGA ============================

/// MEGA usa base64url sin relleno: `+/` → `-_` y sin `=`.
pub fn base64url_decode(s: &str) -> Result<Vec<u8>> {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut rev = [255u8; 256];
    for (i, c) in TBL.iter().enumerate() {
        rev[*c as usize] = i as u8;
    }

    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for b in s.bytes() {
        // Se toleran '=' finales y los alias '+' '/' por si llega una variante
        if b == b'=' {
            continue;
        }
        let v = match b {
            b'+' => 62,
            b'/' => 63,
            _ => {
                let v = rev[b as usize];
                if v == 255 {
                    return Err(MegaError::InvalidKey);
                }
                v
            }
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

pub fn base64url_encode(data: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let idx = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for (i, ix) in idx.iter().enumerate() {
            if i <= c.len() {
                out.push(TBL[*ix as usize] as char);
            }
        }
    }
    out
}

// ============================ Clave de archivo ============================

/// Material criptográfico de un archivo público, ya desempaquetado.
///
/// La clave pública de un enlace son 32 bytes que NO son la clave AES: hay que
/// desdoblarlos. Confirmado idénticamente en megalib
/// (`download_public_file_data`) y mega-rs (`lib.rs:635-639`).
#[derive(Clone)]
pub struct FileKey {
    /// AES-128: primera mitad XOR segunda mitad
    pub aes_key: [u8; 16],
    /// Nonce/IV de 8 bytes: bytes 16..24 de la clave pública
    pub iv: [u8; 8],
    /// MAC condensado esperado: bytes 24..32 de la clave pública
    pub mac: [u8; 8],
}

impl FileKey {
    pub fn unpack(raw: &[u8]) -> Result<Self> {
        if raw.len() != 32 {
            return Err(MegaError::InvalidKey);
        }
        let mut aes_key = [0u8; 16];
        for i in 0..16 {
            aes_key[i] = raw[i] ^ raw[i + 16];
        }
        let mut iv = [0u8; 8];
        iv.copy_from_slice(&raw[16..24]);
        let mut mac = [0u8; 8];
        mac.copy_from_slice(&raw[24..32]);
        Ok(FileKey { aes_key, iv, mac })
    }
}

/// Debug redactado a propósito: la clave de un enlace público sigue siendo un
/// secreto, y un `{:?}` accidental en un log o en un panic la filtraría.
impl std::fmt::Debug for FileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileKey([REDACTED])")
    }
}

impl Drop for FileKey {
    /// Borrado best-effort del material sensible al soltarlo.
    fn drop(&mut self) {
        for b in self.aes_key.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        for b in self.iv.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

// ============================ AES-128-CTR ============================

/// Descifrador CTR posicionable.
///
/// El bloque de contador es `iv(8 bytes) || (offset_bytes / 16) as u64 BE`,
/// tal como construye megalib en `aes128_ctr_decrypt` (líneas 181-206).
///
/// Que sea posicionable es lo que permite reanudar: para seguir en el byte N se
/// coloca el contador en N/16 y se descarta el desfase N%16 del primer bloque.
/// Sin esta propiedad, reanudar exigiría volver a descargar desde cero.
pub struct CtrCipher {
    cipher: Aes128,
    iv: [u8; 8],
}

impl CtrCipher {
    pub fn new(key: &FileKey) -> Self {
        CtrCipher {
            cipher: Aes128::new(GenericArray::from_slice(&key.aes_key)),
            iv: key.iv,
        }
    }

    /// Descifra `buf` en el sitio, sabiendo que empieza en `byte_offset` del
    /// archivo completo. `byte_offset` NO tiene que estar alineado a 16.
    pub fn apply(&self, buf: &mut [u8], byte_offset: u64) {
        let mut counter = byte_offset / 16;
        let mut skip = (byte_offset % 16) as usize;
        let mut done = 0usize;

        let mut block = [0u8; 16];
        block[..8].copy_from_slice(&self.iv);

        while done < buf.len() {
            block[8..].copy_from_slice(&counter.to_be_bytes());
            let mut ks = GenericArray::clone_from_slice(&block);
            self.cipher.encrypt_block(&mut ks);

            let take = (16 - skip).min(buf.len() - done);
            for i in 0..take {
                buf[done + i] ^= ks[skip + i];
            }
            done += take;
            skip = 0;
            counter += 1;
        }
    }
}

// ============================ Atributos ============================

/// AES-128-CBC con IV cero, sólo descifrado. Se implementa aquí en vez de
/// añadir el crate `cbc` porque son doce líneas y evita una dependencia más.
fn aes128_cbc_decrypt_zero_iv(data: &[u8], key: &[u8; 16]) -> Vec<u8> {
    use aes::cipher::BlockDecrypt;
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            out.push(block[i] ^ prev[i]);
        }
        prev.copy_from_slice(chunk);
    }
    out
}

/// AES-128-ECB, solo descifrado. Lo usa el desempaquetado de claves de nodo
/// dentro de una carpeta pública (megalib `decrypt_public_node_key`).
pub fn aes128_ecb_decrypt(data: &[u8], key: &[u8; 16]) -> Vec<u8> {
    use aes::cipher::BlockDecrypt;
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

/// Clave AES efectiva de un nodo para descifrar sus atributos.
///
/// Un nodo de archivo trae 32 bytes y hay que plegarlos; una carpeta trae 16 y
/// se usan tal cual. Idéntico a megalib `decrypt_public_node_attrs`.
pub fn node_attr_key(node_key: &[u8]) -> Option<[u8; 16]> {
    if node_key.len() >= 32 {
        let mut k = [0u8; 16];
        for i in 0..16 {
            k[i] = node_key[i] ^ node_key[i + 16];
        }
        Some(k)
    } else if node_key.len() >= 16 {
        node_key[..16].try_into().ok()
    } else {
        None
    }
}

/// Descifra el blob `at`/`a` con una clave AES ya derivada.
pub fn decrypt_attributes_with(attrs_b64: &str, aes_key: &[u8; 16]) -> Result<String> {
    let raw = base64url_decode(attrs_b64).map_err(|_| MegaError::InvalidAttributes)?;
    if raw.len() < 16 {
        return Err(MegaError::InvalidAttributes);
    }
    let plain = aes128_cbc_decrypt_zero_iv(&raw, aes_key);

    if !plain.starts_with(b"MEGA") {
        return Err(MegaError::InvalidAttributes);
    }
    let json_part = &plain[4..];
    let end = json_part.iter().position(|b| *b == 0).unwrap_or(json_part.len());
    let text = std::str::from_utf8(&json_part[..end]).map_err(|_| MegaError::InvalidAttributes)?;

    let v: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|_| MegaError::InvalidAttributes)?;
    v.get("n")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or(MegaError::InvalidAttributes)
}

/// Descifra el blob `at` de un archivo público y devuelve el nombre original.
///
/// El prefijo «MEGA» es la comprobación de que la clave era la correcta: sin
/// él, un JSON arbitrario podría colarse como nombre de archivo. Se valida
/// ANTES de mirar el contenido, como hace megalib en `decrypt_public_attrs`.
pub fn decrypt_attributes(attrs_b64: &str, key: &FileKey) -> Result<String> {
    decrypt_attributes_with(attrs_b64, &key.aes_key)
}

// ============================ MAC condensado ============================

/// Tamaño del primer trozo del MAC y su incremento (2^17 = 128 KiB).
const MAC_CHUNK_START: u64 = 131_072;
/// Tope al que deja de crecer el trozo (1 MiB).
const MAC_CHUNK_MAX: u64 = 1_048_576;

/// Calculador incremental del MAC condensado de MEGA.
///
/// Portado de mega-rs `src/fingerprint.rs::compute_condensed_mac`. El esquema:
/// el archivo se parte en trozos de 128 KiB que crecen de 128 en 128 hasta
/// 1 MiB; cada trozo se pasa por CBC-MAC con IV = `iv||iv`; el MAC de cada
/// trozo alimenta un segundo CBC con IV cero; al final los 16 bytes se
/// condensan a 8 plegándolos con XOR.
///
/// Se alimenta con el TEXTO CLARO, no con el cifrado.
pub struct MacHasher {
    cipher: Aes128,
    chunk_iv: [u8; 16],
    /// MAC del trozo en curso
    cur: [u8; 16],
    /// Acumulador CBC sobre los MAC de cada trozo
    final_mac: [u8; 16],
    /// Bytes que faltan para cerrar el trozo actual
    remaining_in_chunk: u64,
    /// Tamaño del trozo actual
    chunk_size: u64,
    /// Restos sin completar un bloque de 16
    partial: [u8; 16],
    partial_len: usize,
    started: bool,
}

impl MacHasher {
    pub fn new(key: &FileKey) -> Self {
        let mut chunk_iv = [0u8; 16];
        chunk_iv[..8].copy_from_slice(&key.iv);
        chunk_iv[8..].copy_from_slice(&key.iv);
        MacHasher {
            cipher: Aes128::new(GenericArray::from_slice(&key.aes_key)),
            chunk_iv,
            cur: [0u8; 16],
            final_mac: [0u8; 16],
            remaining_in_chunk: MAC_CHUNK_START,
            chunk_size: MAC_CHUNK_START,
            partial: [0u8; 16],
            partial_len: 0,
            started: false,
        }
    }

    fn begin_chunk(&mut self) {
        self.cur = self.chunk_iv;
        self.started = true;
    }

    /// CBC-MAC de un bloque de 16 bytes dentro del trozo actual
    fn absorb_block(&mut self, block: &[u8; 16]) {
        if !self.started {
            self.begin_chunk();
        }
        let mut b = [0u8; 16];
        for i in 0..16 {
            b[i] = block[i] ^ self.cur[i];
        }
        let mut g = GenericArray::clone_from_slice(&b);
        self.cipher.encrypt_block(&mut g);
        self.cur.copy_from_slice(&g);
    }

    fn close_chunk(&mut self) {
        if !self.started {
            return;
        }
        // Un bloque incompleto al final del trozo se rellena con ceros
        if self.partial_len > 0 {
            let mut padded = [0u8; 16];
            padded[..self.partial_len].copy_from_slice(&self.partial[..self.partial_len]);
            self.partial_len = 0;
            self.absorb_block(&padded);
        }
        // El MAC del trozo entra en el CBC final (IV cero implícito en final_mac)
        let mut b = [0u8; 16];
        for ((dst, cur), mac) in b.iter_mut().zip(self.cur).zip(self.final_mac) {
            *dst = cur ^ mac;
        }
        let mut g = GenericArray::clone_from_slice(&b);
        self.cipher.encrypt_block(&mut g);
        self.final_mac.copy_from_slice(&g);

        self.started = false;
        if self.chunk_size < MAC_CHUNK_MAX {
            self.chunk_size += MAC_CHUNK_START;
        }
        self.remaining_in_chunk = self.chunk_size;
    }

    /// Alimenta texto claro. Puede llamarse con trozos de cualquier tamaño.
    pub fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let room = self.remaining_in_chunk as usize;
            let take = room.min(data.len());
            let (head, tail) = data.split_at(take);

            let mut src = head;
            // Completar primero un bloque parcial pendiente
            if self.partial_len > 0 {
                let need = 16 - self.partial_len;
                let n = need.min(src.len());
                self.partial[self.partial_len..self.partial_len + n].copy_from_slice(&src[..n]);
                self.partial_len += n;
                src = &src[n..];
                if self.partial_len == 16 {
                    let blk = self.partial;
                    self.partial_len = 0;
                    self.absorb_block(&blk);
                }
            }
            let full = src.len() - src.len() % 16;
            for blk in src[..full].chunks_exact(16) {
                let mut b = [0u8; 16];
                b.copy_from_slice(blk);
                self.absorb_block(&b);
            }
            let rest = &src[full..];
            if !rest.is_empty() {
                self.partial[..rest.len()].copy_from_slice(rest);
                self.partial_len = rest.len();
            }

            self.remaining_in_chunk -= take as u64;
            if self.remaining_in_chunk == 0 {
                self.close_chunk();
            }
            data = tail;
        }
    }

    /// Cierra y devuelve los 8 bytes que deben coincidir con `FileKey::mac`.
    pub fn finalize(mut self) -> [u8; 8] {
        if self.started || self.partial_len > 0 {
            self.close_chunk();
        }
        let m = &mut self.final_mac;
        for i in 0..4 {
            m[i] ^= m[i + 4];
            m[i + 4] = m[i + 8] ^ m[i + 12];
        }
        let mut out = [0u8; 8];
        out.copy_from_slice(&m[..8]);
        out
    }
}

/// Comparación en tiempo constante. Un `==` normal puede terminar antes en el
/// primer byte distinto; aquí no importa mucho, pero es gratis hacerlo bien.
pub fn mac_equals(a: &[u8; 8], b: &[u8; 8]) -> bool {
    let mut diff = 0u8;
    for i in 0..8 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}
