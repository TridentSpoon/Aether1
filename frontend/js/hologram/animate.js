// Per-frame animation loop. animate() computes the shared per-frame values (elapsedTime,
// audioIntensity, clickPulse) then dispatches to exactly one animateX() method for the
// active avatar.
//
// Design: every avatar sits front-facing and static at rest. The only things that wake
// it up are the agent's own THINKING / SPEAKING state and a click on the viewport --
// clicking sets lastClickTime (see core.js), which this loop turns into clickPulse, a
// value that eases from ~1 down to 0 over CLICK_REACT_DURATION seconds. LISTENING is
// treated the same as IDLE (static, click-reactive only) since it isn't one of the
// three reactions requested.

/* Whether the native window this webview lives in is on a screen at all.
   `document.hidden` is the browser's answer and it is the one to trust where it changes --
   but in the desktop window it does not always: WebKitGTK keeps a minimised window's page
   "visible", and no browser has an opinion about a window sitting in the tray. So the Rust
   side, which is asking the window manager anyway for its own telemetry loop, says so
   directly (see any_window_on_screen in src-tauri/src/main.rs, and the "hud-visibility"
   event it emits when the answer changes).
   Starts false -- meaning "no reason to think otherwise" -- so a page that never hears the
   event, i.e. every browser tab, behaves exactly as it did before. */
let nativelyHidden = false;

/* The loops that have parked themselves, and the one thing that restarts them.
   Parking rather than skipping is the point: a skipped frame still costs a callback, and
   `requestAnimationFrame` is not reliably throttled in a desktop webview the way it is in a
   background browser tab -- so a minimised window was still waking sixty times a second to
   decide it had nothing to do. */
const parked = new Set();

function resumeParkedLoops() {
    if (parked.size === 0) return;
    // Copied first: each resumed loop may park itself again immediately (the window is back
    // but the panel is still switched off), and that must not mutate what we are iterating.
    const waking = Array.from(parked);
    parked.clear();
    for (const resume of waking) resume();
}

if (typeof document !== 'undefined') {
    /* The events that mean "look again". `visibilitychange` covers a tab or a window the
       browser has an opinion about; `focus` and `pageshow` cover coming back to it. Each one
       only restarts the loop -- the loop's own checks decide whether there is anything to
       draw, so a spurious wake costs one frame's worth of tests. */
    document.addEventListener('visibilitychange', resumeParkedLoops);
    window.addEventListener('focus', resumeParkedLoops);
    window.addEventListener('pageshow', resumeParkedLoops);
    window.addEventListener('resize', resumeParkedLoops);

    if (window.__TAURI__ && window.__TAURI__.event) {
        window.__TAURI__.event.listen('hud-visibility', (event) => {
            nativelyHidden = !(event.payload && event.payload.visible);
            if (!nativelyHidden) resumeParkedLoops();
        }).catch(() => null);
    }
}

/* How long a parked loop waits before looking of its own accord.
   Two of the reasons to park raise no event at all: a panel switched off in Settings
   (`offsetParent`), and a lost WebGL context on a driver that never fires the restore event.
   For those this is the whole mechanism rather than a safety net -- and two wake-ups a
   second instead of sixty is the difference this pass is about. */
const PARKED_RECHECK_MS = 500;

