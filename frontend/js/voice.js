/**
 * Voice & Audio Synthesis Pipeline for Project AETHER1.
 * Features:
 * - Web Audio API Sci-Fi Sound Effects Synthesizer
 * - Microphone capture for push-to-talk, transcribed locally (see startCapture below)
 * - Neural TTS Audio Stream Player with Real-Time Frequency Analyser
 *
 * Listening does not use the browser's Web Speech API. In Chrome that is a cloud service:
 * the audio is uploaded to Google and the transcript sent back. Aether1 records here and
 * transcribes with whisper.cpp on this machine instead, so holding the mic key never puts
 * the operator's voice on someone else's server.
 */

/**
 * The pause left between two sentences of one reply. Long enough to hear as the end of a
 * sentence rather than a dropout, short enough that a paragraph does not drag.
 */
const SENTENCE_GAP_MS = 140;

class VoiceAudioEngine {
    constructor() {
        this.audioCtx = null;
        this.analyser = null;
        this.dataArray = new Uint8Array(64);
        this.currentAudio = null;
        this.capture = null;
        // Which speaker and microphone to use, as browser device ids. Empty means the
        // system's own choice, which is what this engine did before Settings could ask.
        this.outputDeviceId = '';
        this.inputDeviceId = '';
        this.ttsQueue = [];
        this.isDrainingQueue = false;
        this.sfxEnabled = true;
        // Who owns the speaker. Claimed synchronously by playClip and bumped by every cut,
        // so a clip that is still getting ready can tell it has been superseded and stay
        // silent rather than starting on top of whatever took its place. See playClip.
        this.playClaim = 0;
        this.settleCurrent = null;
        // The graph node the playing element feeds the analyser through, kept so it can be
        // disconnected when the clip is done instead of being left live for the life of the
        // page (one per sentence spoken adds up over a conversation).
        this.currentSource = null;

        this.onStateChange = null;
        this.onAudioFrequency = null;

        this.initAudioContext();
    }

    initAudioContext() {
        try {
            const AudioContext = window.AudioContext || window.webkitAudioContext;
            if (AudioContext) {
                this.audioCtx = new AudioContext();
                this.analyser = this.audioCtx.createAnalyser();
                this.analyser.fftSize = 128;
                this.dataArray = new Uint8Array(this.analyser.frequencyBinCount);
                // A gain stage between the analyser and the speakers, wired once here
                // rather than reconnected on every clip. It lets a silent voice self-test
                // (see runVoiceStartupSelfTest in app.js) mute what's actually heard while
                // the analyser -- which taps the signal upstream of this node -- still
                // sees the real decoded audio, so it can tell a genuine synthesis/playback
                // failure from a clip that simply wasn't meant to be heard.
                this.gainNode = this.audioCtx.createGain();
                this.analyser.connect(this.gainNode);
                this.gainNode.connect(this.audioCtx.destination);
            }
        } catch (e) {
            console.warn("Web Audio API not supported", e);
        }
    }

    /**
     * Sends everything this engine plays to one speaker. Returns whether it worked, which
     * the caller reports rather than swallowing.
     *
     * It has to be the *context* that moves, not the audio element. Every clip is routed
     * through `createMediaElementSource` for the waveform (see playClip), and an element
     * inside an audio graph no longer has an output of its own -- `HTMLMediaElement
     * .setSinkId` on it is ignored, silently, which is exactly the kind of failure this
     * pane exists to end. `AudioContext.setSinkId` is the one that moves the graph, and it
     * is newer: WebKitGTK, which is what the native window is, does not have it yet. So a
     * false here is a real answer -- "this window cannot move it" -- and the same setting
     * still steers `aether1 say`, which plays through its own process.
     */
    async setOutputDevice(deviceId) {
        this.outputDeviceId = deviceId || '';
        if (!this.audioCtx || typeof this.audioCtx.setSinkId !== 'function') {
            return !deviceId;
        }
        try {
            await this.audioCtx.setSinkId(this.outputDeviceId || '');
            return true;
        } catch (e) {
            console.warn('could not move audio to that device', e);
            return false;
        }
    }

