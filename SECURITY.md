# Seguridad — Todo Downloader

Resumen de la auditoría de seguridad de la v1.0.0 y del modelo de amenazas de la aplicación.

## Canales de comunicación

Todo el tráfico de red usa **HTTPS con rustls** (implementación TLS en Rust puro, sin OpenSSL del sistema). No hay opción de aceptar certificados inválidos en el código. Cualquier enlace `http://` añadido a la cola se **reescribe automáticamente a `https://`** antes de descargarse: la aplicación nunca transmite en claro. Los motores auxiliares se descargan exclusivamente de URLs fijas de GitHub Releases (`github.com/yt-dlp`, `github.com/mikf/gallery-dl`) sobre TLS.

## Superficie de ataque y mitigaciones

**Inyección de comandos**: yt-dlp y gallery-dl se invocan con `Command` + argumentos como array — nunca pasa nada por un shell, por lo que no existe inyección de shell. Contra la *inyección de argumentos* (una "URL" maliciosa que empiece por `-` e intente colarse como flag, p. ej. `--exec`), toda URL se pasa tras el separador `--`, que cierra la lista de opciones. Además, solo se aceptan cadenas que empiecen por `http`.

**Path traversal**: todos los nombres de archivo y carpetas de autor pasan por `sanitize()`, que elimina separadores de ruta, caracteres de control y comodines, recorta puntos finales y neutraliza los nombres de dispositivo reservados de Windows (CON, NUL, COM1…). Un título de vídeo malicioso no puede escribir fuera de la carpeta de destino.

**Binarios descargados (cadena de suministro)**: tras descargar yt-dlp, gallery-dl o ffmpeg se ejecuta una verificación (`--version` / `-version`); si el binario no responde correctamente se **elimina** y se notifica. En el caso de ffmpeg, que llega como archivo comprimido, se extraen **únicamente** `ffmpeg.exe` y `ffprobe.exe` filtrando por ruta exacta (`*/bin/ffmpeg.exe`); ningún otro contenido del paquete se escribe a disco, y el zip temporal se borra al terminar. El origen es el build oficial que mantiene el propio equipo de yt-dlp. Se escriben primero como `.part` y se renombran de forma atómica. Limitación conocida: no se verifica firma criptográfica del release (GitHub no publica firmas uniformes para estos proyectos); la confianza recae en TLS + github.com. Mantén Windows Defender/AV activo como capa adicional.

**Portapapeles (LinkGrabber)**: se lee localmente cada 900 ms *solo si está activado*. Únicamente se extraen URLs de sitios conocidos (o cualquiera, si el usuario lo habilita expresamente); el contenido del portapapeles **nunca se registra, persiste ni transmite**. Se puede desactivar en Ajustes.

**Cookies del navegador**: función *opt-in*. Las cookies las leen yt-dlp/gallery-dl directamente y viajan solo hacia el sitio de destino sobre TLS; esta aplicación nunca las toca, guarda ni reenvía.

**Datos en reposo**: los ajustes persistidos no contienen secretos (rutas, booleanos, nombre de navegador). No se almacenan credenciales ni tokens.

**Receptor local (captura desde el navegador)**: la función Click'n'Load abre un endpoint HTTP mínimo con estas restricciones — bind **exclusivo a 127.0.0.1** (inalcanzable desde la red, ni siquiera desde otra máquina de la LAN), desactivable desde Ajustes, cuerpo limitado a 8 MiB, y **solo se aceptan cadenas `http://` o `https://`**: cualquier otra (`javascript:`, `file:`, rutas locales) se descarta. Lo recibido únicamente se encola como descarga; nunca se ejecuta ni se evalúa. La cabecera CORS es permisiva por necesidad — el navegador debe poder enviar desde tiktok.com o douyin.com — lo cual es aceptable porque el endpoint no es alcanzable fuera de la máquina y su única acción posible es añadir una URL a la cola. Riesgo residual: una web maliciosa abierta en el navegador podría encolar descargas no deseadas mientras el receptor esté activo; se ven en la cola y no se inician solas salvo que actives el autoarranque.

**Recursos**: concurrencia limitada (1–8), reintentos acotados con backoff, análisis de perfil limitado a 2000 entradas, timeout de conexión de 15 s.

## Lo que esta aplicación NO hace

No tiene telemetría ni analítica, no ejecuta contenido descargado, no se auto-actualiza en silencio, no toca el registro de Windows y no requiere privilegios de administrador. El único puerto de escucha es el receptor local descrito arriba, siempre limitado a 127.0.0.1 y desactivable.

## Limitaciones conocidas

- Pausar una tarea de yt-dlp/gallery-dl en curso no mata el subproceso (termina su descarga actual).
- Los archivos descargados de sitios arbitrarios son responsabilidad del usuario; la app no los analiza (usa tu AV).
- Sin verificación de firma de los binarios auxiliares (ver arriba).

## Reporte de vulnerabilidades

Abre un issue privado en el repositorio o contacta al autor. — *By Eric V. Gramunt*
