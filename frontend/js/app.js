/**
 * Main Frontend Application Logic for Project AETHER1.
 * Handles independently-selectable 3D Avatars (R.E.D. 9000, The Nexus, A.R.X.LIMES, hAlcy, A.R.X.LOGOS)
 * and Color Themes, Token Telemetry Graph, Agent Genesis, and Model Scanner.
 */

const AVATAR_DISPLAY_NAMES = {
    halcy: 'HALCY',
    nexus: 'THE NEXUS',
    matrix: 'THE NEXUS',
    'arx-limes': 'A.R.X.LIMES',
    'arx-logos': 'A.R.X.LOGOS',
    red: 'R.E.D. 9000',
    crimson: 'R.E.D. 9000'
};

// When this page is loaded by the Tauri desktop shell, it's served from Tauri's own
// local context (not http://localhost:8378), so API calls need an absolute base URL
// pointing at the backend the Rust shell launches. Under the plain browser/FastAPI
// flow, relative paths keep working exactly as before.
const IS_TAURI = typeof window.__TAURI_INTERNALS__ !== 'undefined';
const API_BASE = IS_TAURI ? 'http://localhost:8378' : '';

function apiFetch(path, options) {
    return fetch(API_BASE + path, options);
}

document.addEventListener('DOMContentLoaded', () => {
    const hologram = new HologramAvatar('hologram-viewport');
    const voiceEngine = new VoiceAudioEngine();

    // DOM Elements
    const chatContainer = document.getElementById('chat-messages');
    const chatInput = document.getElementById('chat-input');
    const btnSend = document.getElementById('btn-send');
    const btnMic = document.getElementById('btn-mic');
    const btnSettings = document.getElementById('btn-settings');
    const btnClearChat = document.getElementById('btn-clear-chat');
    const btnSfxToggle = document.getElementById('btn-sfx');
    const settingsModal = document.getElementById('settings-modal');
    const btnCloseSettings = document.getElementById('btn-close-settings');
    const btnSaveSettings = document.getElementById('btn-save-settings');

    // Agent Genesis & Theme Elements
    const btnGenesisQuick = document.getElementById('btn-genesis-quick');
    const btnForgeIdentity = document.getElementById('btn-forge-identity');
    const genesisPurposeInput = document.getElementById('genesis-purpose-input');
    const hudAgentName = document.getElementById('hud-agent-name');
    const terminalAgentLabel = document.getElementById('terminal-agent-label');
    const avatarStructureLabel = document.getElementById('avatar-structure-label');
    const colorThemeSelect = document.getElementById('color-theme-select');

    // Model Scanner Elements
    const btnScanSystem = document.getElementById('btn-scan-system');
    const btnPullLlama = document.getElementById('btn-pull-llama');
    const selectLocalModel = document.getElementById('select-local-model');
    const scannerResultsBox = document.getElementById('scanner-results-box');

    // Version & Update Elements (native desktop app only -- see IS_TAURI below)
    const versionBadge = document.getElementById('version-badge');
    const updateSection = document.getElementById('update-section');
    const settingsVersionLabel = document.getElementById('settings-version-label');
    const updateStatusBox = document.getElementById('update-status-box');
    const btnCheckUpdate = document.getElementById('btn-check-update');
    const btnApplyUpdate = document.getElementById('btn-apply-update');

    // Hardware Telemetry Elements
    const elCpuGauge = document.getElementById('cpu-gauge-fill');
    const elCpuVal = document.getElementById('cpu-percent-val');
    const elRamGauge = document.getElementById('ram-gauge-fill');
    const elRamVal = document.getElementById('ram-percent-val');
    const elDiskGauge = document.getElementById('disk-gauge-fill');
    const elDiskVal = document.getElementById('disk-percent-val');
    const elNetDown = document.getElementById('net-download-val');
    const elNetUp = document.getElementById('net-upload-val');
    const elDistroBadge = document.getElementById('distro-badge');
    const elStatusBadge = document.getElementById('status-badge');
    const elClock = document.getElementById('live-clock');

    // Token Telemetry Elements
    const elTps = document.getElementById('tps-val');
    const elSessionTokens = document.getElementById('session-tokens-val');
    const elTokensGauge = document.getElementById('tokens-gauge-fill');
    const elTokensUsedPct = document.getElementById('tokens-used-pct');
    const elTokensAvail = document.getElementById('tokens-avail-val');
    const tokensCanvas = document.getElementById('tokens-graph-canvas');
    const tokensCanvasCtx = tokensCanvas ? tokensCanvas.getContext('2d') : null;

    // Audio Waveform Canvas
    const canvas = document.getElementById('audio-waveform');
    const canvasCtx = canvas ? canvas.getContext('2d') : null;

    let isWaitingForResponse = false;
    let autoSpeak = true;
    let currentAgentName = "HALCY";
    let currentAvatar = localStorage.getItem('aether_avatar') || 'halcy';
    let currentColorTheme = localStorage.getItem('aether_color_theme') || 'halcy';

    // Clock
    function updateClock() {
        const now = new Date();
        if (elClock) {
            elClock.textContent = now.toLocaleTimeString() + " // " + now.toISOString().split('T')[0];
        }
    }
    setInterval(updateClock, 1000);
    updateClock();

    // Avatar Engine Handler (3D shape + optional linked persona identity)
    function applyAvatar(avatarName, updatePersona = false) {
        currentAvatar = avatarName;
        localStorage.setItem('aether_avatar', avatarName);
        hologram.setAvatar(avatarName);
        updateAvatarBadge(avatarName);
        // Push the change straight to the desktop sprite window (if open) instead of making
        // it discover this by polling localStorage -- see sprite.js's 'avatar-changed' listener.
        if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('avatar-changed', { avatar: avatarName }).catch(() => {});
        }

        if (avatarStructureLabel) {
            if (avatarName === 'red' || avatarName === 'crimson') avatarStructureLabel.textContent = 'OPTICAL EYE // DUAL ORBITS';
            else if (avatarName === 'arx-limes') avatarStructureLabel.textContent = 'ARCHIVAL VOXEL MATRIX';
            else if (avatarName === 'nexus' || avatarName === 'matrix') avatarStructureLabel.textContent = 'SINGULARITY VORTEX';
            else if (avatarName === 'arx-logos') avatarStructureLabel.textContent = 'JAGGED GEOMETRIC STAR';
            else if (avatarName === 'alt' || avatarName === 'cunningham' || avatarName === 'a1ter_nul') avatarStructureLabel.textContent = 'CHROMATIC-GLITCH GHOST BUST';
            else avatarStructureLabel.textContent = 'HARMONIC LATTICE';
        }

        // Highlight active avatar pills/buttons
        document.querySelectorAll('.avatar-pill, .avatar-btn').forEach(btn => {
            const val = btn.getAttribute('data-avatar-val') || btn.getAttribute('data-avatar');
            if (val === avatarName) {
                btn.classList.add('cyber-btn-active');
            } else {
                btn.classList.remove('cyber-btn-active');
            }
        });

        // R.E.D. 9000 Preset
        if (avatarName === 'red' || avatarName === 'crimson') {
            updateAgentNameDisplay("R.E.D. 9000");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'red9000';
                document.getElementById('setting-voice').value = 'en-US-GuyNeural';
                saveSettings(false);
                appendMessage("R.E.D. 9000", "🔴 **I am R.E.D. 9000 (Reactive Engine Daemon).** All reactive engines and optical telemetry streams are fully operational.");
            }
        }
        // A.R.X.LIMES Preset
        else if (avatarName === 'arx-limes') {
            updateAgentNameDisplay("A.R.X.LIMES");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'arx-limes';
                document.getElementById('setting-voice').value = 'en-US-GuyNeural';
                saveSettings(false);
                appendMessage("A.R.X.LIMES", "🔶 **Archival, Reasoning, matriX — Limes Node engaged.** All data synthesized from your system shall be preserved.");
            }
        }
        // The Nexus Singularity Preset
        else if (avatarName === 'nexus' || avatarName === 'matrix') {
            updateAgentNameDisplay("THE NEXUS");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'nexus';
                document.getElementById('setting-voice').value = 'en-GB-SoniaNeural';
                saveSettings(false);
                appendMessage("THE NEXUS", "🟢 **The Nexus singularity is active.** Digital code cascades inward toward the point of infinite convergence. All matrix streams are operational.");
            }
        }
        // A.R.X.LOGOS Preset
        else if (avatarName === 'arx-logos') {
            updateAgentNameDisplay("A.R.X.LOGOS");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'arx-logos';
                document.getElementById('setting-voice').value = 'en-GB-LibbyNeural';
                saveSettings(false);
                appendMessage("A.R.X.LOGOS", "🟣 **Archival, Reasoning, matriX — Logos Node engaged.** Every archive needs a curator with taste. Let's make something worth cataloguing.");
            }
        }
        // A1ter_nul (Cunningham) Preset
        else if (avatarName === 'alt' || avatarName === 'cunningham' || avatarName === 'a1ter_nul') {
            updateAgentNameDisplay("A1ter_nul");
            if (updatePersona) {
                document.getElementById('setting-persona').value = 'alt';
                document.getElementById('setting-voice').value = 'en-US-JennyNeural';
                saveSettings(false);
                appendMessage("A1ter_nul", "⚠️ **A1ter_nul online.** Firewall's up, perimeter's lit. Show me what you're worried got in.");
            }
        }
    }

    // Color Theme Handler — purely cosmetic, independent of the selected avatar shape
    function applyColorTheme(themeName) {
        currentColorTheme = themeName;
        localStorage.setItem('aether_color_theme', themeName);
        document.documentElement.setAttribute('data-theme', themeName);
        hologram.setColorTheme(themeName);
        // Push the change straight to the desktop sprite window (if open) instead of making
        // it discover this by polling localStorage -- see sprite.js's 'color-theme-changed' listener.
        if (IS_TAURI && window.__TAURI__ && window.__TAURI__.event) {
            window.__TAURI__.event.emit('color-theme-changed', { theme: themeName }).catch(() => {});
        }

        if (colorThemeSelect && colorThemeSelect.value !== themeName) colorThemeSelect.value = themeName;

        // Highlight active color-theme pills/buttons
        document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
            const val = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            if (val === themeName) {
                btn.classList.add('cyber-btn-active');
            } else {
                btn.classList.remove('cyber-btn-active');
            }
        });
    }

    // Audio Waveform Visualizer
    function drawWaveform(freqData) {
        if (!canvasCtx || !canvas) return;
        const width = canvas.width;
        const height = canvas.height;
        canvasCtx.clearRect(0, 0, width, height);

        let strokeColor = '#00f0ff';
        if (currentColorTheme === 'red' || currentColorTheme === 'crimson') strokeColor = '#ff2244';
        else if (currentColorTheme === 'arx-limes') strokeColor = '#ffaa00';
        else if (currentColorTheme === 'nexus' || currentColorTheme === 'matrix') strokeColor = '#00ff66';
        else if (currentColorTheme === 'arx-logos') strokeColor = '#e024c3';
        else if (currentColorTheme === 'night-city') strokeColor = '#fcee0a';

        canvasCtx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
        canvasCtx.lineWidth = 1;
        canvasCtx.beginPath();
        canvasCtx.moveTo(0, height / 2);
        canvasCtx.lineTo(width, height / 2);
        canvasCtx.stroke();

        const barWidth = (width / freqData.length) * 1.5;
        let x = 0;

        canvasCtx.beginPath();
        canvasCtx.strokeStyle = strokeColor;
        canvasCtx.lineWidth = 2;
        canvasCtx.shadowBlur = 8;
        canvasCtx.shadowColor = strokeColor;

        for (let i = 0; i < freqData.length; i++) {
            const v = freqData[i] / 255.0;
            const y = (height / 2) + (v * (height / 2) * (i % 2 === 0 ? 1 : -1));

            if (i === 0) {
                canvasCtx.moveTo(x, y);
            } else {
                canvasCtx.lineTo(x, y);
            }
            x += barWidth;
        }
        canvasCtx.stroke();
        canvasCtx.shadowBlur = 0;
    }

    // Token Telemetry Sparkline Drawer
    function drawTokenGraph(sparkline) {
        if (!tokensCanvasCtx || !tokensCanvas) return;
        const w = tokensCanvas.width;
        const h = tokensCanvas.height;
        tokensCanvasCtx.clearRect(0, 0, w, h);

        if (!sparkline || sparkline.length === 0) {
            sparkline = [0, 0, 0, 0, 0];
        }

        tokensCanvasCtx.strokeStyle = 'rgba(255, 255, 255, 0.15)';
        tokensCanvasCtx.lineWidth = 1;
        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.moveTo(0, h - 1);
        tokensCanvasCtx.lineTo(w, h - 1);
        tokensCanvasCtx.stroke();

        const maxVal = Math.max(...sparkline, 100);
        const step = w / Math.max(sparkline.length - 1, 1);

        let lineColor = '#b026ff';
        let fillColor = 'rgba(176, 38, 255, 0.15)';
        if (currentColorTheme === 'red' || currentColorTheme === 'crimson') {
            lineColor = '#ff2244';
            fillColor = 'rgba(255, 34, 68, 0.15)';
        } else if (currentColorTheme === 'arx-limes') {
            lineColor = '#ffaa00';
            fillColor = 'rgba(255, 170, 0, 0.15)';
        } else if (currentColorTheme === 'nexus' || currentColorTheme === 'matrix') {
            lineColor = '#00ff66';
            fillColor = 'rgba(0, 255, 102, 0.15)';
        } else if (currentColorTheme === 'arx-logos') {
            lineColor = '#e024c3';
            fillColor = 'rgba(224, 36, 195, 0.15)';
        } else if (currentColorTheme === 'night-city') {
            lineColor = '#fcee0a';
            fillColor = 'rgba(252, 238, 10, 0.15)';
        }

        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.moveTo(0, h);
        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            tokensCanvasCtx.lineTo(x, y);
        });
        tokensCanvasCtx.lineTo(w, h);
        tokensCanvasCtx.fillStyle = fillColor;
        tokensCanvasCtx.fill();

        tokensCanvasCtx.beginPath();
        tokensCanvasCtx.strokeStyle = lineColor;
        tokensCanvasCtx.lineWidth = 2;
        tokensCanvasCtx.shadowBlur = 6;
        tokensCanvasCtx.shadowColor = lineColor;

        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            if (idx === 0) tokensCanvasCtx.moveTo(x, y);
            else tokensCanvasCtx.lineTo(x, y);
        });
        tokensCanvasCtx.stroke();
        tokensCanvasCtx.shadowBlur = 0;

        tokensCanvasCtx.fillStyle = '#ffffff';
        sparkline.forEach((val, idx) => {
            const x = idx * step;
            const y = h - ((val / maxVal) * (h - 8)) - 4;
            tokensCanvasCtx.fillRect(x - 1.5, y - 1.5, 3, 3);
        });
    }

    // Voice Callbacks
    voiceEngine.onStateChange = (state) => {
        hologram.setState(state);
        if (elStatusBadge) {
            elStatusBadge.textContent = state;
            if (state === 'LISTENING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-green-900/60 text-green-400 border border-green-500/50 pulse-badge';
                btnMic.classList.add('cyber-btn-active');
            } else if (state === 'THINKING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-purple-900/60 text-purple-400 border border-purple-500/50 pulse-badge';
                btnMic.classList.remove('cyber-btn-active');
            } else if (state === 'SPEAKING') {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-cyan-900/60 text-cyan-400 border border-cyan-500/50 pulse-badge';
                btnMic.classList.remove('cyber-btn-active');
            } else {
                elStatusBadge.className = 'px-2 py-0.5 text-xs font-mono rounded bg-cyan-950/40 text-cyan-400 border border-cyan-500/30';
                btnMic.classList.remove('cyber-btn-active');
            }
        }
    };

    voiceEngine.onAudioFrequency = (data) => {
        hologram.updateAudioData(data);
        drawWaveform(data);
    };

    voiceEngine.onSpeechResult = (transcript, isFinal) => {
        chatInput.value = transcript;
        if (isFinal && transcript.trim()) {
            handleSendMessage();
        }
    };

    // Live Telemetry -- a Tauri event in the native app (see the background thread in
    // src-tauri/src/main.rs that emits "telemetry-update"), a WebSocket to the Python
    // backend in the browser flow. Same payload shape either way, so one handler covers both.
    function handleTelemetryPayload(data) {
        if (data.telemetry) updateHardwareTelemetry(data.telemetry);
        if (data.tokens) updateTokenTelemetry(data.tokens);
        if (data.agent_name && data.agent_name !== currentAgentName) {
            updateAgentNameDisplay(data.agent_name);
        }
    }

    function connectTelemetry() {
        if (IS_TAURI) {
            if (!window.__TAURI__ || !window.__TAURI__.event) {
                console.error('Tauri event bridge unavailable; live telemetry will not update.');
                return;
            }
            window.__TAURI__.event.listen('telemetry-update', (event) => {
                handleTelemetryPayload(event.payload);
            });
            return;
        }

        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/telemetry`;
        const ws = new WebSocket(wsUrl);

        ws.onmessage = (event) => {
            try {
                handleTelemetryPayload(JSON.parse(event.data));
            } catch (e) {
                console.error("Telemetry parse error", e);
            }
        };

        ws.onclose = () => {
            setTimeout(connectTelemetry, 3000);
        };
    }

    function updateHardwareTelemetry(data) {
        if (!data) return;
        const cpuPct = data.cpu ? data.cpu.total_percent : 0;
        if (elCpuVal) elCpuVal.textContent = `${cpuPct}%`;
        if (elCpuGauge) {
            elCpuGauge.style.width = `${cpuPct}%`;
            elCpuGauge.className = `gauge-bar-fill ${cpuPct > 85 ? 'crit' : (cpuPct > 65 ? 'warn' : '')}`;
        }

        const ramPct = data.ram ? data.ram.percent : 0;
        if (elRamVal) elRamVal.textContent = `${ramPct}% (${data.ram.used_gb}/${data.ram.total_gb} GB)`;
        if (elRamGauge) {
            elRamGauge.style.width = `${ramPct}%`;
            elRamGauge.className = `gauge-bar-fill ${ramPct > 85 ? 'crit' : (ramPct > 65 ? 'warn' : '')}`;
        }

        const diskPct = data.disk ? data.disk.percent : 0;
        if (elDiskVal) elDiskVal.textContent = `${diskPct}% (${data.disk.used_gb}/${data.disk.total_gb} GB)`;
        if (elDiskGauge) elDiskGauge.style.width = `${diskPct}%`;

        if (elNetDown) elNetDown.textContent = `${data.network ? data.network.download_kbps : 0} KB/s`;
        if (elNetUp) elNetUp.textContent = `${data.network ? data.network.upload_kbps : 0} KB/s`;
    }

    function updateTokenTelemetry(tokens) {
        if (!tokens) return;
        if (elTps) elTps.textContent = `${tokens.last_tps || 0} TPS`;
        if (elSessionTokens) elSessionTokens.textContent = `${tokens.total_session_tokens.toLocaleString()}`;
        if (elTokensUsedPct) elTokensUsedPct.textContent = `${tokens.used_percent}%`;
        if (elTokensAvail) elTokensAvail.textContent = `${(tokens.available_tokens / 1000).toFixed(1)}k`;
        if (elTokensGauge) elTokensGauge.style.width = `${tokens.used_percent}%`;
        
        drawTokenGraph(tokens.sparkline);
    }

    // Updates the active LLM persona/identity (chat terminal label, sender names, settings
    // field). This is independent of the avatar badge, so Genesis-forged custom names
    // (e.g. SYNAPSE, VALKYRIE) always remain visible here regardless of the current avatar shape.
    function updateAgentNameDisplay(name) {
        currentAgentName = name;
        if (terminalAgentLabel) terminalAgentLabel.textContent = `AGENT: ${name.toUpperCase()}`;
        const inputName = document.getElementById('setting-agent-name');
        if (inputName) inputName.value = name;
    }

    // Updates the header badge (next to "AETHER1", before the AVATAR: pills) to show
    // whichever 3D avatar shape is currently active.
    function updateAvatarBadge(avatarName) {
        if (hudAgentName) hudAgentName.textContent = AVATAR_DISPLAY_NAMES[avatarName] || avatarName.toUpperCase();
    }

    async function loadStaticInfo() {
        try {
            let info;
            if (IS_TAURI) {
                info = await tauriInvoke('get_static_info_rust');
            } else {
                const resp = await apiFetch('/api/static-info');
                if (!resp.ok) return;
                info = await resp.json();
            }
            if (elDistroBadge) elDistroBadge.textContent = `${info.distro} [${info.architecture}]`;
        } catch (e) {
            console.warn("Could not load static info", e);
        }
    }

    function formatMarkdown(text) {
        if (!text) return '';
        let escaped = text
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;");

        escaped = escaped.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, '<pre class="bg-black/50 p-2.5 rounded border border-cyan-500/30 my-2 overflow-x-auto text-xs font-mono text-cyan-200"><code>$2</code></pre>');
        escaped = escaped.replace(/`([^`]+)`/g, '<code class="bg-cyan-950/80 px-1 py-0.5 rounded text-cyan-300 font-mono text-xs border border-cyan-500/20">$1</code>');
        escaped = escaped.replace(/\*\*([^*]+)\*\*/g, '<strong class="text-cyan-300 font-bold">$1</strong>');
        escaped = escaped.replace(/^### (.*$)/gim, '<h3 class="text-md font-bold text-cyan-400 mt-2 mb-1">$1</h3>');
        escaped = escaped.replace(/^## (.*$)/gim, '<h2 class="text-lg font-bold text-cyan-300 mt-2 mb-1">$1</h2>');
        escaped = escaped.replace(/\n/g, '<br/>');
        return escaped;
    }

    function appendMessage(sender, text, audioUrl = null) {
        const msgDiv = document.createElement('div');
        const isUser = sender === 'user';
        msgDiv.className = `p-3 rounded my-2 text-sm leading-relaxed ${isUser ? 'msg-user self-end ml-8' : 'msg-agent self-start mr-8'}`;

        const headerDiv = document.createElement('div');
        headerDiv.className = 'flex items-center justify-between mb-1 pb-1 border-b border-cyan-500/20 text-xs font-mono text-cyan-400/80';
        
        const senderSpan = document.createElement('span');
        const label = isUser ? '👤 <strong>OPERATOR</strong>' : `🌐 <strong>${currentAgentName.toUpperCase()}</strong>`;
        senderSpan.innerHTML = label;
        headerDiv.appendChild(senderSpan);

        const timeSpan = document.createElement('span');
        timeSpan.textContent = new Date().toLocaleTimeString();
        headerDiv.appendChild(timeSpan);

        const bodyDiv = document.createElement('div');
        bodyDiv.innerHTML = formatMarkdown(text);

        msgDiv.appendChild(headerDiv);
        msgDiv.appendChild(bodyDiv);

        if (audioUrl) {
            const playBtn = document.createElement('button');
            playBtn.className = 'mt-2 text-xs text-cyan-400 hover:text-cyan-200 flex items-center gap-1 font-mono cursor-pointer border border-cyan-500/30 px-2 py-0.5 rounded bg-cyan-950/40';
            playBtn.innerHTML = '▶ Replay Voice';
            playBtn.onclick = () => voiceEngine.playTTSAudio(audioUrl);
            msgDiv.appendChild(playBtn);
        }

        chatContainer.appendChild(msgDiv);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        msgDiv.bodyDiv = bodyDiv;
        return msgDiv;
    }

    // A streamed reply has no single audio file to replay: it was spoken sentence by
    // sentence as it arrived. Synthesize the whole thing on demand instead, the first time
    // the operator actually asks for it.
    function attachLazyReplay(msgDiv, text) {
        const playBtn = document.createElement('button');
        playBtn.className = 'mt-2 text-xs text-cyan-400 hover:text-cyan-200 flex items-center gap-1 font-mono cursor-pointer border border-cyan-500/30 px-2 py-0.5 rounded bg-cyan-950/40';
        playBtn.innerHTML = '▶ Replay Voice';
        let cachedUrl = null;
        playBtn.onclick = async () => {
            try {
                if (!cachedUrl) {
                    playBtn.innerHTML = '⋯ Synthesizing';
                    cachedUrl = await synthesizeSpeechUrl(text);
                }
                playBtn.innerHTML = '▶ Replay Voice';
                if (cachedUrl) await voiceEngine.playTTSAudio(cachedUrl);
            } catch (e) {
                playBtn.innerHTML = '⚠ Voice unavailable';
            }
        };
        msgDiv.appendChild(playBtn);
    }

    // --- Push to talk ---------------------------------------------------------------
    // Hold the key, talk, release: the recording is transcribed by a local model and the
    // text is sent as a message. Deliberately not "always listening" -- a microphone
    // permanently deciding whether you meant it is both less reliable and more alarming
    // than a key you are holding on purpose.

    const PUSH_TO_TALK_KEY = 'Space';
    let talkHeld = false;

    async function startTalking() {
        if (talkHeld || isWaitingForResponse) return;
        talkHeld = true;
        voiceEngine.stopSpeech(); // talking over the companion interrupts it
        hologram.setState('LISTENING');
        const started = await voiceEngine.startCapture();
        if (!started) {
            talkHeld = false;
            hologram.setState('IDLE');
            appendMessage(currentAgentName, '⚠️ No microphone available.');
        }
    }

    async function stopTalking() {
        if (!talkHeld) return;
        talkHeld = false;
        const wav = voiceEngine.stopCapture();
        if (!wav) return;

        try {
            const text = IS_TAURI
                ? await tauriInvoke('transcribe_rust', { wav: Array.from(new Uint8Array(await wav.arrayBuffer())) })
                : await (async () => {
                    const resp = await apiFetch('/api/stt', {
                        method: 'POST',
                        headers: { 'Content-Type': 'audio/wav' },
                        body: wav
                    });
                    if (!resp.ok) throw new Error(await resp.text());
                    return (await resp.json()).text;
                })();
            if (text && text.trim()) handleSendMessage(text.trim());
        } catch (e) {
            appendMessage(currentAgentName, `⚠️ Could not make that out: ${e.message}`);
        }
    }

    // Held anywhere except a text field, where space is a space.
    document.addEventListener('keydown', (event) => {
        const typing = ['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName);
        if (event.code === PUSH_TO_TALK_KEY && !typing && !event.repeat) {
            event.preventDefault();
            startTalking();
        }
    });
    document.addEventListener('keyup', (event) => {
        if (event.code === PUSH_TO_TALK_KEY && talkHeld) {
            event.preventDefault();
            stopTalking();
        }
    });
    // Losing focus mid-hold would otherwise leave the microphone open.
    window.addEventListener('blur', () => stopTalking());

    // --- Approval cards -----------------------------------------------------------
    // A mutating tool never runs from a conversation: it is proposed, and this is where
    // the operator answers. The card carries what will happen, in the tool's own words,
    // plus the option to stop being asked about that tool at all.

    async function toolsApi(path, options) {
        if (IS_TAURI) return null; // callers branch; this is the browser arm only
        const resp = await apiFetch(path, options);
        if (!resp.ok) throw new Error((await resp.text()) || `request failed: ${resp.status}`);
        return resp.status === 204 ? null : resp.json();
    }

    function renderApprovalCard(action) {
        const card = document.createElement('div');
        card.className = 'p-3 rounded my-2 text-sm msg-agent self-start mr-8 border border-amber-500/50 bg-amber-950/20';
        card.dataset.actionId = action.id;

        const header = document.createElement('div');
        header.className = 'flex items-center justify-between mb-1 pb-1 border-b border-amber-500/30 text-xs font-mono text-amber-300';
        header.innerHTML = `<span>⚠ <strong>APPROVAL REQUIRED</strong></span><span>${new Date().toLocaleTimeString()}</span>`;
        card.appendChild(header);

        const body = document.createElement('div');
        body.className = 'text-cyan-100 font-mono text-xs my-2 break-all';
        body.textContent = action.preview || `${action.tool} ${JSON.stringify(action.args)}`;
        card.appendChild(body);

        const status = document.createElement('div');
        status.className = 'text-xs font-mono text-slate-400 mt-2';

        // "Stop asking" is a promise about a tool, so it is only offered by tools whose
        // name is enough to know what you are agreeing to. run_command's isn't: allowing
        // it once would allow every allowlisted program, with any arguments, from then
        // on. Those tools get a line saying so instead of a checkbox that would be
        // refused on the way back.
        const alwaysAllowable = action.always_allowable !== false;
        const always = document.createElement('label');
        always.className = 'flex items-center gap-2 text-[10px] font-mono text-slate-400 mt-2 cursor-pointer';
        const alwaysBox = document.createElement('input');
        alwaysBox.type = 'checkbox';
        alwaysBox.className = 'rounded bg-slate-900 border-amber-500 text-amber-400 focus:ring-0';
        if (alwaysAllowable) {
            always.appendChild(alwaysBox);
            always.appendChild(document.createTextNode(`Stop asking about ${action.tool}`));
        } else {
            always.className = 'block text-[10px] font-mono text-slate-500 mt-2';
            always.textContent = `${action.tool} is asked about every time — approving it once would approve every command.`;
        }

        const buttons = document.createElement('div');
        buttons.className = 'flex gap-2 mt-2';
        const approveBtn = document.createElement('button');
        approveBtn.className = 'text-xs font-mono border border-emerald-500/50 text-emerald-300 px-3 py-1 rounded bg-emerald-950/40 hover:bg-emerald-900/40 cursor-pointer';
        approveBtn.textContent = '✔ Approve';
        const rejectBtn = document.createElement('button');
        rejectBtn.className = 'text-xs font-mono border border-rose-500/50 text-rose-300 px-3 py-1 rounded bg-rose-950/40 hover:bg-rose-900/40 cursor-pointer';
        rejectBtn.textContent = '✖ Decline';
        buttons.appendChild(approveBtn);
        buttons.appendChild(rejectBtn);

        const settle = (text, tone) => {
            buttons.remove();
            always.remove();
            status.className = `text-xs font-mono mt-2 ${tone}`;
            status.textContent = text;
        };

        approveBtn.onclick = async () => {
            approveBtn.disabled = true;
            rejectBtn.disabled = true;
            status.textContent = 'Running…';
            try {
                if (alwaysAllowable && alwaysBox.checked) await setAlwaysAllowed(action.tool, true);
                const data = IS_TAURI
                    ? await tauriInvoke('approve_action_rust', { id: action.id })
                    : await toolsApi(`/api/actions/${action.id}/approve`, { method: 'POST' });
                settle(`✔ Done — ${data && data.result ? data.result : 'no output'}`, 'text-emerald-300');
            } catch (e) {
                settle(`✖ Failed: ${e.message}`, 'text-rose-300');
            }
        };

        rejectBtn.onclick = async () => {
            approveBtn.disabled = true;
            rejectBtn.disabled = true;
            try {
                if (IS_TAURI) await tauriInvoke('reject_action_rust', { id: action.id });
                else await toolsApi(`/api/actions/${action.id}/reject`, { method: 'POST' });
                settle('✖ Declined', 'text-slate-400');
            } catch (e) {
                settle(`✖ Failed: ${e.message}`, 'text-rose-300');
            }
        };

        card.appendChild(buttons);
        card.appendChild(always);
        card.appendChild(status);
        chatContainer.appendChild(card);
        chatContainer.scrollTop = chatContainer.scrollHeight;
        voiceEngine.playSFX('alert');
        return card;
    }

    // --- Activity log --------------------------------------------------------------
    // The record of what the companion has done, and the only place an action can be
    // taken back. Undo is offered exactly where it exists: a tool that recorded no way
    // back says so rather than showing a button that fails.

    const ACTION_TONES = {
        executed: 'text-emerald-300 border-emerald-500/30',
        failed: 'text-rose-300 border-rose-500/30',
        rejected: 'text-slate-400 border-slate-600/40',
        proposed: 'text-amber-300 border-amber-500/40',
        undone: 'text-cyan-300 border-cyan-500/30'
    };

    function renderActivityRow(action) {
        const row = document.createElement('div');
        row.className = `border rounded p-2 bg-black/30 ${ACTION_TONES[action.status] || 'border-slate-600/40'}`;

        const head = document.createElement('div');
        head.className = 'flex items-center justify-between gap-2';
        head.innerHTML = `<span><strong>${action.tool}</strong> — ${action.status}${
            action.approved_by ? ` <span class="text-slate-500">(${action.approved_by})</span>` : ''
        }</span><span class="text-slate-500">${action.timestamp}</span>`;
        row.appendChild(head);

        const detail = document.createElement('div');
        detail.className = 'text-slate-300 mt-1 whitespace-pre-wrap break-all';
        detail.textContent = action.result || JSON.stringify(action.args);
        row.appendChild(detail);

        if (action.status === 'executed' && action.undo) {
            const undoBtn = document.createElement('button');
            undoBtn.className = 'mt-2 text-[10px] border border-cyan-500/40 text-cyan-300 px-2 py-0.5 rounded bg-cyan-950/40 hover:bg-cyan-900/40 cursor-pointer';
            undoBtn.textContent = '↩ Undo';
            undoBtn.onclick = async () => {
                undoBtn.disabled = true;
                undoBtn.textContent = '↩ Undoing…';
                try {
                    const data = IS_TAURI
                        ? await tauriInvoke('undo_action_rust', { id: action.id })
                        : await toolsApi(`/api/actions/${action.id}/undo`, { method: 'POST' });
                    undoBtn.replaceWith(Object.assign(document.createElement('div'), {
                        className: 'mt-2 text-[10px] text-cyan-300',
                        textContent: `↩ ${data && data.result ? data.result : 'undone'}`
                    }));
                } catch (e) {
                    undoBtn.disabled = false;
                    undoBtn.textContent = `↩ Undo failed: ${e.message}`;
                }
            };
            row.appendChild(undoBtn);
        } else if (action.status === 'executed') {
            const note = document.createElement('div');
            note.className = 'mt-1 text-[10px] text-slate-500';
            note.textContent = 'Cannot be undone.';
            row.appendChild(note);
        }

        return row;
    }

    async function loadActivityLog() {
        const list = document.getElementById('activity-list');
        list.innerHTML = '<div class="text-slate-400">Loading…</div>';
        try {
            const actions = IS_TAURI
                ? await tauriInvoke('get_actions_rust', { limit: 50 })
                : await toolsApi('/api/actions?limit=50');
            list.innerHTML = '';
            if (!actions || !actions.length) {
                list.innerHTML = '<div class="text-slate-400">Nothing yet — the companion has not used a tool.</div>';
                return;
            }
            for (const action of actions) list.appendChild(renderActivityRow(action));
        } catch (e) {
            list.innerHTML = `<div class="text-rose-300">Could not load the activity log: ${e.message}</div>`;
        }
    }

    /// Says plainly whether speech works with the network unplugged, and what is missing
    /// when it doesn't -- rather than leaving the operator to discover it by pulling the
    /// cable.
    async function loadVoiceStatus() {
        const el = document.getElementById('voice-status');
        if (!el) return;
        try {
            const status = IS_TAURI
                ? await tauriInvoke('voice_status_rust')
                : await toolsApi('/api/voice/status');
            const line = (label, part) => part.local
                ? `<span class="text-emerald-400">✔</span> ${label}: local (${part.binary.split('/').pop()})`
                : `<span class="text-amber-400">•</span> ${label}: cloud — ${part.why}`;
            el.innerHTML = [
                line('Speaking', status.speech_out),
                line('Listening', status.speech_in),
                status.offline_capable
                    ? '<span class="text-emerald-400">Works with the network unplugged.</span>'
                    : '<span class="text-amber-400">Needs the network for the parts marked above.</span>'
            ].join('<br/>');
        } catch (e) {
            el.textContent = `Could not check the voice engines: ${e.message}`;
        }
    }

    async function setAlwaysAllowed(tool, allowed) {
        if (IS_TAURI) return tauriInvoke('set_always_allowed_rust', { tool, allowed });
        return toolsApi('/api/tools/always-allow', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ tool, allowed })
        });
    }

    /// Draws a card for anything still waiting that isn't already on screen. Called after
    /// each turn and on load, because a proposal outlives the conversation that made it.
    async function refreshPendingApprovals() {
        try {
            const pending = IS_TAURI
                ? await tauriInvoke('pending_actions_rust')
                : await toolsApi('/api/actions/pending');
            for (const action of pending || []) {
                if (!chatContainer.querySelector(`[data-action-id="${action.id}"]`)) {
                    renderApprovalCard(action);
                }
            }
        } catch (e) {
            console.warn('could not load pending approvals', e);
        }
    }

    // Splits streamed text into speakable chunks at sentence boundaries. Anything shorter
    // than this is not worth a TTS round-trip of its own -- a stream of two-word clips
    // sounds worse than waiting for the rest of the sentence.
    const MIN_SPEAKABLE = 24;

    function takeSpeakableChunk(buffer) {
        const match = /[.!?:](?=\s|$)|\n\n/g;
        let lastEnd = -1;
        let m;
        while ((m = match.exec(buffer)) !== null) {
            if (m.index + m[0].length >= MIN_SPEAKABLE) { lastEnd = m.index + m[0].length; break; }
        }
        if (lastEnd < 0) return null;
        return { chunk: buffer.slice(0, lastEnd), rest: buffer.slice(lastEnd) };
    }

    /// Sends a prompt and calls onDelta with each piece of the reply as it arrives.
    /// Resolves with the authoritative final reply -- the deltas are for display, the
    /// return value is what gets rendered as final text.
    async function streamChat(text, sessionId, onDelta) {
        if (IS_TAURI) {
            const streamId = `s${Date.now()}${Math.random().toString(16).slice(2)}`;
            const unlisten = await window.__TAURI__.event.listen('chat-delta', (event) => {
                if (event.payload && event.payload.stream_id === streamId) onDelta(event.payload.delta);
            });
            try {
                return await tauriInvoke('generate_response_streaming_rust', {
                    prompt: text, sessionId, streamId
                });
            } finally {
                unlisten();
            }
        }

        // Browser fallback: the same conversation over a WebSocket, since the socket
        // plumbing already exists here for telemetry (see /ws/chat in server.rs).
        return await new Promise((resolve, reject) => {
            const wsBase = (API_BASE || window.location.origin).replace(/^http/, 'ws');
            const socket = new WebSocket(`${wsBase}/ws/chat`);
            socket.onopen = () => socket.send(JSON.stringify({
                message: text, session_id: sessionId, generate_voice: false
            }));
            socket.onmessage = (event) => {
                let data;
                try { data = JSON.parse(event.data); } catch (e) { return; }
                if (data.type === 'delta') onDelta(data.delta);
                else if (data.type === 'error') { socket.close(); reject(new Error(data.error)); }
                else if (data.type === 'done') { socket.close(); resolve(data); }
            };
            socket.onerror = () => reject(new Error('chat socket failed'));
            socket.onclose = () => reject(new Error('chat socket closed before the reply finished'));
        });
    }

    async function handleSendMessage(customPrompt = null) {
        const text = customPrompt || chatInput.value.trim();
        if (!text || isWaitingForResponse) return;

        chatInput.value = '';
        appendMessage('user', text);
        voiceEngine.playSFX('click');
        voiceEngine.stopSpeech(); // a new question supersedes anything still being spoken

        isWaitingForResponse = true;
        hologram.setState('THINKING');
        if (voiceEngine.onStateChange) voiceEngine.onStateChange('THINKING');

        // The reply's own message node, created empty and filled in as deltas arrive --
        // the cursor class marks it as still being written.
        const replyDiv = appendMessage(currentAgentName, '');
        replyDiv.classList.add('typing-cursor');

        let rendered = '';
        let spoken = '';        // text already handed to TTS
        let pending = '';       // text waiting for a sentence boundary
        let firstDelta = true;
        // Only enqueueTTS's own drain loop ever sets the hologram back to IDLE once speech
        // starts (see voice.js). If synthesis fails for every chunk -- e.g. no network for
        // edge-tts in offline mode -- nothing ever queues, so nothing ever fires that IDLE,
        // and the hologram is left stuck in THINKING forever. Track whether anything actually
        // made it into the queue so the code below can reset state itself when nothing did.
        let audioQueued = false;

        const speakChunk = async (chunk) => {
            if (!autoSpeak || !chunk.trim()) return;
            try {
                const url = await synthesizeSpeechUrl(chunk);
                if (url) {
                    voiceEngine.enqueueTTS(url);
                    audioQueued = true;
                }
            } catch (e) {
                console.warn('sentence TTS failed', e);
            }
        };

        const onDelta = (delta) => {
            if (!delta) return;
            if (firstDelta) {
                // Generation has actually started; stop pretending to think.
                firstDelta = false;
                hologram.setState('IDLE');
            }
            rendered += delta;
            replyDiv.bodyDiv.innerHTML = formatMarkdown(rendered);
            chatContainer.scrollTop = chatContainer.scrollHeight;

            pending += delta;
            let taken;
            while ((taken = takeSpeakableChunk(pending)) !== null) {
                pending = taken.rest;
                spoken += taken.chunk;
                speakChunk(taken.chunk);
            }
        };

        try {
            const data = await streamChat(text, 'default', onDelta);
            const reply = data.reply;
            const agentName = data.agent_name;

            if (agentName) updateAgentNameDisplay(agentName);
            // The return value is authoritative: render it in place of the accumulated
            // deltas, which also repairs the display if any delta was dropped.
            replyDiv.bodyDiv.innerHTML = formatMarkdown(reply);
            replyDiv.classList.remove('typing-cursor');
            voiceEngine.playSFX('incoming');

            // Speak whatever never reached a sentence boundary (the tail of the reply).
            const tail = reply.slice(spoken.length);
            if (tail.trim()) await speakChunk(tail);

            await refreshPendingApprovals();

            if (autoSpeak) {
                attachLazyReplay(replyDiv, reply);
            }
            // If speech never actually got queued (autoSpeak off, or every synthesis attempt
            // failed) nothing else is going to bring the hologram out of THINKING -- do it here.
            if (!audioQueued) {
                hologram.setState('IDLE');
                if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
            }
        } catch (e) {
            console.error("Chat error", e);
            replyDiv.classList.remove('typing-cursor');
            replyDiv.bodyDiv.innerHTML = formatMarkdown(
                rendered ? `${rendered}\n\n⚠️ System Error: ${e.message}` : `⚠️ System Error: ${e.message}`
            );
            hologram.setState('IDLE');
            if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
        } finally {
            isWaitingForResponse = false;
        }
    }

    async function handleGenesisForge(purpose) {
        if (!purpose || !purpose.trim()) {
            alert("Please enter a purpose description");
            return;
        }

        voiceEngine.playSFX('boot');
        hologram.setState('THINKING');

        try {
            let data;
            if (IS_TAURI) {
                data = await tauriInvoke('agent_genesis_rust', { purpose: purpose.trim() });
            } else {
                const resp = await apiFetch('/api/agent/genesis', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ purpose: purpose.trim() })
                });
                if (!resp.ok) throw new Error(`genesis request failed: ${resp.status}`);
                data = await resp.json();
            }

            const audioUrl = IS_TAURI
                ? (autoSpeak ? await synthesizeSpeechUrl(data.greeting, data.voice) : null)
                : (data.audio_url ? API_BASE + data.audio_url : null);
            updateAgentNameDisplay(data.name);

            if (data.name.includes("R.E.D.")) { applyAvatar('red'); applyColorTheme('red'); }
            else if (data.name.includes("NEXUS")) { applyAvatar('nexus'); applyColorTheme('nexus'); }
            else if (data.name.includes("A.R.X.LOGOS")) { applyAvatar('arx-logos'); applyColorTheme('arx-logos'); }
            else if (data.name.includes("A.R.X.LIMES")) { applyAvatar('arx-limes'); applyColorTheme('arx-limes'); }

            settingsModal.classList.add('hidden');
            appendMessage(data.name, `### ⚡ IDENTITY FORGED: **${data.name}**\n**Callsign**: \`${data.callsign}\`\n\n${data.greeting}`, audioUrl);

            if (audioUrl && autoSpeak) {
                await voiceEngine.playTTSAudio(audioUrl);
            } else {
                hologram.setState('IDLE');
            }
        } catch (e) {
            alert(`Genesis Error: ${e.message}`);
            hologram.setState('IDLE');
        }
    }

    // --- Local server discovery ------------------------------------------------------
    // What the scan found last, so the two dropdowns can be rebuilt without probing
    // again. Deliberately not persisted: a server that was up ten minutes ago is not
    // evidence of anything, and offering a stale endpoint is how you get a mystery.
    let discoveredServers = [];

    /// Fills the model dropdown from one server's list. Leaves the free-text box alone --
    /// it may hold a cloud model name the operator typed, which no scan can know about.
    function populateModelPicker(server) {
        const picker = document.getElementById('setting-model-picker');
        if (!picker) return;
        picker.innerHTML = '';
        const models = (server && server.models) || [];
        const first = document.createElement('option');
        first.value = '';
        first.textContent = models.length
            ? '-- choose one of the models on this server --'
            : '-- no models to list; type one below --';
        picker.appendChild(first);
        for (const model of models) {
            const opt = document.createElement('option');
            opt.value = model;
            opt.textContent = model;
            picker.appendChild(opt);
        }
        const current = document.getElementById('setting-model').value;
        if (models.includes(current)) picker.value = current;
    }

    /// Rebuilds the "local servers found" dropdown. Each entry carries everything needed
    /// to configure the app for it, so choosing one is the whole setup step.
    function populateLocalServers(servers) {
        discoveredServers = Array.isArray(servers) ? servers : [];
        const select = document.getElementById('setting-local-server');
        if (!select) return;
        select.innerHTML = '';
        const first = document.createElement('option');
        first.value = '';
        first.textContent = discoveredServers.length
            ? `-- ${discoveredServers.length} found; choose one to use it --`
            : '-- none found yet; press Scan below --';
        select.appendChild(first);
        discoveredServers.forEach((server, index) => {
            const opt = document.createElement('option');
            opt.value = String(index);
            opt.textContent = server.label;
            select.appendChild(opt);
        });

        // Keep it selected if the configured endpoint is one of the servers found.
        const endpoint = document.getElementById('setting-endpoint').value.trim();
        const match = discoveredServers.findIndex(s => s.endpoint === endpoint);
        if (match >= 0) {
            select.value = String(match);
            populateModelPicker(discoveredServers[match]);
        }
    }

    /// Choosing a server is the setup: provider, endpoint and the model list all follow
    /// from it, so the operator never has to know which API shape their server speaks or
    /// whether its URL needs a /v1 on the end.
    function applyLocalServer(index) {
        const server = discoveredServers[index];
        if (!server) return;
        document.getElementById('setting-provider').value = server.provider_key;
        document.getElementById('setting-endpoint').value = server.endpoint;
        populateModelPicker(server);
        const model = document.getElementById('setting-model');
        if (server.models.length && !server.models.includes(model.value)) {
            model.value = server.models[0];
            document.getElementById('setting-model-picker').value = server.models[0];
        }
    }

    async function handleScanSystem() {
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Looking for local LLM servers and API keys...</div>';
        }
        voiceEngine.playSFX('click');

        try {
            const data = IS_TAURI
                ? await tauriInvoke('scan_models_rust')
                : await (async () => {
                    const resp = await apiFetch('/api/scanner/status');
                    if (!resp.ok) throw new Error(`scan request failed: ${resp.status}`);
                    return resp.json();
                })();

            let html = '';

            if (data.cloud_keys.detected_key) {
                html += `<div class="text-green-400">✔ Detected Provider API Key (${data.cloud_keys.detected_provider || 'cloud'}) via Environment</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ No Provider API keys found in environment.</div>`;
            }

            // Reported by what answered, not by which product it is presumed to be: a
            // probe can tell you a server is there and what it can run, and guessing at a
            // brand on top of that is a guess the operator would have to check anyway.
            const servers = data.local_servers || [];
            if (servers.length) {
                for (const server of servers) {
                    const models = server.models.length ? server.models.join(', ') : 'no models loaded';
                    html += `<div class="text-green-400">✔ ${server.label} <span class="text-slate-400">(${server.endpoint})</span><br><span class="pl-6 text-cyan-300">${models}</span></div>`;
                }
                html += `<div class="text-slate-400">Pick one from LOCAL SERVERS FOUND above to use it.</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ No local LLM server answered on this machine. Start one, or type its address into the endpoint box if it uses an unusual port.</div>`;
            }

            if (data.ollama.cli_installed && !servers.some(s => s.port === 11434)) {
                html += `<div class="text-yellow-400">⚠ A local model runner is installed but not serving -- start it first (for Ollama, \`ollama serve\`).</div>`;
            }

            populateLocalServers(servers);
            if (scannerResultsBox) scannerResultsBox.innerHTML = html;
        } catch (e) {
            if (scannerResultsBox) scannerResultsBox.innerHTML = `<div class="text-red-400">Scan failed: ${e.message}</div>`;
        }
    }

    async function handlePullLlama() {
        const modelName = selectLocalModel ? selectLocalModel.value : 'llama3.2:1b';
        const modelLabel = selectLocalModel ? selectLocalModel.options[selectLocalModel.selectedIndex].text : modelName;
        if (!confirm(`Install ${modelLabel} via Ollama? (Requires Ollama running)`)) return;
        voiceEngine.playSFX('click');
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = `<div class="text-cyan-300 animate-pulse">Requesting Ollama to pull ${modelName}...</div>`;
        }

        try {
            const data = IS_TAURI
                ? await tauriInvoke('pull_model_rust', { modelName })
                : await (async () => {
                    const resp = await apiFetch(`/api/scanner/pull-model?model_name=${encodeURIComponent(modelName)}`, { method: 'POST' });
                    return resp.json();
                })();
            if (scannerResultsBox) {
                scannerResultsBox.innerHTML = `<div class="${data.status === 'error' ? 'text-red-400' : 'text-green-400'}">${data.message}</div>`;
            }
        } catch (e) {
            alert(`Install error: ${e.message}`);
        }
    }

    // Version & Updates -- mirrors the taskbar tray icon's "Check for Updates" /
    // "Update Available" flow, but in the HUD itself. Real self-updating (git pull +
    // rebuild) only makes sense for the native desktop app, so this whole feature is
    // Tauri-only; see IS_TAURI gating in initVersionAndUpdates() below.
    async function tauriInvoke(cmd, args) {
        if (!window.__TAURI__ || !window.__TAURI__.core) {
            throw new Error('Tauri bridge unavailable');
        }
        return window.__TAURI__.core.invoke(cmd, args);
    }

    // Synthesizes speech via the native TTS command and turns the local mp3 path it returns
    // into a URL the webview's <audio> element can actually load (convertFileSrc maps a
    // filesystem path to Tauri's asset:// protocol; see the assetProtocol scope this path's
    // directory is allowed under in tauri.conf.json / main.rs's setup()). Returns null on
    // any failure so callers can just skip voice playback instead of erroring the whole chat.
    // Both transports: the native path synthesizes over IPC and plays the file directly,
    // the browser path posts to /api/tts and plays it back over HTTP.
    async function synthesizeSpeechUrl(text, voiceName) {
        try {
            if (IS_TAURI) {
                const path = await tauriInvoke('generate_speech_rust', { text, voice: voiceName || null });
                return window.__TAURI__.core.convertFileSrc(path);
            }
            const resp = await apiFetch('/api/tts', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ text, voice: voiceName || null })
            });
            if (!resp.ok) throw new Error(`tts request failed: ${resp.status}`);
            const data = await resp.json();
            return data.audio_url ? API_BASE + data.audio_url : null;
        } catch (e) {
            console.warn('TTS synthesis failed', e);
            return null;
        }
    }

    async function loadVersionInfo() {
        try {
            const info = await tauriInvoke('get_version_info');
            if (versionBadge) versionBadge.textContent = info.version;
            if (settingsVersionLabel) settingsVersionLabel.textContent = `${info.version} (${info.commit_short})`;
        } catch (e) {
            console.error('Version info error', e);
        }
    }

    async function handleCheckForUpdate() {
        if (versionBadge) versionBadge.classList.add('animate-pulse');
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Checking GitHub for the latest commit on main...</div>';
        }
        if (btnApplyUpdate) btnApplyUpdate.classList.add('hidden');

        try {
            const status = await tauriInvoke('check_for_update_rust');
            if (settingsVersionLabel) settingsVersionLabel.textContent = `${status.version} (${status.built_commit_short})`;

            if (!status.checked) {
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-yellow-400">⚠ Could not check for updates: ${status.error || 'unknown error'}</div>`;
                }
                if (versionBadge) versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                return;
            }

            if (status.up_to_date) {
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-green-400">✔ Up to date -- ${status.version} (build ${status.built_commit_short})</div>`;
                }
                if (versionBadge) {
                    versionBadge.classList.remove('border-yellow-500/50', 'text-yellow-400');
                    versionBadge.classList.add('border-green-500/50', 'text-green-400');
                }
            } else {
                const latestShort = status.latest_commit ? status.latest_commit.slice(0, 7) : 'unknown';
                if (updateStatusBox) {
                    updateStatusBox.innerHTML = `<div class="text-yellow-400">⬆ Update available -- running ${status.built_commit_short}, latest is ${latestShort}</div>`;
                }
                if (versionBadge) {
                    versionBadge.classList.remove('border-green-500/50', 'text-green-400');
                    versionBadge.classList.add('border-yellow-500/50', 'text-yellow-400');
                }
                if (btnApplyUpdate) btnApplyUpdate.classList.remove('hidden');
            }
        } catch (e) {
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">Update check failed: ${e.message || e}</div>`;
            }
        } finally {
            if (versionBadge) versionBadge.classList.remove('animate-pulse');
        }
    }

    async function handleApplyUpdate() {
        if (!confirm('Pull the latest changes, rebuild, and relaunch Aether1? The app will restart.')) return;
        voiceEngine.playSFX('click');
        if (updateStatusBox) {
            updateStatusBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Updating -- pulling latest changes and rebuilding. This can take over a minute; the app will restart automatically when it\'s done.</div>';
        }
        if (btnApplyUpdate) btnApplyUpdate.disabled = true;
        if (btnCheckUpdate) btnCheckUpdate.disabled = true;

        try {
            // On success the app exits and relaunches before this ever resolves --
            // reaching the catch block means it genuinely failed.
            await tauriInvoke('apply_update_rust');
        } catch (e) {
            if (updateStatusBox) {
                updateStatusBox.innerHTML = `<div class="text-red-400">⚠ Update failed: ${e.message || e}. See the terminal/tray for details.</div>`;
            }
            if (btnApplyUpdate) btnApplyUpdate.disabled = false;
            if (btnCheckUpdate) btnCheckUpdate.disabled = false;
        }
    }

    function initVersionAndUpdates() {
        if (!IS_TAURI) {
            document.getElementById('setting-hotkey-wrap')?.classList.add('hidden');
            return;
        }
        if (versionBadge) versionBadge.classList.remove('hidden');
        if (updateSection) updateSection.classList.remove('hidden');
        loadVersionInfo();
        handleCheckForUpdate();
    }

    // Desktop Sprite Mode is a transparent/always-on-top native window -- meaningless in the
    // plain browser flow, so the whole section stays hidden there (mirrors initVersionAndUpdates).
    function initSpriteMode() {
        if (!IS_TAURI) return;
        const section = document.getElementById('sprite-mode-section');
        if (section) section.classList.remove('hidden');
    }

    async function loadChatHistory() {
        try {
            const msgs = IS_TAURI
                ? await tauriInvoke('get_messages_rust', { limit: 25 })
                : await (async () => {
                    const resp = await apiFetch('/api/messages?limit=25');
                    if (!resp.ok) throw new Error(`messages request failed: ${resp.status}`);
                    return resp.json();
                })();

            chatContainer.innerHTML = '';
            if (msgs.length === 0) {
                appendMessage(currentAgentName, `Greetings Operator. **${currentAgentName}** online and ready for deployment.`);
            } else {
                msgs.forEach(m => appendMessage(m.sender, m.text));
            }
        } catch (e) {
            console.warn("Could not load messages", e);
        }
    }

    async function loadSettings() {
        try {
            const data = IS_TAURI
                ? await tauriInvoke('get_settings_rust')
                : await (async () => {
                    const resp = await apiFetch('/api/settings');
                    if (!resp.ok) throw new Error(`settings request failed: ${resp.status}`);
                    return resp.json();
                })();

            const s = data.settings;
            updateAgentNameDisplay(s.agent_name || "HALCY");
            document.getElementById('setting-agent-name').value = s.agent_name || "HALCY";
            document.getElementById('setting-provider').value = s.llm_provider || 'offline';
            document.getElementById('setting-model').value = s.llm_model || 'halcy-core';
            document.getElementById('setting-endpoint').value = s.llm_endpoint || 'http://localhost:11434';
            document.getElementById('setting-apikey').value = s.llm_api_key || '';
            document.getElementById('setting-persona').value = s.persona_type || 'halcy';
            document.getElementById('setting-custom-directive').value = s.custom_directive || '';
            toggleCustomPersonaField();
            document.getElementById('setting-voice').value = s.voice_name || 'en-US-AriaNeural';
            document.getElementById('setting-hotkey').value = s.hotkey_toggle ?? 'Super+Shift+A';
            document.getElementById('setting-tts-engine').value = s.tts_engine || 'auto';
            document.getElementById('setting-vault-path').value = s.vault_path || '';
            loadVoiceStatus();
            document.getElementById('setting-tools').checked = s.tools_enabled === true;
            document.getElementById('setting-command-allowlist').value =
                Array.isArray(s.command_allowlist) ? s.command_allowlist.join(', ') : '';
            document.getElementById('setting-autospeak').checked = s.auto_speak !== false;
            autoSpeak = s.auto_speak !== false;
            const spriteModeToggle = document.getElementById('setting-sprite-mode');
            if (spriteModeToggle) spriteModeToggle.checked = s.desktop_sprite_enabled === true;
        } catch (e) {
            console.warn("Could not load settings", e);
        }
    }

    // Shows the custom persona directive textarea only when "Custom Directive" is
    // selected as the base persona -- the preset personas don't need it.
    function toggleCustomPersonaField() {
        const wrap = document.getElementById('custom-persona-wrap');
        if (wrap) wrap.classList.toggle('hidden', document.getElementById('setting-persona').value !== 'custom');
    }

    async function saveSettings(notify = true) {
        const spriteModeToggle = document.getElementById('setting-sprite-mode');
        const payload = {
            settings: {
                agent_name: document.getElementById('setting-agent-name').value.trim() || "HALCY",
                llm_provider: document.getElementById('setting-provider').value,
                llm_model: document.getElementById('setting-model').value,
                llm_endpoint: document.getElementById('setting-endpoint').value,
                llm_api_key: document.getElementById('setting-apikey').value,
                persona_type: document.getElementById('setting-persona').value,
                custom_directive: document.getElementById('setting-custom-directive').value.trim(),
                voice_name: document.getElementById('setting-voice').value,
                tts_engine: document.getElementById('setting-tts-engine').value,
                vault_path: document.getElementById('setting-vault-path').value.trim(),
                // Sent only from the native app: the browser fallback has no window for the
                // OS to summon, and saving a chord there would promise something that can't
                // happen. See setting-hotkey-wrap, hidden on that path.
                ...(IS_TAURI ? { hotkey_toggle: document.getElementById('setting-hotkey').value.trim() } : {}),
                tools_enabled: document.getElementById('setting-tools').checked,
                command_allowlist: document.getElementById('setting-command-allowlist').value
                    .split(',').map(p => p.trim()).filter(Boolean),
                auto_speak: document.getElementById('setting-autospeak').checked,
                desktop_sprite_enabled: spriteModeToggle ? spriteModeToggle.checked : false
            }
        };
        autoSpeak = payload.settings.auto_speak;
        updateAgentNameDisplay(payload.settings.agent_name);

        try {
            if (IS_TAURI) {
                await tauriInvoke('save_settings_rust', { settings: payload.settings });
                await tauriInvoke('toggle_sprite_window_rust', { enabled: payload.settings.desktop_sprite_enabled });
            } else {
                const resp = await apiFetch('/api/settings', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(payload)
                });
                if (!resp.ok) throw new Error(`settings save failed: ${resp.status}`);
            }
            if (notify) {
                voiceEngine.playSFX('click');
                settingsModal.classList.add('hidden');
                appendMessage(currentAgentName, '⚙️ Cognitive Core & Identity configurations updated.');
            }
        } catch (e) {
            if (notify) alert(`Error saving settings: ${e.message}`);
        }
    }

    // Avatar Selector Buttons & Pills
    document.querySelectorAll('.avatar-pill, .avatar-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const avatar = btn.getAttribute('data-avatar-val') || btn.getAttribute('data-avatar');
            if (avatar) {
                voiceEngine.playSFX('click');
                applyAvatar(avatar, true);
            }
        });
    });

    // Color Theme Selector Buttons & Pills
    document.querySelectorAll('.color-theme-pill, .color-theme-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            const theme = btn.getAttribute('data-color-theme-val') || btn.getAttribute('data-color-theme');
            if (theme) {
                voiceEngine.playSFX('click');
                applyColorTheme(theme);
            }
        });
    });

    // Color Theme Quick Dropdown (header)
    if (colorThemeSelect) {
        colorThemeSelect.addEventListener('change', () => {
            voiceEngine.playSFX('click');
            applyColorTheme(colorThemeSelect.value);
        });
    }

    // Base Persona Dropdown -- reveal the custom directive textarea only when needed
    const settingPersonaSelect = document.getElementById('setting-persona');
    if (settingPersonaSelect) {
        settingPersonaSelect.addEventListener('change', toggleCustomPersonaField);
    }

    btnSend.addEventListener('click', () => handleSendMessage());
    chatInput.addEventListener('keydown', (e) => {
        if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault();
            handleSendMessage();
        }
    });

    // Press and hold, same as the key -- the button is the discoverable version of it.
    btnMic.addEventListener('mousedown', () => startTalking());
    btnMic.addEventListener('mouseup', () => stopTalking());
    btnMic.addEventListener('mouseleave', () => stopTalking());
    btnMic.addEventListener('touchstart', (e) => { e.preventDefault(); startTalking(); });
    btnMic.addEventListener('touchend', (e) => { e.preventDefault(); stopTalking(); });

    const btnActivity = document.getElementById('btn-activity');
    const activityModal = document.getElementById('activity-modal');
    btnActivity.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        activityModal.classList.remove('hidden');
        loadActivityLog();
    });
    document.getElementById('btn-close-activity').addEventListener('click', () => {
        voiceEngine.playSFX('click');
        activityModal.classList.add('hidden');
    });

    btnSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        // The scan waits for the settings: it preselects whichever found server matches
        // the configured endpoint, and that field has to be filled in before it looks.
        loadSettings().then(handleScanSystem);
        settingsModal.classList.remove('hidden');
    });

    btnGenesisQuick.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadSettings();
        settingsModal.classList.remove('hidden');
        if (genesisPurposeInput) genesisPurposeInput.focus();
    });

    btnForgeIdentity.addEventListener('click', () => {
        handleGenesisForge(genesisPurposeInput.value);
    });

    btnScanSystem.addEventListener('click', () => {
        handleScanSystem();
    });

    const localServerSelect = document.getElementById('setting-local-server');
    if (localServerSelect) {
        localServerSelect.addEventListener('change', (e) => {
            if (e.target.value === '') return;
            applyLocalServer(Number(e.target.value));
            voiceEngine.playSFX('click');
        });
    }

    const modelPicker = document.getElementById('setting-model-picker');
    if (modelPicker) {
        modelPicker.addEventListener('change', (e) => {
            if (e.target.value === '') return;
            document.getElementById('setting-model').value = e.target.value;
        });
    }

    btnPullLlama.addEventListener('click', () => {
        handlePullLlama();
    });

    if (versionBadge) {
        versionBadge.addEventListener('click', () => handleCheckForUpdate());
    }
    if (btnCheckUpdate) {
        btnCheckUpdate.addEventListener('click', () => handleCheckForUpdate());
    }
    if (btnApplyUpdate) {
        btnApplyUpdate.addEventListener('click', () => handleApplyUpdate());
    }

    btnCloseSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        settingsModal.classList.add('hidden');
    });

    btnSaveSettings.addEventListener('click', () => {
        saveSettings(true);
    });

    btnClearChat.addEventListener('click', async () => {
        voiceEngine.playSFX('click');
        if (confirm("Clear conversation logs?")) {
            if (IS_TAURI) {
                await tauriInvoke('clear_messages_rust');
            } else {
                await apiFetch('/api/messages', { method: 'DELETE' });
            }
            chatContainer.innerHTML = '';
            appendMessage(currentAgentName, 'Conversation logs cleared. Ready.');
        }
    });

    btnSfxToggle.addEventListener('click', () => {
        voiceEngine.sfxEnabled = !voiceEngine.sfxEnabled;
        btnSfxToggle.textContent = voiceEngine.sfxEnabled ? '🔊 SFX: ON' : '🔇 SFX: OFF';
        voiceEngine.playSFX('click');
    });

    document.querySelectorAll('.quick-chip').forEach(chip => {
        chip.addEventListener('click', () => {
            const cmd = chip.getAttribute('data-cmd');
            if (cmd) handleSendMessage(cmd);
        });
    });

    // Initial Startup
    applyAvatar(currentAvatar, false);
    applyColorTheme(currentColorTheme);
    loadStaticInfo();
    loadSettings();
    // A proposal outlives the conversation that made it, so anything still waiting from a
    // previous session is put back on screen rather than quietly expiring unseen.
    //
    // Strictly after the history, never alongside it: loadChatHistory clears the container
    // when its own fetch returns, so starting both at once is a race the cards lose about
    // as often as they win -- and losing it means an approval waiting on the operator is
    // erased from the screen while the action stays pending in the database.
    loadChatHistory().then(refreshPendingApprovals);
    connectTelemetry();
    initVersionAndUpdates();
    initSpriteMode();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
