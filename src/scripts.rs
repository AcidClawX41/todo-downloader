//! Scripts de consola que capturan enlaces en el navegador y los envían
//! directamente a la cola de Todo Downloader (estilo Click'n'Load).
//!
//! Se ejecutan en la pestaña del perfil (F12 → Consola). Como corren dentro de
//! la propia página, heredan la sesión y las firmas de la API, que es justo lo
//! que yt-dlp no puede replicar desde fuera.

/// Cabecera común: panel visual (HUD) + envío al receptor local.
///
/// Toda la información se muestra en un panel flotante dentro de la página, no
/// en la consola: los sitios como Douyin escupen cientos de errores propios
/// (CORS, xgplayer, zijieapi) y el progreso se perdía entre el ruido.
/// Texto en el idioma activo de la aplicación.
fn m(es: &'static str, en: &'static str) -> &'static str {
    if crate::i18n::lang() == crate::i18n::Lang::Es { es } else { en }
}

/// Comillas y barras escapadas para meter el texto en un literal de JavaScript.
fn js(t: &str) -> String {
    format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Diccionario `T` que se inyecta al principio de cada script.
///
/// POR QUÉ AQUÍ Y NO EN `i18n.rs`: estos textos no los pinta la aplicación,
/// los pinta el NAVEGADOR. Viajan dentro del JavaScript que el usuario pega en
/// la consola, así que el idioma se resuelve al generar el script y se manda
/// ya traducido. El script no puede preguntarle nada a la aplicación: puede
/// estar ejecutándose sin que el receptor esté siquiera encendido.
fn dic() -> String {
    let pares: &[(&str, &str, &str)] = &[
        ("capturando", "Capturando…", "Capturing…"),
        ("enlaces", "enlaces", "links"),
        ("archivos", "archivos", "files"),
        ("detener", "■ Detener y enviar", "■ Stop and send"),
        ("guardar", "💾 Guardar JSON", "💾 Save JSON"),
        ("deteniendo", "Deteniendo…", "Stopping…"),
        ("cerrar", "Cerrar", "Close"),
        ("enviando", "Enviando…", "Sending…"),
        ("enviado", "Enviado a la app ✓", "Sent to the app ✓"),
        ("terminado", "Terminado", "Finished"),
        ("enCola", "Ya están en la cola de descargas", "They are in the download queue now"),
        ("enviadosLog", "enlaces enviados a Todo Downloader", "links sent to Todo Downloader"),
        ("respondio", "El receptor respondió ", "The receiver replied "),
        ("noApp", "No contesta la app. Ábrela y mira en Capturar que el receptor esté escuchando; si tienes DOS copias abiertas, solo una tiene el puerto. Copiando al portapapeles…",
                  "The app is not answering. Open it and check in Capture that the receiver is listening; if you have TWO copies open, only one holds the port. Copying to the clipboard…"),
        ("noRecep", "Receptor no disponible:", "Receiver unavailable:"),
        ("copiados", "📋 Copiados al portapapeles", "📋 Copied to the clipboard"),
        ("copiadosLog", "📋 Enlaces copiados — el LinkGrabber los detectará",
                        "📋 Links copied — LinkGrabber will pick them up"),
        ("usaGuardar", "Tampoco ha entrado en el portapapeles (la pestaña tiene que tener el foco). Usa el botón 💾 Guardar JSON y luego Descargas → Importar TXT/JSON.",
                       "It did not reach the clipboard either (the tab must have focus). Use the 💾 Save JSON button, then Downloads → Import TXT/JSON."),
        ("jsonGuardado", "JSON guardado con", "JSON saved with"),
        ("elementos", "elementos", "items"),
        ("sinApi", "Sin respuestas de la API", "No replies from the API"),
        ("scrollBloqueado", "¿scroll bloqueado o login?", "scroll blocked, or sign-in needed?"),
        ("analizados", "analizados", "scanned"),
        ("sinUrl", "sin URL", "no URL"),
        ("capturados", "capturados", "captured"),
        ("diagnostico", "Diagnóstico:", "Diagnostics:"),
        ("apiNoResp", "La API no respondió: inicia sesión y recarga el perfil",
                      "The API did not reply: sign in and reload the profile"),
        ("respRecibidas", "Respuestas recibidas", "Replies received"),
        ("sinUtiles", "pero sin enlaces utilizables", "but no usable links"),
        ("ejecutaDouyin", "Todo Downloader: ejecuta esto en una página de perfil de Douyin (/user/...)",
                          "Todo Downloader: run this on a Douyin profile page (/user/...)"),
        ("leyendo", "Leyendo publicaciones…", "Reading posts…"),
        ("iniciada", "Captura iniciada — sigue el progreso en el panel de la esquina",
                     "Capture started — follow the progress in the corner panel"),
        ("vacia", "Respuesta vacía, reintentando", "Empty reply, retrying"),
        ("publicaciones", "publicaciones", "posts"),
        ("videos", "vídeos", "videos"),
        ("imagenes", "imágenes", "images"),
        ("reintentando", "Reintentando", "Retrying"),
        ("analizandoPag", "Analizando la página…", "Analyzing the page…"),
        ("pagina", "página", "page"),
        ("fotos", "fotos", "photos"),
        ("sinAlbumes", "No se ven álbumes en esta página", "No albums visible on this page"),
        ("albumesPag", "álbumes en esta página", "albums on this page"),
        ("album", "Álbum", "Album"),
        ("albumOmitido", "álbum omitido:", "album skipped:"),
        ("abreAlbum", "Abre un álbum o la página de una modelo/agencia",
                      "Open an album, or a model/agency page"),
        ("sinFotos", "No se ha encontrado ninguna foto", "No photo was found"),
        ("postSinId", "Abre un post concreto: la URL no lleva ningún identificador",
                      "Open a specific post: the URL carries no identifier"),
        ("postBuscando", "Buscando los archivos del post…", "Looking for the post files…"),
        ("postAytdlp", "El reproductor no expone una URL descargable (blob:). Se manda el post a la cola para que lo resuelva yt-dlp.",
                       "The player exposes no downloadable URL (blob:). The post is sent to the queue for yt-dlp to resolve."),
        ("postNada", "No se ha encontrado ninguna imagen en este post. Pasa el carrusel una vez a mano y vuelve a intentarlo.",
                     "No image was found in this post. Step through the carousel once by hand and try again."),
        ("postVariosVideos",
         "ATENCIÓN: este post tiene varias diapositivas. Se manda la URL del post y yt-dlp resuelve SOLO la primera; las demás no se descargan. Diapositivas detectadas:",
         "WARNING: this post has several slides. The post URL is sent and yt-dlp resolves ONLY the first one; the rest are not downloaded. Slides detected:"),
        ("desplazando", "Desplazando el perfil…", "Scrolling the profile…"),
        ("thPerfil", "Todo Downloader: abre un perfil o un post de Threads (threads.com/@usuario)",
                     "Todo Downloader: open a Threads profile or post (threads.com/@user)"),
        ("thLeyendo", "Leyendo lo que carga la página…", "Reading what the page loads…"),
        ("thAjenos", "de otras cuentas", "from other accounts"),
        ("thSinMedios", "Threads respondió, pero sin archivos de este perfil",
                        "Threads replied, but with no files from this profile"),
    ];
    let cuerpo: String = pares
        .iter()
        .map(|&(k, es, en)| format!("  {k}: {},\n", js(m(es, en))))
        .collect();
    format!("const T = {{\n{cuerpo}}};\n")
}

fn sender(port: u16) -> String {
    let dic = dic();
    format!(
        r#"
// ---- Panel visual de Todo Downloader ----
const TD_PORT = {port};
{dic}

function tdHud() {{
    document.getElementById('__td_hud')?.remove();
    const el = document.createElement('div');
    el.id = '__td_hud';
    el.style.cssText = [
        'position:fixed', 'top:18px', 'right:18px', 'z-index:2147483647',
        'background:#151821', 'color:#E8EAF0',
        'font:13px/1.5 -apple-system,Segoe UI,Roboto,sans-serif',
        'border:1px solid #262B39', 'border-radius:14px',
        'padding:14px 16px', 'min-width:250px',
        'box-shadow:0 10px 40px rgba(0,0,0,.55)', 'user-select:none'
    ].join(';');
    el.innerHTML = `
      <div style="display:flex;align-items:center;gap:9px;margin-bottom:10px">
        <div style="width:26px;height:26px;border-radius:8px;background:#FE2C55;
                    display:flex;align-items:center;justify-content:center;font-size:15px">⬇</div>
        <div style="line-height:1.15">
          <div style="font-weight:600">Todo <span style="color:#25F4EE">Downloader</span></div>
          <div id="__td_sub" style="font-size:10.5px;color:#8A90A0">${{T.capturando}}</div>
        </div>
        <div id="__td_x" style="margin-left:auto;cursor:pointer;color:#8A90A0;font-size:16px;padding:0 4px">×</div>
      </div>
      <div style="display:flex;align-items:baseline;gap:7px">
        <div id="__td_n" style="font-size:30px;font-weight:600;color:#25F4EE">0</div>
        <div style="font-size:11px;color:#8A90A0" id="__td_lbl">${{T.enlaces}}</div>
      </div>
      <div style="height:5px;background:#262B39;border-radius:3px;margin:10px 0 4px;overflow:hidden">
        <div id="__td_bar" style="height:100%;width:30%;background:linear-gradient(90deg,#FE2C55,#25F4EE);
             border-radius:3px;transition:width .3s;animation:__tdp 1.4s ease-in-out infinite"></div>
      </div>
      <div id="__td_msg" style="font-size:11px;color:#8A90A0;min-height:15px"></div>
      <button id="__td_stop" style="width:100%;margin-top:9px;padding:7px;border:0;border-radius:8px;
              background:#262B39;color:#E8EAF0;font-size:12px;cursor:pointer">${{T.detener}}</button>
      <button id="__td_save" style="width:100%;margin-top:6px;padding:7px;border:0;border-radius:8px;
              background:#FE2C55;color:#fff;font-size:12px;cursor:pointer;display:none">${{T.guardar}}</button>
      <style>@keyframes __tdp{{0%{{transform:translateX(-100%)}}100%{{transform:translateX(340%)}}}}</style>`;
    document.body.appendChild(el);
    el.querySelector('#__td_x').onclick = () => el.remove();
    el.querySelector('#__td_stop').onclick = () => {{
        window.__tdStop = true;
        el.querySelector('#__td_msg').textContent = T.deteniendo;
    }};
    // Guardar a archivo: vía manual siempre disponible al terminar
    el.querySelector('#__td_save').onclick = () => tdSaveFile(window.__tdItems || []);
    return {{
        n(v) {{ const e = document.getElementById('__td_n'); if (e) e.textContent = v; }},
        lbl(t) {{ const e = document.getElementById('__td_lbl'); if (e) e.textContent = t; }},
        msg(t) {{ const e = document.getElementById('__td_msg'); if (e) e.textContent = t; }},
        sub(t) {{ const e = document.getElementById('__td_sub'); if (e) e.textContent = t; }},
        done(v, ok, extra) {{
            const bar = document.getElementById('__td_bar');
            if (bar) {{ bar.style.animation = 'none'; bar.style.width = '100%'; }}
            const btn = document.getElementById('__td_stop');
            if (btn) {{ btn.textContent = T.cerrar; btn.onclick = () => document.getElementById('__td_hud')?.remove(); }}
            // El botón de guardar aparece al terminar, haya ido bien el envío o no
            const sv = document.getElementById('__td_save');
            if (sv && v > 0) sv.style.display = 'block';
            const sub = document.getElementById('__td_sub');
            if (sub) {{ sub.textContent = ok ? T.enviado : T.terminado; sub.style.color = ok ? '#3DDC84' : '#FFB454'; }}
            this.n(v);
            this.msg(extra || '');
        }}
    }};
}}

async function tdSend(items, hud, mode) {{
    if (!items.length) return false;
    const cuerpo = JSON.stringify({{ source: location.hostname, items, mode: mode || '' }});

    // Chrome y Vivaldi bloquean que una PÁGINA hable con 127.0.0.1 (Private
    // Network Access). GM_xmlhttpRequest corre en el contexto de la extensión
    // del gestor de userscripts, que no tiene esa restricción. Si no existe
    // —bookmarklet, o consola pelada— se cae a fetch, que en Firefox va bien.
    if (typeof GM_xmlhttpRequest === 'function') {{
        const ok = await new Promise(res => GM_xmlhttpRequest({{
            method: 'POST',
            url: `http://127.0.0.1:${{TD_PORT}}/add`,
            headers: {{ 'Content-Type': 'application/json' }},
            data: cuerpo,
            onload: r => res(r.status >= 200 && r.status < 300),
            onerror: () => res(false),
            ontimeout: () => res(false)
        }}));
        if (ok) {{
            hud && hud.done(items.length, true, T.enCola);
            return true;
        }}
        hud && hud.msg(T.noApp);
        return false;
    }}

    try {{
        const res = await fetch(`http://127.0.0.1:${{TD_PORT}}/add`, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: cuerpo
        }});
        if (res.ok) {{
            hud && hud.done(items.length, true, T.enCola);
            console.log(`[TD] ✅ ${{items.length}} ${{T.enviadosLog}}`);
            return true;
        }}
        hud && hud.msg(T.respondio + res.status);
    }} catch (e) {{
        hud && hud.msg(T.noApp);
        console.warn('[TD]', T.noRecep, e.message);
    }}
    return false;
}}

function tdFallbackCopy(items, hud) {{
    const txt = items.map(i => i.url).join('\n');
    navigator.clipboard.writeText(txt).then(
        () => {{
            hud && hud.done(items.length, false, T.copiados);
            console.log('[TD]', T.copiadosLog);
        }},
        () => {{
            hud && hud.done(items.length, false, T.usaGuardar);
            console.log(txt);
        }}
    );
}}

/** Guarda un JSON con TODOS los metadatos (autor, título, portada) en el
 *  formato que importa la app: Descargas → Importar TXT/JSON.
 *
 *  Es la vía a prueba de balas: no depende del portapapeles (que exige que la
 *  pestaña tenga el foco) ni de que el navegador permita hablar con 127.0.0.1
 *  (Chrome lo restringe con Private Network Access). Descargar un archivo
 *  siempre funciona. */
function tdSaveFile(items) {{
    if (!items || !items.length) return;
    const data = {{ videos: items.map(i => ({{
        id: i.id, author: i.author, title: i.title,
        hqUrl: i.url, pageUrl: i.pageUrl, thumb: i.thumb || ''
    }})) }};
    const blob = new Blob([JSON.stringify(data, null, 2)], {{ type: 'application/json' }});
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `todo-downloader-${{location.hostname.replace(/^www\./, '')}}-${{items.length}}.json`;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => {{ URL.revokeObjectURL(a.href); a.remove(); }}, 5000);
    console.log(`[TD] 💾 ${{T.jsonGuardado}} ${{items.length}} ${{T.elementos}}`);
}}
"#
    )
}