    /** Which microphone startCapture opens. Applied at the next recording, not this one. */
    setInputDevice(deviceId) {
        this.inputDeviceId = deviceId || '';
    }

    /**
     * Queues one clip of a streamed reply. Sentences are synthesized as they complete, so
     * they can arrive out of order relative to playback -- this plays them strictly in the
     * order they were queued, and keeps the avatar in SPEAKING across the gaps between
     * clips instead of flickering back to IDLE between every sentence.
     */
    async enqueueTTS(audioUrl) {
        if (!audioUrl) return;
        this.ttsQueue.push(audioUrl);
        if (this.isDrainingQueue) return;

        this.isDrainingQueue = true;
        try {
            while (this.ttsQueue.length) {
                const url = this.ttsQueue.shift();
                let outcome = null;
                try {
                    outcome = await this.playClip(url);
                } catch (e) {
                    // playClip resolves rather than rejects for anything that happens to the
                    // audio itself, so this is the surrounding machinery failing -- a
                    // suspended context refusing to resume, most likely. Letting it out of
                    // here would leave the rest of the reply sitting in the queue to be
                    // spoken by whichever later reply next starts a drain loop, in among its
                    // sentences. Drop the clip and carry on with this reply's own.
                    console.warn('a sentence could not be played', e);
                } finally {
                    VoiceAudioEngine.releaseClip(url);
                }
                // Something else is on the speaker right now (a Replay button, the voice
                // test, an avatar tap): the rest of this reply is no longer what should be
                // heard, and starting the next sentence would put it over the top of that
                // clip. A cut that left the speaker *free* is stopSpeech dropping an old
                // turn, and anything queued after it belongs to the new one -- so the test
                // is whether something is actually playing, not merely that a cut happened.
                if (outcome && outcome.superseded && this.currentAudio) break;
                // A beat between sentences. Piper and edge-tts both end a clip on the last
                // sample of the last word, so playing the next one the instant `ended` fires
                // runs two sentences together with no breath between them -- which is what
                // "not giving it time to finish a sentence" sounds like from the outside.
                if (this.ttsQueue.length) await VoiceAudioEngine.wait(SENTENCE_GAP_MS);
            }
        } finally {
            this.isDrainingQueue = false;
            // Only if the speaker is actually free. A drain loop that stopped because
            // something else took the clip off it would otherwise drop the avatar to IDLE
            // while that something is still talking.
            if (!this.currentAudio) {
                if (this.onStateChange) this.onStateChange('IDLE');
                if (this.onAudioFrequency) this.onAudioFrequency(new Uint8Array(64));
            }
        }
    }

    /**
     * Stops whatever is playing and releases whoever is awaiting it.
     *
     * Pausing alone is not enough: a paused element fires no `ended` and no `error`, so the
     * awaiting `playTTSAudio` promise never settles and enqueueTTS's drain loop never gets
     * its turn back. See the note on `finish` in playTTSAudio.
     */
    cutCurrentClip() {
        // Bumping the claim first is what stops a clip that is still in playClip's setup --
        // awaiting the context's resume, with no element built yet -- from starting after
        // this cut and being unstoppable, because nothing here can reach an element that
        // does not exist. That check is on the far side of the await in playClip.
        this.playClaim += 1;
        if (this.currentAudio) {
            this.currentAudio.pause();
        }
        this.releaseCurrentSource();
        const settle = this.settleCurrent;
        this.settleCurrent = null;
        this.currentAudio = null;
        if (settle) settle();
    }

