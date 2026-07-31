/**
 * TikTok Video Downloader HQ - By Eric V. Gramunt
 * Conversión del script de DouYin para perfiles y vídeos de TikTok.
 *
 * DIFERENCIA CLAVE vs DouYin:
 * TikTok firma sus peticiones API (X-Bogus / msToken), por lo que llamar al
 * endpoint directamente desde la consola devuelve respuestas vacías.
 * Este script usa un INTERCEPTOR: captura las respuestas reales de la API
 * mientras hace auto-scroll del perfil. 100% fiable, sin firmas.
 *
 * USO:
 *   1. Abre https://www.tiktok.com/@usuario  (o un vídeo individual)
 *   2. Pega este script en la consola (F12) y pulsa Enter
 *   3. Espera al auto-scroll. Al terminar descarga:
 *        - TXT  -> enlaces directos HQ (pegar en JDownloader2 o en la GUI)
 *        - JSON -> metadatos completos (usado por TikTok Downloader GUI.pyw)
 *
 * Comandos manuales:
 *   downloader.stop()    - Detener el scroll y exportar lo capturado
 *   downloader.export()  - Volver a exportar
 */

class TikTokDownloader {
    constructor(options = {}) {
        this.username = (location.pathname.match(/@([^/]+)/) || [])[1] || 'unknown';
        this.items = new Map(); // id -> videoInfo (dedupe)
        this.stats = {
            captured: 0,
            failedExtractions: 0,
            apiResponses: 0,
            startTime: new Date()
        };
        this.running = false;
        this._idleRounds = 0;

        this.config = {
            scrollDelay: options.scrollDelay || 1400,   // ms entre scrolls
            maxIdleRounds: options.maxIdleRounds || 8,  // rondas sin items nuevos antes de parar
            maxVideos: options.maxVideos || 0,          // 0 = sin límite
            exportTxt: options.exportTxt !== false,
            exportJson: options.exportJson !== false,
            preferNoWatermark: options.preferNoWatermark !== false,
            dateFilter: options.dateFilter || null,     // {startDate, endDate}
            debugMode: options.debugMode || false,
            ...options
        };
    }

    log(message, level = 'info') {
        const prefix = { info: '📘', success: '✅', warning: '⚠️', error: '❌', debug: '🔍' }[level] || '📝';
        if (level === 'debug' && !this.config.debugMode) return;
        console.log(`${prefix} [${new Date().toISOString()}] ${message}`);
    }

    validatePage() {
        if (!/tiktok\.com$/.test(location.hostname.replace(/^www\./, '')) && !location.hostname.includes('tiktok.com')) {
            throw new Error('Este script debe ejecutarse en tiktok.com');
        }
        if (!location.pathname.includes('@')) {
            throw new Error('Abre un perfil (https://www.tiktok.com/@usuario) o un vídeo individual');
        }
        this.isVideoPage = /\/video\/\d+/.test(location.pathname);
        this.log(`Usuario detectado: @${this.username} (${this.isVideoPage ? 'vídeo individual' : 'perfil'})`, 'success');
    }

    /* ==================== INTERCEPTOR ==================== */

    installHooks() {
        if (window.__ttdl_hooked) { this.log('Interceptor ya instalado', 'debug'); return; }
        window.__ttdl_hooked = true;
        const self = this;
        const apiRegex = /\/api\/(post|repost|favorite)\/item_list|\/api\/item\/detail|\/api\/user\/detail/;

        // Hook fetch
        const origFetch = window.fetch;
        window.fetch = async function (...args) {
            const res = await origFetch.apply(this, args);
            try {
                const url = typeof args[0] === 'string' ? args[0] : (args[0] && args[0].url) || '';
                if (apiRegex.test(url)) {
                    res.clone().json().then(d => self.ingest(d)).catch(() => {});
                }
            } catch (e) { /* nunca romper la petición original */ }
            return res;
        };

        // Hook XMLHttpRequest
        const origOpen = XMLHttpRequest.prototype.open;
        XMLHttpRequest.prototype.open = function (method, url, ...rest) {
            this.__ttdl_url = url;
            return origOpen.call(this, method, url, ...rest);
        };
        const origSend = XMLHttpRequest.prototype.send;
        XMLHttpRequest.prototype.send = function (...args) {
            this.addEventListener('load', () => {
                try {
                    if (apiRegex.test(this.__ttdl_url || '')) {
                        self.ingest(JSON.parse(this.responseText));
                    }
                } catch (e) {}
            });
            return origSend.apply(this, args);
        };

        this.log('Interceptor de API instalado', 'success');
    }

    /** Procesa una respuesta JSON de la API */
    ingest(data) {
        if (!data) return;
        this.stats.apiResponses++;
        const list = data.itemList || data.items ||
            (data.itemInfo && data.itemInfo.itemStruct ? [data.itemInfo.itemStruct] : []);
        for (const item of list) this.addItem(item);
    }