/// Script para perfiles de TikTok (interceptor de API + auto-scroll)
pub fn tiktok(port: u16) -> String {
    let rotulo = m("Capturador de TikTok", "TikTok capturer");
    let donde = m("Ejecutar en", "Run this on");
    let consola = m("Consola", "Console");
    format!(
        r#"/* Todo Downloader — {rotulo} — By Eric V. Gramunt
   {donde} https://www.tiktok.com/@usuario (F12 → {consola}) */
(() => {{
{sender}
const items = new Map();
// Muy amplio a propósito: TikTok cambia rutas con frecuencia. Antes solo se
// miraban 3 endpoints exactos y bastaba un renombrado suyo para no capturar nada.
const API = /item_list|\/api\/(post|repost|favorite|item|search)|aweme|\/feed/;
const diag = {{ hits: 0, scanned: 0, added: 0, noUrl: 0 }};

/** Saca la primera URL utilizable de un campo que puede venir en varias formas:
 *  string suelto, {{url_list:[…]}}, {{urlList:[…]}} o un array de strings. */
function firstUrl(x) {{
    if (!x) return '';
    if (typeof x === 'string') return x.startsWith('http') ? x : '';
    if (Array.isArray(x)) return firstUrl(x[0]);
    return firstUrl(x.url_list || x.urlList || x.url || x.playAddr || '');
}}

function add(it) {{
    diag.scanned++;
    if (!it || !it.id || items.has(it.id)) return;
    const v = it.video || {{}};
    const author = (it.author && (it.author.uniqueId || it.author.unique_id))
                || (location.pathname.match(/@([^/]+)/) || [])[1] || '';

    // Máxima calidad: mayor bitrate de bitrateInfo; si no, playAddr/downloadAddr
    const q = (v.bitrateInfo || v.bitrate_info || [])
        .map(b => ({{ br: b.Bitrate || b.bit_rate || 0,
                     u: firstUrl((b.PlayAddr || b.play_addr)) }}))
        .filter(x => x.u).sort((a, b) => b.br - a.br);

    let url = q.length ? q[0].u
            : firstUrl(v.playAddr) || firstUrl(v.play_addr)
              || firstUrl(v.downloadAddr) || firstUrl(v.download_addr);

    // Post de imágenes (carrusel): TikTok los sirve en imagePost
    const imgs = (it.imagePost && it.imagePost.images) || (it.image_post_info && it.image_post_info.images) || [];
    if (!url && imgs.length) {{
        imgs.forEach((im, n) => {{
            const iu = firstUrl(im.imageURL || im.display_image || im.owner_watermark_image || im);
            if (!iu) return;
            const key = it.id + '_' + n;
            if (items.has(key)) return;
            items.set(key, {{ id: key, author, title: it.desc || '', url: iu, thumb: iu,
                pageUrl: `https://www.tiktok.com/@${{author}}/photo/${{it.id}}` }});
            diag.added++;
        }});
        return;
    }}

    if (!url) {{ diag.noUrl++; return; }}
    if (url.startsWith('http://')) url = 'https://' + url.slice(7);
    const thumb = firstUrl(v.cover) || firstUrl(v.originCover) || firstUrl(v.origin_cover) || firstUrl(v.dynamicCover);
    items.set(it.id, {{ id: it.id, author, title: it.desc || '', url, thumb,
        pageUrl: `https://www.tiktok.com/@${{author}}/video/${{it.id}}` }});
    diag.added++;
}}

/** Recorre CUALQUIER JSON buscando objetos que parezcan publicaciones.
 *  Mucho más resistente que asumir `itemList`: si TikTok renombra el contenedor,
 *  esto los sigue encontrando por su forma (id + video/imagePost). */
function harvest(o, depth) {{
    if (!o || typeof o !== 'object' || (depth || 0) > 6) return;
    if (Array.isArray(o)) {{ o.forEach(x => harvest(x, (depth || 0) + 1)); return; }}
    if (o.id && (o.video || o.imagePost || o.image_post_info || o.desc !== undefined)) {{
        add(o);
    }}
    for (const k in o) {{
        const v = o[k];
        if (v && typeof v === 'object') harvest(v, (depth || 0) + 1);
    }}
}}

function ingest(d) {{
    if (!d) return;
    diag.hits++;
    const list = d.itemList || d.items || d.aweme_list
              || (d.itemInfo && d.itemInfo.itemStruct ? [d.itemInfo.itemStruct] : null);
    if (list && list.length) list.forEach(add);
    else harvest(d, 0);   // formato desconocido: búsqueda por forma
}}

// Interceptar fetch y XHR
if (!window.__td_hooked) {{
    window.__td_hooked = true;
    const of = window.fetch;
    window.fetch = async function (...a) {{
        const r = await of.apply(this, a);
        try {{
            const u = typeof a[0] === 'string' ? a[0] : (a[0] && a[0].url) || '';
            if (API.test(u)) r.clone().json().then(ingest).catch(() => {{}});
        }} catch (e) {{}}
        return r;
    }};
    const oo = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (m, u, ...r) {{ this.__u = u; return oo.call(this, m, u, ...r); }};
    const os = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function (...a) {{
        this.addEventListener('load', () => {{
            try {{ if (API.test(this.__u || '')) ingest(JSON.parse(this.responseText)); }} catch (e) {{}}
        }});
        return os.apply(this, a);
    }};
}}

// Datos ya embebidos en la página
try {{
    const el = document.getElementById('__UNIVERSAL_DATA_FOR_REHYDRATION__');
    if (el) {{
        const s = JSON.parse(el.textContent)['__DEFAULT_SCOPE__'] || {{}};
        const d = s['webapp.video-detail'];
        if (d && d.itemInfo && d.itemInfo.itemStruct) add(d.itemInfo.itemStruct);
    }}
}} catch (e) {{}}

// Auto-scroll hasta agotar el perfil
(async () => {{
    const hud = tdHud();
    hud.sub(T.desplazando);
    console.log('[TD]', T.iniciada);
    let idle = 0, last = 0;
    window.__tdStop = false;
    while (!window.__tdStop && idle < 8) {{
        window.scrollTo(0, document.body.scrollHeight);
        await new Promise(r => setTimeout(r, 1400));
        if (items.size === last) {{
            idle++;
            // Diagnóstico: distingue «no llegan respuestas» de «llegan pero no
            // se extrae nada». Sin esto era imposible saber qué falla.
            hud.msg(diag.hits === 0
                ? `${{T.sinApi}} (${{idle}}/8) — ${{T.scrollBloqueado}}`
                : `API: ${{diag.hits}} · ${{T.analizados}}: ${{diag.scanned}} · ${{T.sinUrl}}: ${{diag.noUrl}} (${{idle}}/8)`);
        }} else {{
            idle = 0;
            last = items.size;
            hud.msg(`API: ${{diag.hits}} · ${{T.capturados}}: ${{diag.added}}`);
        }}
        hud.n(items.size);
    }}
    const arr = [...items.values()];
    window.__tdItems = arr;   // lo usa el botón «💾 Guardar JSON»
    console.log('[TD]', T.diagnostico, diag);
    if (!arr.length) {{
        hud.done(0, false, diag.hits === 0
            ? T.apiNoResp
            : `${{T.respRecibidas}} (${{diag.hits}}) ${{T.sinUtiles}}`);
        return;
    }}
    hud.sub(T.enviando);
    if (!(await tdSend(arr, hud))) tdFallbackCopy(arr, hud);
}})();
}})();
"#,
        sender = sender(port)
    )
}