    /**
     * Takes the finished clip's element back out of the audio graph.
     *
     * A MediaElementAudioSourceNode cannot be reused and cannot be garbage collected while
     * it is connected, so a connected node per sentence means every clip a conversation ever
     * spoke is still in the graph, with its decoded element behind it. It is silent -- a
     * paused element feeds zeroes -- but the analyser still sums all of them on every render
     * quantum, and that cost grows for as long as the window stays open.
     */
    releaseCurrentSource() {
        if (!this.currentSource) return;
        try {
            this.currentSource.disconnect();
        } catch (e) {
            // Already disconnected, or a context that has gone away -- either way there is
            // nothing left to release.
        }
        this.currentSource = null;
    }

    /** A plain delay, used for the beat between two sentences of one reply. */
    static wait(ms) {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }

    /** Drops anything queued but not yet played (a new turn supersedes the old one). */
    stopSpeech() {
        const dropped = this.ttsQueue;
        this.ttsQueue = [];
        dropped.forEach((url) => VoiceAudioEngine.releaseClip(url));
        this.cutCurrentClip();
    }

    /**
     * Frees a blob URL minted for one clip. The streaming path owns the clips it queues and
     * plays each exactly once, so holding the blob afterwards only keeps the decoded audio
     * alive for the life of the page. A URL that is not a blob (the HTTP transport's own
     * /api/tts path) is left alone, as is one a caller deliberately keeps to replay.
     */
    static releaseClip(url) {
        if (typeof url === 'string' && url.startsWith('blob:')) {
            try {
                URL.revokeObjectURL(url);
            } catch (e) {
                // Already revoked, or no URL support -- nothing to recover from.
            }
        }
    }

    // --- Local push-to-talk ---------------------------------------------------------
    // Hold a key, talk, release. Audio is captured here and assembled into a 16 kHz mono
    // WAV in the page, then posted to Aether1 to be transcribed by a local model. Nothing
    // is sent anywhere -- which is the difference between this and the Web Speech API
    // above, which in most browsers is a cloud service wearing a local-looking API.

    /**
     * Opens the microphone and starts collecting audio.
     *
     * `options.onLevel` is called with the RMS level of every buffer as it arrives. Push to
     * talk has no use for it -- a held key already says when the sentence ended -- but
     * hands-free listening has nothing else to go on, so it watches the level to tell
     * talking from the silence after it (see listenHandsFree in js/app.js).
     */
    async startCapture(options = {}) {
        if (this.capture) return true;
        const onLevel = typeof options.onLevel === 'function' ? options.onLevel : null;
        try {
            // `exact` rather than a preference: a chosen microphone that has been
            // unplugged should fail here and say so, not quietly record the laptop lid
            // one while the operator believes they are on the headset.
            const audio = { channelCount: 1, echoCancellation: true, noiseSuppression: true };
            if (this.inputDeviceId) audio.deviceId = { exact: this.inputDeviceId };
            const stream = await navigator.mediaDevices.getUserMedia({ audio });
            const AudioContext = window.AudioContext || window.webkitAudioContext;
            const ctx = new AudioContext();
            const source = ctx.createMediaStreamSource(stream);
            // ScriptProcessor is deprecated but universally available and adequate here;
            // an AudioWorklet would need a separate module file for no practical gain at
            // this sample rate.
            const processor = ctx.createScriptProcessor(4096, 1, 1);
            const chunks = [];

            processor.onaudioprocess = (event) => {
                const samples = event.inputBuffer.getChannelData(0);
                chunks.push(new Float32Array(samples));
                if (onLevel) {
                    let sum = 0;
                    for (let i = 0; i < samples.length; i += 1) sum += samples[i] * samples[i];
                    onLevel(Math.sqrt(sum / samples.length));
                }
            };
            source.connect(processor);
            processor.connect(ctx.destination);

            this.capture = { stream, ctx, source, processor, chunks, sampleRate: ctx.sampleRate };
            if (this.onStateChange) this.onStateChange('LISTENING');
            this.playSFX('listen_start');
            return true;
        } catch (e) {
            console.warn('microphone unavailable', e);
            return false;
        }
    }

