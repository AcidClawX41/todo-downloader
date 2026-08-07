//! Errores del motor MEGA.
//!
//! Se distingue cada fallo en su propia variante en vez de devolver cadenas
//! sueltas: la interfaz necesita saber si merece la pena reintentar, si hay que
//! pedir una URL de transferencia nueva o si el archivo está corrupto y no debe
//! aparecer nunca con su nombre definitivo.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MegaError {
    /// La URL no es un enlace de MEGA válido
    InvalidUrl(&'static str),
    /// Enlace reconocido pero de un tipo que este motor no cubre
    UnsupportedLinkType,
    /// Falta el fragmento con la clave (`#...`)
    MissingKey,
    /// La clave existe pero no decodifica a la longitud esperada
    InvalidKey,
    /// Los atributos no descifran, o no llevan el prefijo «MEGA»
    InvalidAttributes,
    /// El nodo público ya no existe (API -9 ENOENT)
    NotFound,
    /// Acceso denegado (API -11 EACCESS / -16 blocked)
    AccessDenied,
    /// Cuota de transferencia agotada (HTTP 509, API -17 ETOOMANY)
    TransferQuotaExceeded,
    /// Limitación de ritmo: hay que esperar (API -4 ERATELIMIT)
    RateLimited,
    /// Congestión temporal; MEGA pide reintento con espera (API -3 EAGAIN)
    TemporaryUnavailable,
    /// La URL de transferencia caducó: hay que pedir otra
    ExpiredTransferUrl,
    /// Se pidió un rango y el servidor devolvió el archivo entero
    RangeNotSupported,
    /// El `Content-Range` no cuadra con lo solicitado
    InvalidContentRange,
    /// El tamaño final no coincide con el que anunciaron los metadatos
    SizeMismatch { expected: u64, got: u64 },
    /// El MAC del archivo no coincide: los datos están corruptos
    IntegrityMismatch,
    /// El usuario pausó o canceló
    Cancelled,
    Io(String),
    Http(String),
    /// Código de error de la API de MEGA sin traducción específica
    Api(i64),
    /// La respuesta no tiene la forma esperada
    MalformedResponse(&'static str),
}

impl MegaError {
    /// Mapea los códigos numéricos de la API de MEGA.
    ///
    /// Referencia: códigos negativos documentados por el SDK oficial y usados
    /// de forma idéntica por megalib (`src/api`) y mega-rs (`src/error.rs`).
    pub fn from_api_code(code: i64) -> Self {
        match code {
            -3 => MegaError::TemporaryUnavailable, // EAGAIN
            -4 => MegaError::RateLimited,          // ERATELIMIT
            -9 => MegaError::NotFound,             // ENOENT
            -11 => MegaError::AccessDenied,        // EACCESS
            -15 => MegaError::AccessDenied,        // ESID
            -16 => MegaError::AccessDenied,        // EBLOCKED
            -17 => MegaError::TransferQuotaExceeded, // ETOOMANY
            -18 => MegaError::TemporaryUnavailable, // EEXPIRED / temporarily unavailable
            other => MegaError::Api(other),
        }
    }

    /// ¿Tiene sentido reintentar solo? Los errores definitivos no deben entrar
    /// en un bucle de reintentos que nunca va a converger.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            MegaError::TemporaryUnavailable
                | MegaError::RateLimited
                | MegaError::ExpiredTransferUrl
                | MegaError::Http(_)
                | MegaError::Io(_)
        )
    }

    /// ¿Basta con pedir una URL de transferencia nueva y seguir desde el `.part`?
    pub fn needs_fresh_transfer_url(&self) -> bool {
        matches!(
            self,
            MegaError::ExpiredTransferUrl | MegaError::AccessDenied | MegaError::NotFound
        )
    }
}