    /** Lee los datos embebidos en la página (estado inicial, sin scroll) */
    harvestEmbeddedState() {
        // Formato nuevo: __UNIVERSAL_DATA_FOR_REHYDRATION__
        try {
            const el = document.getElementById('__UNIVERSAL_DATA_FOR_REHYDRATION__');
            if (el) {
                const scope = JSON.parse(el.textContent)['__DEFAULT_SCOPE__'] || {};
                const detail = scope['webapp.video-detail'];
                if (detail && detail.itemInfo && detail.itemInfo.itemStruct) {
                    this.addItem(detail.itemInfo.itemStruct);
                }
            }
        } catch (e) { this.log(`UNIVERSAL_DATA no parseable: ${e.message}`, 'debug'); }

        // Formato antiguo: SIGI_STATE
        try {
            const el = document.getElementById('SIGI_STATE');
            if (el) {
                const state = JSON.parse(el.textContent);
                if (state.ItemModule) {
                    Object.values(state.ItemModule).forEach(item => this.addItem(item));
                }
            }
        } catch (e) { this.log(`SIGI_STATE no parseable: ${e.message}`, 'debug'); }
    }

    /* ==================== EXTRACCIÓN ==================== */

    addItem(item) {
        try {
            if (!item || !item.id || this.items.has(item.id)) return;
            const video = item.video || {};
            const author = (item.author && (item.author.uniqueId || item.author.unique_id)) || this.username;

            // Todas las calidades disponibles
            const qualities = (video.bitrateInfo || []).map(b => ({
                gearName: b.GearName || b.gear_name || '',
                quality: b.QualityType || b.quality_type || 0,
                bitrate: b.Bitrate || b.bitrate || 0,
                codec: b.CodecType || b.codec_type || '',
                width: (b.PlayAddr && b.PlayAddr.Width) || 0,
                height: (b.PlayAddr && b.PlayAddr.Height) || 0,
                dataSize: (b.PlayAddr && b.PlayAddr.DataSize) || 0,
                url: (b.PlayAddr && b.PlayAddr.UrlList && b.PlayAddr.UrlList[b.PlayAddr.UrlList.length - 1]) || ''
            })).filter(q => q.url);

            // HQ = mayor bitrate disponible
            qualities.sort((a, b) => b.bitrate - a.bitrate);
            let hqUrl = qualities.length ? qualities[0].url : '';
            if (!hqUrl) hqUrl = video.playAddr || video.downloadAddr || '';
            if (hqUrl && hqUrl.startsWith('http://')) hqUrl = hqUrl.replace('http://', 'https://');

            if (!hqUrl) { this.stats.failedExtractions++; return; }

            const info = {
                id: item.id,
                author: author,
                title: item.desc || 'Sin título',
                pageUrl: `https://www.tiktok.com/@${author}/video/${item.id}`,
                hqUrl: hqUrl,
                playAddr: video.playAddr || '',
                downloadAddr: video.downloadAddr || '',
                width: video.width || 0,
                height: video.height || 0,
                duration: video.duration || 0,
                createTime: item.createTime ? new Date(item.createTime * 1000).toISOString() : null,
                statistics: {
                    likes: (item.stats && item.stats.diggCount) || 0,
                    comments: (item.stats && item.stats.commentCount) || 0,
                    shares: (item.stats && item.stats.shareCount) || 0,
                    plays: (item.stats && item.stats.playCount) || 0
                },
                qualities: qualities
            };

            if (!this.filterByDate(info)) return;

            this.items.set(item.id, info);
            this.stats.captured = this.items.size;
            this.log(`+ ${item.id} | ${qualities.length} calidades | ${info.title.substring(0, 40)}`, 'debug');
        } catch (error) {
            this.stats.failedExtractions++;
            this.log(`Error extrayendo item: ${error.message}`, 'error');
        }
    }

    filterByDate(info) {
        if (!this.config.dateFilter || !info.createTime) return true;
        const d = new Date(info.createTime);
        const { startDate, endDate } = this.config.dateFilter;
        if (startDate && d < new Date(startDate)) return false;
        if (endDate && d > new Date(endDate)) return false;
        return true;
    }

    sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

    updateProgress() {
        console.log(
            `📊 Capturados: ${this.items.size} | ` +
            `📡 Respuestas API: ${this.stats.apiResponses} | ` +
            `❌ Fallos: ${this.stats.failedExtractions} | ` +
            `⏳ Rondas sin novedades: ${this._idleRounds}/${this.config.maxIdleRounds}`
        );
    }

    /* ==================== AUTO-SCROLL ==================== */