HologramAvatar.prototype.animate = function() {
    // A disposed engine stops here rather than queueing another frame. Without this,
    // anything that replaces one engine with another -- the avatar workbench does it
    // on every slider nudge -- leaves the old loop running forever behind the new one.
    if (this.disposed) return;

    /* Nothing to draw for a window nobody can see. Minimised, hidden behind another window
       on a compositor that reports it, or sitting in the tray, this loop was running the
       full scene sixty times a second -- particle physics, tentacle chains, the lot --
       burning CPU and GPU on pixels that go nowhere.
       The loop parks instead of queueing another frame, and something has to wake it: an
       event, or the timer above. The clock keeps counting wall time while it is parked, so
       the avatar comes back where it would have been rather than frozen mid-gesture -- what
       you return to is the companion carrying on, not one that stopped when you looked away.

       The same argument one level down, and it is why parking is not simply "when the window
       is hidden": the window can be visible while this particular panel is switched off in
       Settings. `offsetParent` is null for an element with no layout box, which is exactly
       what `display: none` on any ancestor produces -- so this needs to know nothing about
       panels, and holds for a collapsed column or a hidden tab just as well. Guarded because
       a `position: fixed` canvas also reports null, and the desktop sprite window uses one.

       And the last of them: the driver has taken the context away. Drawing into it does
       nothing but throw, and the webglcontextrestored handler in core.js rebuilds the scene
       when it comes back. */
    const canvas = this.renderer && this.renderer.domElement;
    const hidden = (typeof document !== 'undefined' && document.hidden) || nativelyHidden;
    const offLayout = canvas && canvas.offsetParent === null &&
        getComputedStyle(canvas).position !== 'fixed';
    if (hidden || offLayout || this.contextLost) {
        this.park();
        return;
    }

    requestAnimationFrame(() => this.animate());

    const elapsedTime = this.clock.getElapsedTime();

    let audioSum = 0;
    for (let i = 0; i < 16; i++) {
        audioSum += this.audioData[i] || 0;
    }
    const audioIntensity = audioSum / (16 * 255);

    const CLICK_REACT_DURATION = 0.7;
    // Ramp the pulse up over a short rise instead of snapping to ~1 on the very first
    // frame after a click. hAlcy's rings ease toward a click-driven target radius over
    // several frames (see hAlcy's own animate) -- an instant-rise pulse gave the lattice's
    // displacement (which isn't eased, just recomputed from clickPulse every frame) no
    // time margin, so it could visibly bulge past the ring before the ring caught up.
    const CLICK_RISE_DURATION = 0.15;
    const clickAge = elapsedTime - this.lastClickTime;
    let clickPulse = 0;
    if (clickAge >= 0 && clickAge < CLICK_REACT_DURATION) {
        const decay = Math.sin((1 - clickAge / CLICK_REACT_DURATION) * (Math.PI / 2));
        const rise = clickAge < CLICK_RISE_DURATION
            ? Math.sin((clickAge / CLICK_RISE_DURATION) * (Math.PI / 2))
            : 1;
        clickPulse = decay * rise;
    }

    /* Half rate while there is genuinely nothing happening.

       At rest an avatar is a slow drift -- a ring turning, a core breathing -- and every
       one of those motions is driven by `elapsedTime`, which is wall time and keeps
       counting whether or not a frame was drawn. So a skipped frame does not slow the
       motion down or make it stutter; it draws the same drift with half as many samples,
       which at these speeds is not something an eye can pick out. The moment anything is
       actually moving -- a reply being spoken, the agent thinking, a click still ringing,
       a drag still coasting -- this steps straight back to the full rate, and those are
       exactly the moments where the extra frames are worth paying for.

       This is the single largest saving available in the loop: the HUD sits idle for most
       of its life, and idle used to cost exactly as much as speaking. */
    const lite = HologramAvatar.quality() === 'lite';
    const IDLE_FRAME_INTERVAL = 1 / 30;
    const busy = this.state === 'SPEAKING' || this.state === 'THINKING' ||
        clickPulse > 0 || audioIntensity > 0.01 || this.isDraggingView || this.viewSpinVelocity;
    // On the lite tier even a working avatar is held to 30: a machine that cannot draw
    // this at 60 does not produce 60 smooth frames by being asked for them, it produces
    // 40 uneven ones and starves everything else in the window -- the chat, the typing,
    // the audio callbacks -- while it tries. A steady 30 looks better than that and
    // leaves the CPU for the parts of the app the operator is actually using.
    const interval = busy ? (lite ? 1 / 30 : 0) : IDLE_FRAME_INTERVAL;
    if (interval) {
        if (elapsedTime - (this.lastGatedFrame || 0) < interval) return;
        this.lastGatedFrame = elapsedTime;
    }

    if (busy) this.sampleFrameRate(elapsedTime);

    /* Every avatar animates itself. A throw here would kill the whole render loop for
       every avatar, so the failing one is reported once and left standing still rather
       than taking the HUD down with it. */
    const plugin = this.plugins.get(this.currentAvatar);
    if (plugin) {
        if (typeof plugin.def.animate === 'function' && !plugin.broken) {
            try {
                plugin.def.animate(plugin.model, {
                    time: elapsedTime,
                    audio: audioIntensity,
                    audioData: this.audioData,
                    click: clickPulse,
                    state: this.state,
                    palette: this.activePalette,
                    // The scene's current drag-yaw and the manual zoom level (see core.js's
                    // pointermove handler and setZoom) -- most avatars don't need either
                    // (their group sits inside avatarZoomGroup, which already gets both for
                    // free), but one that wants to counter-rotate itself static against a
                    // drag (Operator) or scale only part of itself (the custom avatar's
                    // tiers 1-3, never its tier-4 effect) needs them directly.
                    viewRotationY: this.scene.rotation.y,
                    zoom: this.zoomScale,
                    /* clickPulse above is the decaying strength of the last click. These
                       two are the click itself: clickTime is when it happened, which is
                       what an avatar compares against to notice a *new* click rather than
                       a pulse that is merely still loud (R.E.D. 9000 blinks on one,
                       A.R.X.LOGOS opens its aperture), and clickAge is how long ago. */
                    clickTime: this.lastClickTime,
                    clickAge,
                    /* Where the pointer is over the viewport, -1..1 on each axis, and how
                       long it has been still. The Nexus looks at it. */
                    pointer: {
                        x: this.pointerNX,
                        y: this.pointerNY,
                        idleFor: elapsedTime - this.lastMouseMoveTime,
                    },
                    THREE,
                });
            } catch (err) {
                plugin.broken = true;
                console.error(`Avatar "${this.currentAvatar}" threw while animating and was stopped:`, err);
            }
        }
    }

    // Drag-to-spin inertia -- once you let go, the yaw picked up during the drag (see
    // core.js's pointermove handler) keeps coasting and decays, like a spun globe settling.
    if (!this.isDraggingView && this.viewSpinVelocity) {
        this.scene.rotation.y += this.viewSpinVelocity;
        this.viewSpinVelocity *= 0.94;
        if (Math.abs(this.viewSpinVelocity) < 0.00005) this.viewSpinVelocity = 0;
    }

    this.renderer.render(this.scene, this.camera);
};