/// Script para perfiles de Douyin — vídeos e imágenes (API directa, como el
/// script original que ya funciona: Douyin no firma estas peticiones)
pub fn douyin(port: u16) -> String {
    let rotulo = m("Capturador de Douyin (vídeos + imágenes)", "Douyin capturer (videos + images)");
    let donde = m("Ejecutar en", "Run this on");
    let consola = m("Consola", "Console");
    format!(
        r#"/* Todo Downloader — {rotulo} — By Eric V. Gramunt
   {donde} https://www.douyin.com/user/... (F12 → {consola}) */
(() => {{
{sender}
const SEC = location.pathname.replace('/user/', '').split('?')[0];
const Q = 100;                 // calidad de imagen (q100 = máxima)
const WANT_VIDEOS = true;
const WANT_IMAGES = true;

const items = [];
const seen = new Set();
const posts = new Set();   // publicaciones distintas (un carrusel = 1 publicación)
let vids = 0, imgs = 0;

async function api(cursor) {{
    const u = `https://www.douyin.com/aweme/v1/web/aweme/post/?device_platform=webapp&aid=6383`
        + `&channel=channel_pc_web&sec_user_id=${{SEC}}&max_cursor=${{cursor}}&count=20`
        + `&version_code=170400&version_name=17.4.0`;
    const r = await fetch(u, {{ credentials: 'include', headers: {{ accept: 'application/json, text/plain, */*' }} }});
    if (!r.ok) throw new Error('HTTP ' + r.status);
    return r.json();
}}

// Fuerza la máxima calidad en las URL de imagen de Douyin.
// Cubre las tres variantes que usa su CDN: ":q75" en la ruta, "&quality=75"
// y el parámetro suelto "?q=75" / "&q=75".
// NO se tocan los parámetros de tamaño (w/h): poner 0 no devuelve el original,
// hace que el CDN rechace la petición.
function hq(url) {{
    return url
        .replace(/:q\d+/g, ':q' + Q)
        .replace(/([?&])quality=\d+/g, '$1quality=' + Q)
        .replace(/([?&])q=\d+/g, '$1q=' + Q);
}}

function collect(aw) {{
    const id = aw.aweme_id;
    const author = (aw.author && (aw.author.nickname || aw.author.unique_id)) || 'douyin';
    const title = (aw.desc || '').slice(0, 60);

    posts.add(id);

    // Post de imágenes (carrusel: cada foto es un archivo aparte)
    if (WANT_IMAGES && Array.isArray(aw.images) && aw.images.length) {{
        aw.images.forEach((img, n) => {{
            let u = (img.download_url_list && img.download_url_list[0])
                 || (img.url_list && img.url_list[0]) || '';
            if (!u) return;
            u = hq(u);
            const key = id + '_' + n;
            if (seen.has(key)) return;
            seen.add(key);
            // Miniatura: la última entrada de url_list suele ser la vista previa
            // webp ligera (la original iría por download_url_list)
            const thumb = (img.url_list && img.url_list[img.url_list.length - 1]) || '';
            items.push({{ id: key, author, title, url: u, thumb, pageUrl: `https://www.douyin.com/note/${{id}}` }});
            imgs++;
        }});
        return;
    }}

    // Vídeo
    if (WANT_VIDEOS && aw.video) {{
        const list = (aw.video.play_addr && aw.video.play_addr.url_list)
                  || (aw.video.download_addr && aw.video.download_addr.url_list) || [];
        let u = list[list.length - 1] || '';
        if (!u || seen.has(id)) return;
        seen.add(id);
        if (u.startsWith('http://')) u = 'https://' + u.slice(7);
        const thumb = (aw.video.cover && aw.video.cover.url_list && aw.video.cover.url_list[0]) || '';
        items.push({{ id, author, title, url: u, thumb, pageUrl: `https://www.douyin.com/video/${{id}}` }});
        vids++;
    }}
}}

(async () => {{
    if (!SEC || !location.pathname.includes('/user/')) {{
        alert(T.ejecutaDouyin);
        return;
    }}
    const hud = tdHud();
    hud.sub(T.leyendo);
    console.log('[TD]', T.iniciada);

    let cursor = 0, more = true, fails = 0, pages = 0;
    window.__tdStop = false;
    while (more && !window.__tdStop && fails < 5) {{
        try {{
            const d = await api(cursor);
            if (!d || !d.aweme_list) {{
                fails++;
                hud.msg(`${{T.vacia}} (${{fails}}/5)`);
                await new Promise(r => setTimeout(r, 2000));
                continue;
            }}
            fails = 0;
            pages++;
            d.aweme_list.forEach(collect);
            more = d.has_more === 1;
            cursor = d.max_cursor;
            hud.n(items.length);
            hud.lbl(T.archivos);
            hud.msg(`${{posts.size}} ${{T.publicaciones}} · ${{vids}} ${{T.videos}} · ${{imgs}} ${{T.imagenes}}`);
            await new Promise(r => setTimeout(r, 1500));
        }} catch (e) {{
            fails++;
            hud.msg(`${{T.reintentando}} (${{fails}}/5)…`);
            await new Promise(r => setTimeout(r, 3000));
        }}
    }}
    hud.sub(T.enviando);
    window.__tdItems = items;   // lo usa el botón «💾 Guardar JSON»
    const resumen = `${{posts.size}} ${{T.publicaciones}} → ${{vids}} ${{T.videos}} + ${{imgs}} ${{T.imagenes}}`;
    if (await tdSend(items, hud)) {{
        hud.msg(resumen);
    }} else {{
        tdFallbackCopy(items, hud);
    }}
    console.log(`[TD] ${{resumen}} = ${{items.length}} archivos`);
}})();
}})();
"#,
        sender = sender(port)
    )
}

