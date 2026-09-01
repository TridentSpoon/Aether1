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

        if (avatarStructureLabel) {
            if (avatarName === 'red' || avatarName === 'crimson') avatarStructureLabel.textContent = 'OPTICAL EYE // DUAL ORBITS';
            else if (avatarName === 'arx-limes') avatarStructureLabel.textContent = 'ARCHIVAL VOXEL MATRIX';
            else if (avatarName === 'nexus' || avatarName === 'matrix') avatarStructureLabel.textContent = 'SINGULARITY VORTEX';
            else if (avatarName === 'arx-logos') avatarStructureLabel.textContent = 'JAGGED GEOMETRIC STAR';
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
    }

    // Color Theme Handler — purely cosmetic, independent of the selected avatar shape
    function applyColorTheme(themeName) {
        currentColorTheme = themeName;
        localStorage.setItem('aether_color_theme', themeName);
        document.documentElement.setAttribute('data-theme', themeName);
        hologram.setColorTheme(themeName);

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
        return msgDiv;
    }

    async function handleSendMessage(customPrompt = null) {
        const text = customPrompt || chatInput.value.trim();
        if (!text || isWaitingForResponse) return;

        chatInput.value = '';
        appendMessage('user', text);
        voiceEngine.playSFX('click');

        isWaitingForResponse = true;
        hologram.setState('THINKING');
        if (voiceEngine.onStateChange) voiceEngine.onStateChange('THINKING');

        const thinkingDiv = document.createElement('div');
        thinkingDiv.className = 'p-3 rounded my-2 text-sm leading-relaxed msg-agent self-start mr-8 typing-cursor';
        thinkingDiv.innerHTML = `<span class="text-xs font-mono text-cyan-400">🌐 ${currentAgentName} // Reactive processing</span>`;
        chatContainer.appendChild(thinkingDiv);
        chatContainer.scrollTop = chatContainer.scrollHeight;

        try {
            let reply, agentName, audioUrl;
            if (IS_TAURI) {
                const data = await tauriInvoke('generate_response_rust', { prompt: text, sessionId: 'default' });
                reply = data.reply;
                agentName = data.agent_name;
                audioUrl = autoSpeak ? await synthesizeSpeechUrl(reply) : null;
            } else {
                const resp = await apiFetch('/api/chat', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        message: text,
                        session_id: 'default',
                        generate_voice: autoSpeak
                    })
                });
                if (!resp.ok) throw new Error(`chat request failed: ${resp.status}`);
                const data = await resp.json();
                reply = data.reply;
                agentName = data.agent_name;
                audioUrl = data.audio_url ? API_BASE + data.audio_url : null;
            }

            chatContainer.removeChild(thinkingDiv);

            if (agentName) updateAgentNameDisplay(agentName);
            voiceEngine.playSFX('incoming');
            appendMessage(currentAgentName, reply, audioUrl);

            if (audioUrl && autoSpeak) {
                await voiceEngine.playTTSAudio(audioUrl);
            } else {
                hologram.setState('IDLE');
                if (voiceEngine.onStateChange) voiceEngine.onStateChange('IDLE');
            }
        } catch (e) {
            console.error("Chat error", e);
            if (chatContainer.contains(thinkingDiv)) chatContainer.removeChild(thinkingDiv);
            appendMessage(currentAgentName, `⚠️ System Error: ${e.message}`);
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

    async function handleScanSystem() {
        if (scannerResultsBox) {
            scannerResultsBox.innerHTML = '<div class="text-cyan-300 animate-pulse">Scanning for Provider API keys, Ollama, and LM Studio...</div>';
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

            if (data.ollama.available) {
                const modelsStr = data.ollama.models.length > 0 ? data.ollama.models.join(', ') : 'No models pulled yet';
                html += `<div class="text-green-400">✔ Ollama Online (Port 11434) - Models: <strong>${modelsStr}</strong></div>`;
            } else if (data.ollama.cli_installed) {
                html += `<div class="text-yellow-400">⚠ Ollama CLI is installed but server not running (\`ollama serve\`).</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ Ollama is not active on localhost:11434.</div>`;
            }

            if (data.lmstudio.available) {
                const lmStr = data.lmstudio.models.length > 0 ? data.lmstudio.models.join(', ') : 'Ready';
                html += `<div class="text-green-400">✔ LM Studio Active (Port 1234) - ${lmStr}</div>`;
            } else {
                html += `<div class="text-slate-400">⚪ LM Studio is not active on localhost:1234.</div>`;
            }

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
    async function synthesizeSpeechUrl(text, voiceName) {
        try {
            const path = await tauriInvoke('generate_speech_rust', { text, voice: voiceName || null });
            return window.__TAURI__.core.convertFileSrc(path);
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
        if (!IS_TAURI) return;
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

    btnMic.addEventListener('click', () => {
        voiceEngine.toggleListening();
    });

    btnSettings.addEventListener('click', () => {
        voiceEngine.playSFX('click');
        loadSettings();
        handleScanSystem();
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
    loadChatHistory();
    loadSettings();
    connectTelemetry();
    initVersionAndUpdates();
    initSpriteMode();

    document.body.addEventListener('click', () => {
        voiceEngine.playSFX('boot');
    }, { once: true });
});