    async autoScroll() {
        this.running = true;
        this._idleRounds = 0;
        let lastCount = this.items.size;

        while (this.running) {
            window.scrollTo(0, document.body.scrollHeight);
            await this.sleep(this.config.scrollDelay);

            if (this.items.size === lastCount) {
                this._idleRounds++;
                // Pequeño scroll arriba/abajo para forzar lazy-load
                if (this._idleRounds % 3 === 0) {
                    window.scrollBy(0, -800);
                    await this.sleep(400);
                    window.scrollTo(0, document.body.scrollHeight);
                }
            } else {
                this._idleRounds = 0;
                lastCount = this.items.size;
            }

            this.updateProgress();

            if (this._idleRounds >= this.config.maxIdleRounds) {
                this.log('Sin items nuevos, finalizando scroll', 'info');
                break;
            }
            if (this.config.maxVideos > 0 && this.items.size >= this.config.maxVideos) {
                this.log(`Límite de ${this.config.maxVideos} vídeos alcanzado`, 'info');
                break;
            }
        }
        this.running = false;
    }

    stop() {
        this.running = false;
        this.log('Detenido manualmente. Exportando...', 'warning');
        this.export();
    }

    /* ==================== EXPORTACIÓN ==================== */

    get videos() { return Array.from(this.items.values()); }

    saveFile(content, filename, mimeType) {
        const blob = new Blob([content], { type: mimeType });
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(a.href);
    }

    export() {
        const videos = this.videos;
        if (!videos.length) { this.log('Nada que exportar', 'warning'); return; }
        const ts = Date.now();

        if (this.config.exportTxt) {
            const txt = videos.map(v => v.hqUrl).filter(Boolean).join('\n');
            this.saveFile(txt, `tiktok_${this.username}_${ts}.txt`, 'text/plain');
            this.log(`TXT guardado (${videos.length} enlaces HQ)`, 'success');
        }

        if (this.config.exportJson) {
            const json = JSON.stringify({
                source: 'tiktok',
                username: this.username,
                export_date: new Date().toISOString(),
                total_videos: videos.length,
                statistics: this.stats,
                videos: videos
            }, null, 2);
            this.saveFile(json, `tiktok_${this.username}_${ts}.json`, 'application/json');
            this.log('JSON guardado (metadatos completos para la GUI)', 'success');
        }
    }

    /* ==================== EJECUCIÓN ==================== */

    async run() {
        try {
            console.log('╔══════════════════════════════════════════════════════════╗');
            console.log('║      TikTok Video Downloader HQ - By Eric V. Gramunt            ║');
            console.log('╚══════════════════════════════════════════════════════════╝');

            this.validatePage();
            this.installHooks();
            this.harvestEmbeddedState();

            if (this.isVideoPage) {
                // Vídeo individual: los datos ya están embebidos o llegan solos
                await this.sleep(2500);
            } else {
                this.log('Iniciando auto-scroll del perfil... (downloader.stop() para parar)', 'info');
                window.scrollTo(0, 0);
                await this.sleep(800);
                await this.autoScroll();
            }

            const elapsed = (new Date() - this.stats.startTime) / 1000;
            console.log('\n╔══════════════════════════════════════════════════════════╗');
            console.log('║                  CAPTURA COMPLETADA                      ║');
            console.log('╚══════════════════════════════════════════════════════════╝');
            console.log(`✅ Vídeos capturados: ${this.items.size}`);
            console.log(`⏱️ Tiempo total: ${elapsed.toFixed(1)}s`);

            if (this.items.size > 0) {
                this.export();
                console.log('\n📹 Primeros 5 vídeos:');
                this.videos.slice(0, 5).forEach((v, i) => {
                    const q = v.qualities[0];
                    console.log(`${i + 1}. [${q ? q.width + 'x' + q.height : '?'}] ${v.title.substring(0, 50)}`);
                });
                console.log('\n💡 Los enlaces directos del CDN caducan en horas.');
                console.log('   Descarga pronto con la GUI o JDownloader2.');
                console.log('   El JSON incluye pageUrl (no caduca) como respaldo.');
            } else {
                console.log('\n⚠️ No se capturaron vídeos. Recarga la página, pega el script');
                console.log('   ANTES de hacer scroll, y verifica que el perfil no sea privado.');
            }
        } catch (error) {
            this.log(`Error crítico: ${error.message}`, 'error');
            console.error(error);
        }
    }
}

// =======================
// CONFIGURACIÓN Y USO
// =======================
const config = {
    debugMode: false,        // Logs detallados
    scrollDelay: 1400,       // ms entre scrolls (sube si tu conexión es lenta)
    maxIdleRounds: 8,        // Rondas sin vídeos nuevos antes de terminar
    maxVideos: 0,            // 0 = todos los del perfil
    exportTxt: true,         // TXT con enlaces directos HQ
    exportJson: true,        // JSON completo (para TikTok Downloader GUI)
    dateFilter: null
    // dateFilter: { startDate: '2025-01-01', endDate: '2025-12-31' }
};

const downloader = new TikTokDownloader(config);

console.log('📌 Comandos: downloader.stop() | downloader.export() | downloader.config');
console.log('▶️ Ejecutando automáticamente en 3 segundos...');
setTimeout(() => { downloader.run(); }, 3000);