/// Script para V2PH: el NAVEGADOR recorre el álbum y la app solo descarga.
///
/// POR QUÉ EXISTE: V2PH pasó a rechazar con 403 las peticiones de la
/// aplicación aunque llevaran sesión y las cabeceras correctas, mientras el
/// navegador seguía entrando sin problema. Eso ocurre por debajo de las
/// cabeceras —en la huella del handshake TLS—, así que no hay cabecera ni
/// cookie que lo arregle desde fuera.
///
/// Aquí las peticiones las hace la pestaña donde el usuario ya está: su
/// sesión, su IP y su huella. No imita a un navegador, ES el navegador. De
/// paso resuelve el muro de las 10 fotos, porque la sesión va incluida.
///
/// La aplicación solo se queda con la descarga de `cdn.v2ph.com`, que no está
/// protegido. Si algún día también lo estuviera, queda el botón de guardar
/// JSON para importarlo a mano.
pub fn v2ph(port: u16) -> String {
    format!(
        r#"{cabecera}
(async () => {{
    const hud = tdHud();
    hud.msg(T.analizandoPag);

    const RETARDO = 900;      // ms entre páginas: el navegador también es un cliente
    const MAX_ALBUMES = 12;   // tope al recorrer un listado
    const MAX_PAGINAS = 60;   // salvaguarda contra una paginación rota

    const dormir = ms => new Promise(r => setTimeout(r, ms));

    // Descarga una página del sitio y la convierte en documento
    async function pedir(url) {{
        const r = await fetch(url, {{ credentials: 'include' }});
        if (!r.ok) throw new Error('HTTP ' + r.status + ' en ' + url);
        return new DOMParser().parseFromString(await r.text(), 'text/html');
    }}

    // Última página según los enlaces de paginación: se toma el máximo en vez
    // de buscar el rótulo «Último», que está traducido a diez idiomas.
    function ultimaPagina(doc) {{
        let max = 1;
        doc.querySelectorAll('a[href*="page="]').forEach(a => {{
            const m = a.getAttribute('href').match(/[?&]page=(\d+)/);
            if (m) max = Math.max(max, parseInt(m[1], 10));
        }});
        return Math.min(max, MAX_PAGINAS);
    }}

    // Las fotos del álbum viven en /photos/. Las portadas de «galerías
    // relacionadas» están en /album/, otra ruta, y por eso no se cuelan.
    function fotosDe(doc) {{
        return [...doc.querySelectorAll('img[src*="cdn.v2ph.com/photos/"]')]
            .map(i => i.src);
    }}

    function tituloDe(doc) {{
        const og = doc.querySelector('meta[property="og:title"]');
        return (og && og.content) || (doc.title || '').replace(/ - V2PH$/, '');
    }}

    function modeloDe(doc) {{
        const a = doc.querySelector('a[href*="/actor/"]');
        if (!a) return '';
        return (a.textContent || '').trim()
            || (a.getAttribute('href').match(/\/actor\/([^/.]+)/) || [])[1] || '';
    }}

    /** Recorre TODAS las páginas internas de un álbum. */
    async function album(base, aviso) {{
        const id = (base.match(/\/album\/([^/?#]+)/) || [])[1] || 'v2ph';
        const doc1 = await pedir(base);
        const titulo = tituloDe(doc1);
        const autor = modeloDe(doc1) || id;
        const ultima = ultimaPagina(doc1);

        const vistas = new Set();
        const fotos = [];
        for (const u of fotosDe(doc1)) if (!vistas.has(u)) {{ vistas.add(u); fotos.push(u); }}

        for (let p = 2; p <= ultima; p++) {{
            aviso(`${{titulo.slice(0, 40)}} — ${{T.pagina}} ${{p}}/${{ultima}} (${{fotos.length}} ${{T.fotos}})`);
            await dormir(RETARDO);
            let doc;
            try {{ doc = await pedir(base.split('?')[0] + '?page=' + p); }}
            catch (e) {{ console.warn('[TD]', e.message); break; }}
            const nuevas = fotosDe(doc).filter(u => !vistas.has(u));
            // Sin fotos nuevas = fin del álbum, aunque la paginación prometiera más
            if (!nuevas.length) break;
            nuevas.forEach(u => {{ vistas.add(u); fotos.push(u); }});
        }}

        const total = fotos.length;
        return fotos.map((u, i) => ({{
            url: u,
            author: autor,
            title: `${{titulo}} (${{i + 1}}/${{total}})`,
            pageUrl: base,
            id: `${{id}}_${{String(i + 1).padStart(3, '0')}}`,
            thumb: u
        }}));
    }}

    try {{
        const ruta = location.pathname;
        let items = [];

        if (/^\/album\//.test(ruta)) {{
            items = await album(location.origin + ruta, m => hud.msg(m));
        }} else if (/^\/(actor|company|category|country)\//.test(ruta)) {{
            // Listado: se recorren los álbumes de ESTA página, no del sitio entero
            const enlaces = [...new Set(
                [...document.querySelectorAll('a[href*="/album/"]')]
                    .map(a => a.href.split('?')[0])
            )].slice(0, MAX_ALBUMES);

            if (!enlaces.length) throw new Error(T.sinAlbumes);
            hud.msg(`${{enlaces.length}} ${{T.albumesPag}}`);

            for (let k = 0; k < enlaces.length; k++) {{
                hud.msg(`${{T.album}} ${{k + 1}}/${{enlaces.length}}…`);
                try {{
                    items = items.concat(await album(enlaces[k], m => hud.msg(`[${{k + 1}}/${{enlaces.length}}] ${{m}}`)));
                }} catch (e) {{
                    console.warn('[TD]', T.albumOmitido, e.message);
                }}
                hud.n(items.length);
                await dormir(RETARDO);
            }}
        }} else {{
            throw new Error(T.abreAlbum);
        }}

        if (!items.length) throw new Error(T.sinFotos);
        hud.n(items.length);

        const ok = await tdSend(items, hud);
        if (!ok) {{ tdFallbackCopy(items, hud); tdSaveFile(items); }}
    }} catch (e) {{
        hud.msg('❌ ' + e.message);
        console.error('[TD]', e);
    }}
}})();
"#,
        cabecera = sender(port)
    )
}

// ============================================================================
//  Captura de un post suelto (Douyin y TikTok)
// ============================================================================

/// Cuerpo compartido del capturador de un post suelto.
///
/// POR QUÉ NO SE LLAMA A LA API: la de Douyin va firmada (X-Bogus, msToken) y
/// pedirla desde fuera devuelve vacío. El capturador de perfiles lo esquiva
/// interceptando las respuestas mientras haces scroll, pero para un post ya
/// abierto esa respuesta YA PASÓ. Así que se lee lo que la página tiene
/// delante, que además resultó ser más completo de lo esperado.
///
/// CÓMO SE IDENTIFICAN LAS DIAPOSITIVAS, medido sobre el DOM real de Douyin:
///
/// Cada diapositiva se pinta dos veces. Una pequeña y centrada, que es la que
/// se ve, y otra a ANCHO COMPLETO por detrás, desenfocada, que hace de fondo.
/// El visor es un feed vertical de posts y cada uno ocupa una franja:
///
/// ```text
///     2100x415 @ (0, -415)     post anterior
///     2100x415 @ (0, 0)        ESTE post, diapositiva 1
///     2100x415 @ (2100, 0)     ESTE post, diapositiva 2
///     2100x415 @ (0, +415)     post siguiente
/// ```
///
/// Las diapositivas de un mismo post comparten `top` y se separan en `left`;
/// los posts distintos se separan en `top`. Con eso se identifica el post
/// entero sin depender de nombres de clase (ofuscados), sin pulsar flechas y
/// sin esperar a que cargue nada: ya está todo en la página.
///
/// Lo que hubo antes —recorrer el carrusel a golpe de clic— nunca funcionó:
/// el botón no se encontraba porque su clase cambia en cada despliegue, y
/// aunque se hubiera encontrado habría sido dar un rodeo para llegar a algo
/// que ya estaba ahí. Se descubrió volcando el DOM en vez de suponiéndolo.
///
/// LOS VÍDEOS NO SE EXTRAEN. En un post de vídeo el `<video>` expone un
/// `blob:` de Media Source Extensions, que solo existe dentro de esa pestaña.
/// Y lo que SÍ tiene URL en la página es la música (`ies-music/….mp3`) y los
/// vídeos de otros posts del feed: cogerlos sería descargar cualquier cosa
/// menos lo que se pidió. Se le pasa el post a yt-dlp, que para eso está.
///
/// LA CALIDAD NO SE RESUELVE AQUÍ. Douyin sirve el `~noop` —el original sin
/// marca de agua ni recompresión— directamente en el DOM, así que se coge tal
/// cual. Y para lo que no lo traiga, `quality_variants()` en la aplicación ya
/// lo intenta. Repetir esa lógica en JavaScript sería tener dos sitios donde
/// equivocarse.
///
/// Los comentarios DENTRO del script van en inglés y son cortos a propósito:
/// es un archivo que el usuario pega en su navegador, no código que nadie
/// vaya a mantener desde ahí. El razonamiento vive aquí, que es donde se lee.
/// Núcleo del capturador de Threads, sin el envoltorio del `sender`.
///
/// POR QUÉ ESTE SITIO NO SE PUEDE RESOLVER DESDE LA APLICACIÓN: no existe
/// extractor de Threads, ni en gallery-dl ni en yt-dlp (su incidencia #7523
/// lleva abierta desde 2023). Y lo que impide escribir uno es que **los enlaces
/// del CDN de Meta van firmados**: no se puede coger la miniatura que se ve y
/// reescribirla al original, como sí se hace con `~tplv-…` → `~noop` en los CDN
/// de ByteDance. La URL a máxima calidad solo existe dentro de la respuesta
/// JSON, y esa respuesta la pide la propia página.
///
/// POR QUÉ INTERCEPTAR EN VEZ DE PEDIR: reconstruir la petición exigiría el
/// `doc_id`, el `lsd` y el `X-IG-App-ID` de Meta, y el `doc_id` cambia con cada
/// despliegue suyo. Un extractor así se rompe sin avisar y el fallo parece «el
/// perfil está vacío». Leyendo lo que la página ya recibió, el script no
/// construye ninguna petición: cuando Meta cambia su API, cambia su cliente con
/// ella y esto sigue funcionando. La sesión, la IP y la huella TLS son las del
/// usuario, que es la única forma honesta de mirar contenido que su cuenta ya ve.
///
/// SEPARADO DEL ENVOLTORIO, igual que `post_core()`, para que el userscript
/// pueda incluirlo junto al capturador de posts sin duplicar el HUD ni el envío.
fn threads_core() -> &'static str {
    r#"
// Estado global: el interceptor puede llevar puesto desde que cargó la página
// y la captura empezar mucho después.
const TD_TH = { items: new Map(), diag: { hits: 0, nodes: 0, sinUrl: 0 }, hooked: false };

// Threads pagina contra /graphql/query. `bulk-route-definitions` y
// `/ajax/navigation` también pasan por ahí, pero no llevan medios: el filtro de
// contenido de `tdThIngest` los descarta solos.
const TD_TH_API = /\/graphql\/query|\/api\/v1\//;

/** Cuenta que se está mirando AHORA.
 *
 *  Se lee en cada captura y no una sola vez: Threads es una SPA y navegar de un
 *  perfil a otro no recarga la página, así que un valor cacheado al instalar el
 *  script apuntaría a la cuenta equivocada. */
function tdThHandle() {
    return ((location.pathname.match(/@([^/?#]+)/) || [])[1] || '').toLowerCase();
}

/** Ordena un array de {width,height,url} y devuelve el mayor o el menor.
 *  Por ÁREA y no por anchura: un vertical de 1080×1350 y uno de 1080×1080
 *  empatan en anchura y no son el mismo archivo. */
function tdThArea(arr, mayor) {
    const c = (Array.isArray(arr) ? arr : []).filter(
        x => x && typeof x.url === 'string' && x.url.startsWith('http'));
    c.sort((a, b) => {
        const A = (a.width || 0) * (a.height || 0), B = (b.width || 0) * (b.height || 0);
        return mayor ? B - A : A - B;
    });
    return c[0] || null;
}

/** Un archivo suelto: o el nodo de una publicación simple, o un elemento de un
 *  carrusel. */
function tdThAdd(n, code, autor, texto, idx, total) {
    if (!n || typeof n !== 'object') return;
    TD_TH.diag.nodes++;
    const base = String(n.pk || n.id || '');
    if (!base) return;
    const id = total > 1 ? base + '_' + idx : base;
    if (TD_TH.items.has(id)) return;

    const cands = n.image_versions2 && n.image_versions2.candidates;
    const img = tdThArea(cands, true);
    const mini = tdThArea(cands, false);
    const vid = tdThArea(n.video_versions, true);

    // `video_versions` vacío es lo normal en una imagen: en las respuestas de
    // Threads TODO nodo de medios lleva las dos claves.
    const mejor = vid || img;
    if (!mejor) { TD_TH.diag.sinUrl++; return; }

    TD_TH.items.set(id, {
        id,
        author: autor,
        title: texto,
        url: mejor.url,
        // La miniatura es el candidato MÁS PEQUEÑO a propósito: la rejilla
        // pinta 320 px y descargar el original de 1440 para eso serían decenas
        // de megas por un perfil.
        thumb: (mini || img || {}).url || '',
        w: mejor.width || n.original_width || 0,
        h: mejor.height || n.original_height || 0,
        video: !!vid,
        pageUrl: code ? 'https://www.threads.com/@' + autor + '/post/' + code : location.href
    });
}

/** Una publicación: puede ser un archivo o un carrusel.
 *
 *  NO filtra por cuenta aquí. El filtro va en el envío porque el interceptor
 *  sigue puesto mientras navegas: lo que se capturó en un perfil no debe
 *  perderse por abrir otro, y lo que se manda debe ser solo el que miras. */
function tdThPost(p) {
    const autor = String((p.user && (p.user.username || p.user.pk)) || tdThHandle() || '');
    const code = p.code || p.shortcode || '';
    const texto = (p.caption && p.caption.text) || p.accessibility_caption || '';
    const car = p.carousel_media;
    if (Array.isArray(car) && car.length) {
        car.forEach((c, i) => tdThAdd(c, code, autor, texto, i, car.length));
    } else {
        tdThAdd(p, code, autor, texto, 0, 1);
    }
}

/** Recorre CUALQUIER JSON buscando publicaciones por su FORMA.
 *
 *  Deliberadamente no se asume la ruta (`data.mediaData.edges[].node…`): esa
 *  ruta es de Meta y la cambia cuando quiere. Lo que no cambia es que una
 *  publicación lleva medios Y algo que la identifica —code, caption o user—.
 *  Los hijos de un carrusel no llevan ninguna de las tres, por eso no se
 *  procesan dos veces. Y una foto de perfil no lleva `image_versions2`, por eso
 *  los avatares no acaban en la rejilla. */
function tdThHarvest(o, d) {
    if (!o || typeof o !== 'object' || (d || 0) > 14) return;
    if (Array.isArray(o)) { for (const x of o) tdThHarvest(x, (d || 0) + 1); return; }
    if ((o.image_versions2 || o.video_versions || o.carousel_media)
        && (o.code || o.caption || o.user || o.carousel_media)) {
        tdThPost(o);
        return;   // no se baja a los hijos: ya los ha visto `tdThPost`
    }
    for (const k in o) {
        const v = o[k];
        if (v && typeof v === 'object') tdThHarvest(v, (d || 0) + 1);
    }
}

function tdThIngest(txt) {
    if (!txt || txt.indexOf('image_versions2') < 0) return;
    TD_TH.diag.hits++;
    // Threads responde a veces con varios JSON separados por saltos de línea.
    for (const linea of txt.split('\n')) {
        const t = linea.trim();
        if (!t || t[0] !== '{') continue;
        try { tdThHarvest(JSON.parse(t), 0); } catch (e) {}
    }
}

/** Instala el interceptor. Idempotente: en el userscript se llama al cargar y
 *  otra vez al pulsar el botón. */
function tdThHook() {
    if (TD_TH.hooked) return;
    TD_TH.hooked = true;
    const of = window.fetch;
    window.fetch = async function (...a) {
        const r = await of.apply(this, a);
        try {
            const u = typeof a[0] === 'string' ? a[0] : (a[0] && a[0].url) || '';
            if (TD_TH_API.test(u)) r.clone().text().then(tdThIngest).catch(() => {});
        } catch (e) {}
        return r;
    };
    const oo = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (m, u, ...r) { this.__u = u; return oo.call(this, m, u, ...r); };
    const os = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function (...a) {
        this.addEventListener('load', () => {
            try { if (TD_TH_API.test(this.__u || '')) tdThIngest(this.responseText || ''); } catch (e) {}
        });
        return os.apply(this, a);
    };
}

/** Lo que ya venía en el HTML.
 *
 *  Threads deja el arranque de Relay en etiquetas <script type="application/json">
 *  y ahí están ya las primeras publicaciones. Leerlas evita que la captura
 *  empiece en cero y, en la página de UN post, evita tener que recargar. */
function tdThBootstrap() {
    try {
        for (const s of document.querySelectorAll('script[type="application/json"]')) {
            const t = s.textContent || '';
            if (t.length > 2000 && t.indexOf('image_versions2') >= 0) {
                try { tdThHarvest(JSON.parse(t), 0); } catch (e) {}
            }
        }
    } catch (e) {}
}

/** Captura el perfil abierto: auto-scroll —el scroll ES la paginación— y envío
 *  a la rejilla de selección. */
async function tdCapturarThreads() {
    tdThHook();
    tdThBootstrap();

    const handle = tdThHandle();
    if (!handle) console.warn('[TD]', T.thPerfil);

    // El filtro por cuenta no es cosmético: en una sola respuesta de un perfil
    // con cuatro publicaciones llegaron OCHENTA Y UN bloques de medios, porque
    // Threads mezcla recomendaciones de otras cuentas. Sin esto la rejilla se
    // llenaba de archivos que nadie había pedido.
    const mios = () => [...TD_TH.items.values()].filter(
        i => !handle || String(i.author).toLowerCase() === handle);

    const hud = tdHud();
    hud.sub(T.thLeyendo);
    console.log('[TD]', T.iniciada);
    hud.n(mios().length);

    let idle = 0, last = mios().length;
    window.__tdStop = false;
    while (!window.__tdStop && idle < 8) {
        window.scrollTo(0, document.body.scrollHeight);
        await new Promise(r => setTimeout(r, 1500));
        const n = mios().length;
        if (n === last) {
            idle++;
            // Distingue «no llegan respuestas» de «llegan pero no son de esta
            // cuenta». Sin esto era imposible saber cuál de las dos pasa.
            hud.msg(TD_TH.diag.hits === 0
                ? T.sinApi + ' (' + idle + '/8) — ' + T.scrollBloqueado
                : 'API: ' + TD_TH.diag.hits + ' · ' + T.analizados + ': ' + TD_TH.diag.nodes
                  + ' · ' + T.thAjenos + ': ' + (TD_TH.items.size - n) + ' (' + idle + '/8)');
        } else {
            idle = 0;
            last = n;
            hud.msg('API: ' + TD_TH.diag.hits + ' · ' + T.capturados + ': ' + n);
        }
        hud.n(n);
    }

    const arr = mios();
    window.__tdItems = arr;   // lo usa el botón «💾 Guardar JSON»
    console.log('[TD]', T.diagnostico, TD_TH.diag, T.thAjenos, TD_TH.items.size - arr.length);
    if (!arr.length) {
        hud.done(0, false, TD_TH.diag.hits === 0 ? T.apiNoResp : T.thSinMedios);
        return;
    }
    hud.sub(T.enviando);
    // A la REJILLA, no a la cola: de un perfil de Threads se quieren unos
    // cuantos archivos, no los doscientos que salgan.
    if (!(await tdSend(arr, hud, 'select'))) tdFallbackCopy(arr, hud);
}
"#
}

/// Script de consola para perfiles y posts de Threads.
pub fn threads(port: u16) -> String {
    let rotulo = m("Capturador de Threads", "Threads capturer");
    let donde = m("Ejecutar en", "Run this on");
    let consola = m("Consola", "Console");
    format!(
        r#"/* Todo Downloader — {rotulo} — By Eric V. Gramunt
   {donde} https://www.threads.com/@usuario (F12 → {consola}) */
(() => {{
{sender}
{nucleo}
tdCapturarThreads();
}})();
"#,
        sender = sender(port),
        nucleo = threads_core()
    )
}

fn post_core() -> &'static str {
    r#"
const TD_IMG = /(douyinpic|byteimg|ibyteimg|tiktokcdn|bytecdn)\.com\//i;

// Post id, from the URL.
function tdPostId() {
    const u = new URL(location.href);
    const modal = u.searchParams.get('modal_id');
    if (modal) return modal;
    const m = u.pathname.match(/\/(?:note|video|photo)\/(\d+)/);
    return m ? m[1] : '';
}

// Key for one photo, ignoring the CDN processing: `~noop` and `~tplv-…` of the
// same original share this prefix.
function tdBase(u) {
    const i = u.indexOf('~');
    return (i > 0 ? u.slice(0, i) : u.split('?')[0]);
}

// Slides of the open post. Each slide is painted twice: small and centred (the
// one you see) and full width behind it, blurred. Slides of one post share
// `top` and differ in `left`; different posts differ in `top`. `~noop` is the
// unprocessed original and is preferred when present.
function tdSlides() {
    const anchos = [];
    for (const img of document.querySelectorAll('img')) {
        const u = img.currentSrc || img.src || '';
        if (!u || !TD_IMG.test(u)) continue;
        const r = img.getBoundingClientRect();
        if (r.width < innerWidth * 0.85) continue;
        anchos.push({ u, top: r.top, alto: r.height });
    }
    if (!anchos.length) return { urls: [] };

    const cy = innerHeight / 2;
    let ref = anchos.find(o => o.top <= cy && o.top + o.alto >= cy);
    if (!ref) {
        ref = anchos.reduce((a, b) =>
            Math.abs(a.top + a.alto / 2 - cy) < Math.abs(b.top + b.alto / 2 - cy) ? a : b);
    }
    const tol = Math.max(20, ref.alto * 0.3);

    const porFoto = new Map();
    for (const o of anchos) {
        if (Math.abs(o.top - ref.top) > tol) continue;
        const b = tdBase(o.u);
        const previa = porFoto.get(b);
        if (!previa || (o.u.indexOf('~noop') >= 0 && previa.indexOf('~noop') < 0)) {
            porFoto.set(b, o.u);
        }
    }
    return { urls: [...porFoto.values()] };
}

// Is the centre of the window a video player?
function tdEsVideo() {
    const el = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    if (el && el.tagName === 'VIDEO') return true;
    for (const v of document.querySelectorAll('video')) {
        const r = v.getBoundingClientRect();
        if (r.width > innerWidth * 0.15 && r.height > innerHeight * 0.3
            && r.top < innerHeight / 2 && r.bottom > innerHeight / 2) return true;
    }
    return false;
}

// Author and title, for the subfolder and the file name.
function tdMeta() {
    const t = (document.title || '').replace(/\s*[-|]\s*(抖音|TikTok).*$/, '').trim();
    const a = document.querySelector('[class*="account-name"],[data-e2e="user-title"],h1');
    return { autor: (a && a.textContent.trim()) || location.hostname.replace(/^www\./, ''),
             titulo: t.slice(0, 80) };
}

async function tdCapturarPost(modo) {
    const hud = tdHud();
    const id = tdPostId();
    if (!id) { hud.done(0, false, T.postSinId); return; }

    hud.lbl(T.archivos);
    hud.msg(T.postBuscando);

    const meta = tdMeta();
    const canon = h => (location.hostname.indexOf('tiktok') >= 0
        ? 'https://www.tiktok.com/@' + encodeURIComponent(meta.autor) + '/' + h + '/' + id
        : 'https://www.douyin.com/' + h + '/' + id);

    // Video post: the player exposes only a `blob:`, unusable outside this tab.
    // The post URL goes to the application and yt-dlp resolves it. The blurred
    // backdrop is the poster and serves as the thumbnail.
    if (tdEsVideo()) {
        const portadas = tdSlides().urls || [];
        const item = [{ id: id, author: meta.autor, title: meta.titulo,
                        url: canon('video'), pageUrl: canon('video'), thumb: portadas[0] || '' }];
        window.__tdItems = item;
        hud.n(1);
        // Una publicación de vídeo se manda como UNA sola cosa: la URL del post,
        // que resuelve yt-dlp. Si el post tiene varias diapositivas, yt-dlp
        // devuelve solo la primera y el resto se pierde EN SILENCIO. No se sabe
        // arreglar todavía, pero callarlo es peor: el panel decía «1 archivo» y
        // parecía que había ido bien.
        hud.msg(portadas.length > 1
            ? T.postVariosVideos + ' (' + portadas.length + ')'
            : T.postAytdlp);
        if (!(await tdSend(item, hud, modo))) tdFallbackCopy(item, hud);
        return;
    }

    const r = tdSlides();
    if (!r.urls.length) { hud.done(0, false, T.postNada); return; }

    const pagina = canon('note');
    const items = r.urls.map((u, i) => ({
        id: id + '_' + String(i + 1).padStart(2, '0'),
        author: meta.autor,
        title: meta.titulo,
        url: u,
        pageUrl: pagina,
        thumb: u
    }));

    window.__tdItems = items;
    hud.n(items.length);
    hud.msg(items.length + ' ' + T.imagenes);
    if (!(await tdSend(items, hud, modo))) tdFallbackCopy(items, hud);
}

"#
}

/// Userscript para Tampermonkey o Violentmonkey.
///
/// POR QUÉ ES LA VÍA BUENA EN CHROME Y VIVALDI: `GM_xmlhttpRequest` corre en
/// el contexto de la extensión, que no está sujeta a Private Network Access.
/// Una página normal —y por tanto un bookmarklet— no puede hablar con
/// `127.0.0.1` en los navegadores basados en Chromium.
pub fn userscript(port: u16) -> String {
    let nucleo = post_core();
    let threads = threads_core();
    let envio = sender(port);
    let boton = m("Capturar este post", "Capture this post");
    let boton_th = m("Capturar este perfil", "Capture this profile");
    let rotulo = m("captura desde la página", "in-page capture");
    let descripcion = m(
        "Un botón para enviar el post abierto (Douyin, TikTok) o el perfil de Threads a Todo Downloader",
        "A button to send the open post (Douyin, TikTok) or the Threads profile to Todo Downloader",
    );
    format!(
        r#"// ==UserScript==
// @name         Todo Downloader — {rotulo}
// @namespace    https://github.com/AcidClawX41/todo-downloader
// @version      1.2
// @description  {descripcion}
// @match        https://www.douyin.com/*
// @match        https://www.tiktok.com/*
// @match        https://www.threads.com/*
// @match        https://www.threads.net/*
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// @run-at       document-start
// ==/UserScript==
(() => {{
'use strict';
{envio}
{nucleo}
{threads}

const TD_ES_THREADS = /(^|\.)threads\.(com|net)$/i.test(location.hostname);

// En Threads el interceptor se instala YA, antes de que la página pida nada.
// A diferencia de Douyin, aquí no hay nada que leer del DOM: si la respuesta
// pasa sin que estemos escuchando, esos archivos se han perdido hasta recargar.
// Por eso este userscript corre en `document-start` y no en `document-idle`.
if (TD_ES_THREADS) tdThHook();

function tdBoton(texto, color, abajo, accion) {{
    const b = document.createElement('button');
    b.textContent = texto;
    b.style.cssText = [
        'position:fixed', 'right:18px', 'bottom:' + abajo + 'px', 'z-index:2147483646',
        'padding:10px 14px', 'border:0', 'border-radius:10px',
        'background:' + color, 'color:#fff', 'font:13px/1 sans-serif',
        'cursor:pointer', 'box-shadow:0 6px 24px rgba(0,0,0,.45)', 'display:none'
    ].join(';');
    b.onclick = accion;
    return b;
}}

// El <body> puede no existir todavía en `document-start`.
function tdPintar() {{
    if (!document.body) return setTimeout(tdPintar, 50);

    if (TD_ES_THREADS) {{
        // A 96 px del borde y no a 18: Threads tiene su propio botón redondo
        // de «Nuevo hilo» fijo en esa misma esquina, y a 18 px los dos se
        // pisan. Subirlo es más fiable que confiar en el z-index, porque el
        // botón de abajo sigue siendo clicable aunque el nuestro gane.
        const t = tdBoton('⬇ {boton_th}', '#000', 96, () => tdCapturarThreads());
        document.body.appendChild(t);
        // Solo en un perfil o un post, no en el inicio ni en Buscar.
        setInterval(() => {{
            t.style.display = location.pathname.indexOf('/@') >= 0 ? 'block' : 'none';
        }}, 800);
        return;
    }}

    // Douyin y TikTok: se pinta siempre y se activa solo cuando hay un post
    // abierto, porque ahí la navegación tampoco recarga la página.
    const b = tdBoton('⬇ {boton}', '#FE2C55', 18, () => tdCapturarPost('post'));
    document.body.appendChild(b);
    setInterval(() => {{ b.style.display = tdPostId() ? 'block' : 'none'; }}, 800);
}}
tdPintar();
}})();
"#
    )
}

/// Bookmarklet: una sola línea `javascript:` para arrastrar a la barra.
///
/// LO QUE NO PUEDE HACER: corre dentro de la página, así que en Chrome y
/// Vivaldi choca con Private Network Access y no alcanza a `127.0.0.1`. Ahí
/// cae al respaldo de siempre —portapapeles y botón de guardar JSON—, que
/// funciona en todas partes. En Firefox llega directo a la aplicación.
pub fn bookmarklet(port: u16) -> String {
    let cuerpo = format!("{}\n{}\ntdCapturarPost('post');", sender(port), post_core());

    // Un bookmarklet es una URL: los comentarios de línea se comerían todo lo
    // que viniera detrás, y hay que codificar lo que no es seguro en una URL.
    let sin_comentarios: String = cuerpo
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if t.starts_with("//") || t.starts_with("*") || t.starts_with("/*") { "" } else { l }
        })
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let escapado: String = sin_comentarios
        .chars()
        .map(|c| match c {
            '%' => "%25".into(),
            '"' => "%22".into(),
            '#' => "%23".into(),
            '&' => "%26".into(),
            '+' => "%2B".into(),
            '?' => "%3F".into(),
            ' ' => "%20".into(),
            '\n' => "%0A".into(),
            _ => c.to_string(),
        })
        .collect();

    format!("javascript:(()=>{{{escapado}}})();")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los scripts se montan con `format!`, y ahí una llave mal escapada no da
    /// error de compilación: sale un `${id}` literal en el JavaScript y falla
    /// en el navegador del usuario. Ya pasó una vez.
    #[test]
    fn los_scripts_no_dejan_marcadores_sin_resolver() {
        for (nombre, s) in [
            ("tiktok", tiktok(4567)),
            ("douyin", douyin(4567)),
            ("v2ph", v2ph(4567)),
            ("threads", threads(4567)),
            ("userscript", userscript(4567)),
        ] {
            assert!(
                s.contains("const TD_PORT = 4567;"),
                "{nombre}: el puerto no se ha sustituido"
            );
            for marcador in ["{sender}", "{rotulo}", "{donde}", "{consola}", "{port}", "{dic}"] {
                assert!(
                    !s.contains(marcador),
                    "{nombre}: ha quedado el marcador {marcador} sin resolver"
                );
            }
        }
    }

    /// Lo que decide la calidad en Threads: los enlaces del CDN van firmados,
    /// así que la URL del original solo puede salir de los arrays de la API.
    /// Si alguna de estas piezas desaparece, el script deja de traer originales
    /// sin dar ningún error visible.
    #[test]
    fn el_script_de_threads_lee_los_arrays_de_calidad() {
        let s = threads(4567);
        for clave in [
            "image_versions2",
            "candidates",
            "video_versions",
            "carousel_media",
            "original_width",
        ] {
            assert!(s.contains(clave), "falta {clave}");
        }
        // Va a la rejilla de selección, no a la cola.
        assert!(s.contains("tdSend(arr, hud, 'select')"));
        // Y manda el tamaño real, que es lo que la rejilla no puede medir.
        assert!(s.contains("w: mejor.width"));
        assert!(s.contains("video: !!vid"));
    }

    /// El filtro por cuenta no es cosmético: en una sola respuesta de un perfil
    /// llegan las publicaciones del perfil Y las recomendaciones de otros.
    ///
    /// Y se resuelve en cada captura, no al instalar: Threads es una SPA, y un
    /// handle leído una sola vez apuntaría a la cuenta anterior en cuanto
    /// navegues de un perfil a otro sin recargar.
    #[test]
    fn el_script_de_threads_filtra_por_la_cuenta_abierta() {
        let s = threads(4567);
        assert!(s.contains("function tdThHandle()"));
        assert!(s.contains("String(i.author).toLowerCase() === handle"));
    }

    /// En Threads no hay nada que leer del DOM: si una respuesta pasa sin que
    /// el interceptor esté puesto, esos archivos se pierden hasta recargar.
    /// Por eso el userscript engancha antes de que la página pida nada.
    #[test]
    fn el_userscript_cubre_threads_desde_el_arranque() {
        let s = userscript(4567);
        assert!(s.contains("@match        https://www.threads.com/*"));
        assert!(s.contains("@match        https://www.threads.net/*"));
        assert!(s.contains("@run-at       document-start"));
        assert!(s.contains("if (TD_ES_THREADS) tdThHook();"));
        // Y sigue trayendo el capturador de posts de Douyin/TikTok.
        assert!(s.contains("tdCapturarPost('post')"));
        assert!(s.contains("tdCapturarThreads()"));
    }

    /// Los textos del script los pinta el navegador, así que se resuelven al
    /// generarlo. Una cadena en español fija dentro del JavaScript no la
    /// detecta ninguna revisión de la interfaz: ya había una así.
    #[test]
    fn los_textos_del_script_siguen_el_idioma() {
        use crate::i18n::{set_lang, Lang};
        set_lang(Lang::En);
        let en = threads(4567);
        set_lang(Lang::Es);
        let es = threads(4567);
        set_lang(Lang::default()); // el idioma es global: se deja como estaba
        assert!(en.contains("Scrolling the profile"), "falta el texto en inglés");
        assert!(es.contains("Desplazando el perfil"), "falta el texto en español");
        assert_ne!(en, es);
    }
}