impl fmt::Display for MegaError {
    /// Mensajes accionables, en el idioma elegido en la aplicación.
    ///
    /// Nunca incluyen la clave ni el fragmento de la URL.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::i18n::Lang;
        let es = crate::i18n::lang() == Lang::Es;
        macro_rules! m {
            ($esp:expr, $eng:expr) => {
                if es { $esp } else { $eng }
            };
        }
        match self {
            MegaError::InvalidUrl(why) => write!(
                f, "{}({why})", m!("MEGA: enlace no válido ", "MEGA: invalid link ")
            ),
            MegaError::UnsupportedLinkType => write!(f, "{}", m!(
                "MEGA: tipo de enlace no soportado (solo archivos y carpetas públicos)",
                "MEGA: unsupported link type (public files and folders only)"
            )),
            MegaError::MissingKey => write!(f, "{}", m!(
                "MEGA: al enlace le falta la clave de descifrado (la parte tras #)",
                "MEGA: the link is missing its decryption key (the part after #)"
            )),
            MegaError::InvalidKey => write!(f, "{}", m!(
                "MEGA: la clave del enlace no es válida",
                "MEGA: the link's key is not valid"
            )),
            MegaError::InvalidAttributes => write!(f, "{}", m!(
                "MEGA: no se pudo descifrar el nombre del archivo; la clave no corresponde",
                "MEGA: could not decrypt the file name; the key does not match"
            )),
            MegaError::NotFound => write!(f, "{}", m!(
                "MEGA: el archivo ya no existe o el enlace fue retirado",
                "MEGA: the file no longer exists or the link was taken down"
            )),
            MegaError::AccessDenied => write!(f, "{}", m!(
                "MEGA: acceso denegado a este enlace",
                "MEGA: access denied for this link"
            )),
            MegaError::TransferQuotaExceeded => write!(f, "{}", m!(
                "MEGA: cuota de transferencia agotada; inténtalo más tarde",
                "MEGA: transfer quota exhausted; try again later"
            )),
            MegaError::RateLimited => write!(f, "{}", m!(
                "MEGA: demasiadas peticiones; esperando",
                "MEGA: too many requests; waiting"
            )),
            MegaError::TemporaryUnavailable => write!(f, "{}", m!(
                "MEGA: no disponible temporalmente",
                "MEGA: temporarily unavailable"
            )),
            MegaError::ExpiredTransferUrl => write!(f, "{}", m!(
                "MEGA: el enlace de transferencia caducó",
                "MEGA: the transfer link expired"
            )),
            MegaError::RangeNotSupported => write!(f, "{}", m!(
                "MEGA: el servidor ignoró la reanudación; reiniciando de forma segura",
                "MEGA: the server ignored the resume request; restarting safely"
            )),
            MegaError::InvalidContentRange => write!(f, "{}", m!(
                "MEGA: rango de bytes incoherente",
                "MEGA: inconsistent byte range"
            )),
            MegaError::SizeMismatch { expected, got } => write!(
                f, "{}({expected} / {got})",
                m!("MEGA: tamaño final incorrecto: esperado / recibido ",
                   "MEGA: wrong final size: expected / received ")
            ),
            MegaError::IntegrityMismatch => write!(f, "{}", m!(
                "MEGA: la verificación de integridad falló; el archivo final NO se ha creado",
                "MEGA: integrity check failed; the final file was NOT created"
            )),
            MegaError::Cancelled => write!(f, "{}", m!("MEGA: cancelado", "MEGA: cancelled")),
            MegaError::Io(e) => write!(
                f, "{}{e}", m!("MEGA: error de disco: ", "MEGA: disk error: ")
            ),
            MegaError::Http(e) => write!(
                f, "{}{e}", m!("MEGA: error de red: ", "MEGA: network error: ")
            ),
            MegaError::Api(c) => write!(
                f, "{}{c}", m!("MEGA: la API devolvió el error ", "MEGA: the API returned error ")
            ),
            MegaError::MalformedResponse(w) => write!(
                f, "{}({w})", m!("MEGA: respuesta inesperada ", "MEGA: unexpected response ")
            ),
        }
    }
}

impl std::error::Error for MegaError {}

pub type Result<T> = std::result::Result<T, MegaError>;