    /** Stops capture and returns the recording as a 16 kHz mono WAV, or null. */
    stopCapture() {
        if (!this.capture) return null;
        const { stream, ctx, source, processor, chunks, sampleRate } = this.capture;
        this.capture = null;

        processor.disconnect();
        source.disconnect();
        stream.getTracks().forEach((track) => track.stop());
        ctx.close();
        this.playSFX('listen_end');
        if (this.onStateChange) this.onStateChange('IDLE');

        const total = chunks.reduce((n, c) => n + c.length, 0);
        if (!total) return null;
        const samples = new Float32Array(total);
        let offset = 0;
        for (const chunk of chunks) { samples.set(chunk, offset); offset += chunk.length; }

        return VoiceAudioEngine.encodeWav(VoiceAudioEngine.resampleTo16k(samples, sampleRate));
    }

    /** Nearest-neighbour downsample to 16 kHz, which is what whisper.cpp expects. */
    static resampleTo16k(samples, sampleRate) {
        const target = 16000;
        if (sampleRate === target) return samples;
        const ratio = sampleRate / target;
        const out = new Float32Array(Math.floor(samples.length / ratio));
        for (let i = 0; i < out.length; i++) out[i] = samples[Math.floor(i * ratio)];
        return out;
    }

    /** 16-bit PCM mono WAV. */
    static encodeWav(samples) {
        const buffer = new ArrayBuffer(44 + samples.length * 2);
        const view = new DataView(buffer);
        const writeText = (offset, text) => {
            for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i));
        };

        writeText(0, 'RIFF');
        view.setUint32(4, 36 + samples.length * 2, true);
        writeText(8, 'WAVE');
        writeText(12, 'fmt ');
        view.setUint32(16, 16, true);        // PCM header size
        view.setUint16(20, 1, true);         // PCM
        view.setUint16(22, 1, true);         // mono
        view.setUint32(24, 16000, true);     // sample rate
        view.setUint32(28, 16000 * 2, true); // byte rate
        view.setUint16(32, 2, true);         // block align
        view.setUint16(34, 16, true);        // bits per sample
        writeText(36, 'data');
        view.setUint32(40, samples.length * 2, true);

        let offset = 44;
        for (const sample of samples) {
            const clamped = Math.max(-1, Math.min(1, sample));
            view.setInt16(offset, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true);
            offset += 2;
        }
        return new Blob([buffer], { type: 'audio/wav' });
    }

    /**
     * Plays one TTS clip and reports what actually happened, not just whether a JS
     * exception was thrown: `signalDetected` (via the analyser, which taps the signal
     * upstream of the gain node so this still works when muted) says whether real,
     * non-zero audio was ever decoded -- distinguishing a clip that genuinely played from
     * one that "succeeded" into a broken output device and produced silence. That gap is
     * exactly what let TTS failures go unnoticed before: `audio.play()` resolving, or
     * `onended` firing, only ever meant no exception was thrown, never that anything was
     * actually heard.
     *
     * `opts.audible` (default true) controls whether the clip is actually heard through
     * the gain node; pass false to run the exact same check silently (see
     * runVoiceStartupSelfTest in app.js). The returned promise always resolves (never
     * rejects) with `{ played, error, signalDetected }` -- `signalDetected` is `null` when
     * there is no analyser to ask (Web Audio unsupported).
     */
    /**
     * Plays one clip on its own account -- a Replay Voice button, the voice test, a forged
     * avatar's greeting. It supersedes whatever speech is in flight rather than joining it:
     * dropping the queue is the point.
     *
     * Cutting only the *current* clip would leave the drain loop alive. Settling its await
     * hands it its turn back, so it starts the next queued sentence immediately -- over the
     * top of the clip that just pre-empted it. Two voices, from one page, with no second
     * window involved.
     */
    async playTTSAudio(audioUrl, opts = {}) {
        this.stopSpeech();
        return this.playClip(audioUrl, opts);
    }

    /**
     * Plays one clip, pre-empting the current one but leaving the queue alone.
     *
     * Getting the speaker ready is not instant -- a suspended context has to be resumed
     * first, and that is an await -- so this claims the speaker *before* it yields and
     * checks the claim is still its own before it starts anything. Without that, two
     * callers that both reach here while the context is suspended (the reply being spoken
     * and a Replay button, the start-up self-test and the first answer) each cut a current
     * clip that does not exist yet, and then both build an element and play it: two voices,
     * and only the second one reachable by any later cut, so nothing can stop the first.
     * A claim that has moved on means some other clip is the one that should be heard, and
     * this one reports itself superseded instead of joining it.
     */
    async playClip(audioUrl, opts = {}) {
        const quiet = window.AETHER_QUIET_HOURS || {};
        const minuteOfDay = new Date().getHours() * 60 + new Date().getMinutes();
        const toMinute = (value, fallback) => {
            const match = /^(\d{2}):(\d{2})$/.exec(value || '');
            return match ? Number(match[1]) * 60 + Number(match[2]) : fallback;
        };
        const start = toMinute(quiet.start, 22 * 60);
        const end = toMinute(quiet.end, 7 * 60);
        const duringQuietHours = start === end ? true
            : start < end ? minuteOfDay >= start && minuteOfDay < end
                : minuteOfDay >= start || minuteOfDay < end;
        const quietBlocked = quiet.enabled === true && duringQuietHours
            && window.AETHER_HEADPHONES_CONNECTED !== true
            && window.AETHER_VOICE_REQUESTED !== true;
        const audible = opts.audible !== false && !quietBlocked;

        this.cutCurrentClip();
        const claim = this.playClaim;

        if (this.audioCtx && this.audioCtx.state === 'suspended') {
            try {
                await this.audioCtx.resume();
            } catch (e) {
                // A context that will not resume plays nothing, and saying so is more use
                // than throwing: the caller has a queue to get through, and a clip that
                // could not be heard is not a reason to abandon the rest of the reply.
                console.warn('the audio context would not resume', e);
            }
        }

        if (claim !== this.playClaim) {
            return { played: false, error: 'superseded', signalDetected: null, superseded: true };
        }

        if (this.gainNode) {
            this.gainNode.gain.value = audible ? 1 : 0;
        }

        return new Promise((resolve) => {
            // Every clip reaching here must be same-origin with this page, which is why
            // the native transport hands over bytes and mints a blob rather than using
            // convertFileSrc (see synthesizeSpeechUrl, and speech_clip_rust in main.rs).
            //
            // The reason is `createMediaElementSource` below: routing the element into the
            // graph makes the graph its *only* output, and WebKit mutes a
            // MediaElementAudioSourceNode whose media would taint the page's origin --
            // setFormat does `m_muted = wouldTaintOrigin()` and process() then zeroes the
            // output bus. No error, no failed load, no rejected play(); just silence, which
            // is what an `asset://localhost/...` clip on a `tauri://localhost` page got.
            //
            // Setting `crossOrigin` does not rescue that, which was an earlier attempt at
            // this and wrong: wry registers the asset scheme as secure and never as
            // CORS-enabled, so WebKit will not grant CORS on it whatever headers come back.
            // A blob URL sidesteps the question -- it is this page's own origin.
            const audio = new Audio(audioUrl);
            const requestedRate = Number(opts.playbackRate ?? window.AETHER_VOICE_RATE ?? 1);
            const baseRate = Number.isFinite(requestedRate)
                ? Math.max(0.75, Math.min(1.25, requestedRate)) : 1;
            const requestedPitch = Number(opts.pitchSemitones ?? window.AETHER_VOICE_PITCH ?? 0);
            const pitchSemitones = Number.isFinite(requestedPitch)
                ? Math.max(-4, Math.min(4, requestedPitch)) : 0;
            audio.playbackRate = baseRate * Math.pow(2, pitchSemitones / 12);
            const preservePitch = pitchSemitones === 0;
            if ('preservesPitch' in audio) audio.preservesPitch = preservePitch;
            if ('mozPreservesPitch' in audio) audio.mozPreservesPitch = preservePitch;
            if ('webkitPreservesPitch' in audio) audio.webkitPreservesPitch = preservePitch;
            this.currentAudio = audio;

            let signalDetected = false;
            let playbackError = null;
            // A byte frequency bin sits at 0-255; real decoded audio -- even a quiet
            // sentence -- clears a few counts somewhere across the spectrum, so this
            // threshold only fails to trip on genuine silence (a muted device, an empty
            // clip, a pipeline that produced nothing).
            const SIGNAL_THRESHOLD = 2;

            if (this.audioCtx && this.analyser) {
                try {
                    // Nothing to disconnect first: cutCurrentClip above has already released
                    // the outgoing clip's node, and it is the only thing that ever holds one.
                    const source = this.audioCtx.createMediaElementSource(audio);
                    source.connect(this.analyser);
                    this.currentSource = source;
                } catch (e) {
                    // Only thrown when this element already has a source node -- a fresh
                    // Audio is created per clip above, so this is unreachable in practice.
                    // Note what it does *not* catch: a cross-origin clip does not throw
                    // here, it silently mutes the node, which is why every clip reaching
                    // this function is same-origin (see the note above). Leaving the
                    // analyser unattached costs the frequency readout, never the audio.
                }
            }

            if (this.onStateChange) this.onStateChange('SPEAKING');

            // Frequency polling loop
            const pollFrequency = () => {
                if (!this.currentAudio || this.currentAudio.paused) return;
                if (this.analyser) {
                    this.analyser.getByteFrequencyData(this.dataArray);
                    if (!signalDetected) {
                        for (let i = 0; i < this.dataArray.length; i++) {
                            if (this.dataArray[i] > SIGNAL_THRESHOLD) {
                                signalDetected = true;
                                break;
                            }
                        }
                    }
                    if (this.onAudioFrequency) this.onAudioFrequency(this.dataArray);
                }
                requestAnimationFrame(pollFrequency);
            };

            // Settling exactly once, and reachable from outside.
            //
            // `stopSpeech()` and the pre-empt at the top of this function both *pause* the
            // outgoing clip, and a paused element fires neither `ended` nor `error` -- so
            // whoever was awaiting it waited forever. That matters because enqueueTTS awaits
            // this inside its drain loop while holding `isDrainingQueue`: one interruption
            // latched that flag true for the life of the page, and every later clip was
            // pushed onto a queue nothing would ever drain again. Speech simply stopped,
            // with the queue silently filling up behind it.
            let settled = false;
            const finish = () => {
                if (settled) return;
                settled = true;
                if (this.settleCurrent === finish) this.settleCurrent = null;
                if (this.currentAudio === audio) {
                    this.currentAudio = null;
                }
                /* The source node is deliberately *not* released here.

                   `ended` says the element has handed its last sample to the graph, not that
                   the last sample has been heard -- and on some backends it is optimistic
                   about even that. Disconnecting the node at that moment takes the element's
                   only output away while the tail of the sentence is still on its way to the
                   speakers, so the clip audibly stops short of its last word. Nothing else in
                   this file could cut a clip that the version before it played in full, which
                   is what made this the thing to undo.

                   It is released instead by the next playClip, through cutCurrentClip below,
                   by which point this clip is paused or long finished. That bounds what is
                   left connected to one node rather than one per sentence, which is all the
                   leak fix needed; a node that has stopped being pulled costs a summed zero,
                   and the silence cost a sentence. */
                // Mid-queue, the next clip is about to start: staying SPEAKING keeps the
                // avatar steady across the seam. enqueueTTS emits IDLE when it drains.
                if (!this.ttsQueue.length) {
                    if (this.onStateChange) this.onStateChange('IDLE');
                    if (this.onAudioFrequency) this.onAudioFrequency(new Uint8Array(64));
                }
                resolve({
                    played: !playbackError,
                    error: playbackError,
                    signalDetected: this.analyser ? signalDetected : null,
                    // Settled by a cut rather than by the clip running out: whoever is
                    // working through a queue should stop instead of playing the next
                    // sentence over the clip that just took the speaker.
                    superseded: claim !== this.playClaim,
                });
            };

            this.settleCurrent = finish;

            audio.onplay = () => {
                pollFrequency();
            };

            audio.onended = finish;

            audio.onerror = () => {
                playbackError = (audio.error && audio.error.message) || 'audio element error';
                finish();
            };

            audio.play().catch(e => {
                console.warn("Audio playback blocked or failed:", e);
                playbackError = (e && e.message) || String(e);
                finish();
            });
        });
    }

    /**
     * Synthesize futuristic sci-fi sound effects using Web Audio API oscillators.
     */
    playSFX(type) {
        if (!this.sfxEnabled || !this.audioCtx) return;
        try {
            if (this.audioCtx.state === 'suspended') {
                this.audioCtx.resume();
            }

            const now = this.audioCtx.currentTime;
            const osc = this.audioCtx.createOscillator();
            const gain = this.audioCtx.createGain();

            osc.connect(gain);
            gain.connect(this.audioCtx.destination);

            if (type === 'boot') {
                // Sci-fi boot chime
                osc.type = 'sine';
                osc.frequency.setValueAtTime(440, now);
                osc.frequency.exponentialRampToValueAtTime(880, now + 0.15);
                osc.frequency.exponentialRampToValueAtTime(1760, now + 0.3);
                gain.gain.setValueAtTime(0.2, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.5);
                osc.start(now);
                osc.stop(now + 0.5);
            } else if (type === 'click') {
                // Cyber click
                osc.type = 'triangle';
                osc.frequency.setValueAtTime(1200, now);
                osc.frequency.exponentialRampToValueAtTime(300, now + 0.05);
                gain.gain.setValueAtTime(0.15, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.05);
                osc.start(now);
                osc.stop(now + 0.05);
            } else if (type === 'listen_start') {
                // High futuristic ping
                osc.type = 'sine';
                osc.frequency.setValueAtTime(600, now);
                osc.frequency.exponentialRampToValueAtTime(1200, now + 0.12);
                gain.gain.setValueAtTime(0.2, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.15);
                osc.start(now);
                osc.stop(now + 0.15);
            } else if (type === 'listen_end') {
                // Confirmation tone
                osc.type = 'sine';
                osc.frequency.setValueAtTime(1200, now);
                osc.frequency.exponentialRampToValueAtTime(600, now + 0.12);
                gain.gain.setValueAtTime(0.15, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.15);
                osc.start(now);
                osc.stop(now + 0.15);
            } else if (type === 'incoming') {
                // One soft arrival tone. A three-note arpeggio here sounded like three
                // audio dropouts immediately before the avatar's spoken reply.
                osc.type = 'sine';
                osc.frequency.setValueAtTime(659.25, now); // E5
                gain.gain.setValueAtTime(0.08, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.18);
                osc.start(now);
                osc.stop(now + 0.18);
            } else if (type === 'alert') {
                // Approval request: deliberately unlike the message chime. Something is
                // waiting on the operator, and it should not sound like an answer arriving.
                osc.type = 'square';
                osc.frequency.setValueAtTime(880, now);
                osc.frequency.setValueAtTime(660, now + 0.1);
                osc.frequency.setValueAtTime(880, now + 0.2);
                gain.gain.setValueAtTime(0.08, now);
                gain.gain.exponentialRampToValueAtTime(0.001, now + 0.32);
                osc.start(now);
                osc.stop(now + 0.32);
            }
        } catch (e) {
            console.warn("SFX synthesis error:", e);
        }
    }
}

window.VoiceAudioEngine = VoiceAudioEngine;