/* Stops the loop until something wakes it. Registered once per engine no matter how many
   frames decide to park, so an engine cannot end up with two loops running when it wakes. */
HologramAvatar.prototype.unpark = function() {
    if (this.parkedTimer) {
        clearTimeout(this.parkedTimer);
        this.parkedTimer = null;
    }
    if (this.parkedResume) {
        parked.delete(this.parkedResume);
        this.parkedResume = null;
    }
};

HologramAvatar.prototype.park = function() {
    if (this.parkedResume) return;
    const resume = () => {
        this.parkedResume = null;
        if (this.parkedTimer) {
            clearTimeout(this.parkedTimer);
            this.parkedTimer = null;
        }
        // A disposed engine has no business coming back, and animate() refuses anyway --
        // this just saves the frame.
        if (!this.disposed) this.animate();
    };

    this.parkedResume = resume;
    parked.add(resume);
    this.parkedTimer = setTimeout(() => {
        if (parked.delete(resume)) resume();
    }, PARKED_RECHECK_MS);
};

/* No avatar is animated from this file any more. Every one of them carries its own
   animate(model, ctx) and runs through the plugin path at the top of animate() above --
   see js/hologram/README.md. What is left here is the loop itself: the clock, the audio
   envelope, the click pulse, the drag-spin inertia and the frame-rate watch below. */

/* ---- Is this machine keeping up? -----------------------------------------------
   Watches the frame rate only while the avatar is actually working, because that is the
   only time the number means anything: an idle avatar is deliberately drawing at 30 and
   would look like a struggling one to any measurement that could not tell the difference.

   A window is two seconds long. One slow window proves nothing -- a window can be slow
   because the operator dragged another application across the screen, because a model
   just finished loading, or because the compositor was busy elsewhere -- so it takes
   three consecutive slow windows before the tier moves. That is six seconds of sustained
   failure to hold the frame rate, which no passing stall produces and a genuinely
   underpowered machine produces immediately.

   The drop is one-way for the life of the page, and written down so the next launch
   starts where this one ended up. */
HologramAvatar.prototype.sampleFrameRate = function(elapsedTime) {
    if (!HologramAvatar.qualityIsAuto() || HologramAvatar.quality() === 'lite') return;

    const WINDOW_SECONDS = 2;
    const SLOW_FPS = 20;
    const WINDOWS_BEFORE_DROP = 3;

    if (this.fpsWindowStart === undefined) {
        this.fpsWindowStart = elapsedTime;
        this.fpsWindowFrames = 0;
        this.fpsSlowWindows = 0;
        return;
    }
    this.fpsWindowFrames++;
    const age = elapsedTime - this.fpsWindowStart;
    if (age < WINDOW_SECONDS) return;

    const fps = this.fpsWindowFrames / age;
    this.fpsWindowStart = elapsedTime;
    this.fpsWindowFrames = 0;

    if (fps >= SLOW_FPS) {
        // One good window is enough to forgive the bad ones: what is being looked for is
        // a machine that cannot do this at all, not one that had a rough few seconds.
        this.fpsSlowWindows = 0;
        return;
    }
    if (++this.fpsSlowWindows < WINDOWS_BEFORE_DROP) return;

    console.warn(`hologram: ${fps.toFixed(1)} fps under load over ${WINDOWS_BEFORE_DROP} windows -- dropping to the lite tier`);
    HologramAvatar.writeStore(HologramAvatar.QUALITY_AUTO_KEY, 'lite');
    HologramAvatar.applyQualityToDocument();
    // The pixel ratio is the half of it that can change without a new context; MSAA
    // cannot, so it stays as it is until the next launch, which now starts lite.
    this.renderer.setPixelRatio(HologramAvatar.pixelRatioFor('lite'));
};
