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

HologramAvatar.prototype.animate = function() {
    // A disposed engine stops here rather than queueing another frame. Without this,
    // anything that replaces one engine with another -- the avatar workbench does it
    // on every slider nudge -- leaves the old loop running forever behind the new one.
    if (this.disposed) return;
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
    // several frames (see animateHalcy) -- an instant-rise pulse gave the lattice's
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

    // A registered avatar animates itself and nothing built-in runs -- see core.js.
    // A throw here would kill the whole render loop for every avatar, so the failing
    // one is reported once and left standing still rather than taking the HUD with it.
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
                    THREE,
                });
            } catch (err) {
                plugin.broken = true;
                console.error(`Avatar "${this.currentAvatar}" threw while animating and was stopped:`, err);
            }
        }
    } else if (this.currentAvatar === 'arx-logos') {
        this.animateArxLogos(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'red' || this.currentAvatar === 'crimson') {
        this.animateRed9000(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'nexus' || this.currentAvatar === 'matrix') {
        this.animateNexus(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'arx-limes') {
        this.animateArxLimes(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'alt' || this.currentAvatar === 'cunningham' || this.currentAvatar === 'a1ter_nul') {
        this.animateAlt(elapsedTime, audioIntensity, clickPulse);
    } else {
        this.animateHalcy(elapsedTime, audioIntensity, clickPulse);
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

// ==============================================================
// A.R.X.LOGOS: CENTRAL HEXAGON WITH SIX SPIRALING HEXAGON ARMS
// ==============================================================
HologramAvatar.prototype.animateArxLogos = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';

    if (this.arxLogosGroup) {
        const spinSpeed = isThinking ? 0.02 : (isSpeaking ? 0.01 : clickPulse * 0.012);
        this.arxLogosGroup.rotation.z -= spinSpeed; // clockwise, matching the arm winding
        if (isThinking || isSpeaking) {
            this.arxLogosGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.1;
            this.arxLogosGroup.rotation.y = Math.cos(elapsedTime * 0.35) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod, plus the
            // aperture-close below (a click also triggers one immediately; otherwise it
            // closes on its own every several seconds so the eye never reads as a fixed prop).
            this.arxLogosGroup.rotation.x += (clickPulse * 0.08 - this.arxLogosGroup.rotation.x) * 0.15;
            this.arxLogosGroup.rotation.y += (0 - this.arxLogosGroup.rotation.y) * 0.15;

            if (this.lastClickTime !== this.arxLogosLastHandledApertureClick) {
                this.arxLogosLastHandledApertureClick = this.lastClickTime;
                this.arxLogosApertureStartTime = elapsedTime;
                this.arxLogosNextApertureTime = elapsedTime + 5 + Math.random() * 4;
            }
            if (elapsedTime >= this.arxLogosNextApertureTime) {
                this.arxLogosApertureStartTime = elapsedTime;
                this.arxLogosNextApertureTime = elapsedTime + 5 + Math.random() * 6;
            }
        }
    }

    // How far into an aperture-close/reopen cycle we are (0 = fully open, 1 = fully closed),
    // on the same sine close-then-reopen curve as R.E.D. 9000's blink.
    const APERTURE_DURATION = 0.5;
    const apertureAge = elapsedTime - this.arxLogosApertureStartTime;
    const apertureCloseness = (apertureAge >= 0 && apertureAge < APERTURE_DURATION)
        ? Math.sin((apertureAge / APERTURE_DURATION) * Math.PI)
        : 0;

    if (this.arxLogosCentralFill && this.arxLogosCentralOutline) {
        let coreScale = 1.0;
        if (isSpeaking) {
            coreScale = 1.0 + audioIntensity * 0.5;
        } else if (isThinking) {
            coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.18;
        } else {
            coreScale = 1.0 + clickPulse * 0.15;
        }
        this.arxLogosCentralFill.scale.set(coreScale, coreScale, coreScale);
        this.arxLogosCentralOutline.scale.set(coreScale, coreScale, coreScale);
    }

    // The catchlight flashes brighter right at the peak of an aperture-close -- a glint off
    // the core-eye as the blades sweep shut over it.
    if (this.arxLogosCatchlight) this.arxLogosCatchlight.material.opacity = 0.5 + apertureCloseness * 0.45;

    // Arm hexagons — double as camera-aperture blades: energy pulses outward along each arm
    // while speaking/thinking (or a brief pulse on click), same as always, but on top of
    // that every hex is pulled in from its resting spot toward the core-eye and twisted
    // during an aperture-close, then released back out (see the idle branch above for what
    // triggers this and apertureCloseness's 0..1 progress through it).
    this.arxLogosArmHexes.forEach(hex => {
        let pulseFactor = 1.0;
        if (isSpeaking) {
            const fVal = (this.audioData[hex.userData.stepIndex % 16] || 0) / 255;
            pulseFactor = 1.0 + fVal * 0.6 + Math.sin(elapsedTime * 10 + hex.userData.phase) * 0.15;
        } else if (isThinking) {
            pulseFactor = 1.0 + Math.sin(elapsedTime * 12 + hex.userData.phase) * 0.3;
        } else {
            pulseFactor = 1.0 + clickPulse * 0.2;
        }
        hex.scale.set(pulseFactor, pulseFactor, pulseFactor);

        hex.position.x = -hex.userData.baseX * apertureCloseness * 0.92;
        hex.position.y = -hex.userData.baseY * apertureCloseness * 0.92;
        hex.rotation.z = apertureCloseness * 1.1 * (hex.userData.armIndex % 2 === 0 ? 1 : -1);
    });

    // Outer dotted boundary ring — shimmers while speaking, otherwise still.
    this.arxLogosOuterDots.forEach(dot => {
        const shimmer = isSpeaking
            ? 1.0 + Math.sin(elapsedTime * 2 + dot.userData.phase) * 0.35
            : 1.0 + clickPulse * 0.25;
        dot.scale.set(shimmer, shimmer, shimmer);
    });

    // Two hex-frame shell rings, each tumbling on its own axis at its own speed, always --
    // like hAlcy's rings, they never go fully still, just faster while thinking/speaking.
    const shellSpinMult = isThinking ? 2.2 : (isSpeaking ? 1.5 : 1.0);
    this.arxLogosShellRings.forEach(ring => {
        ring.rotation.x += ring.userData.speed.x * shellSpinMult;
        ring.rotation.y += ring.userData.speed.y * shellSpinMult;
        ring.rotation.z += ring.userData.speed.z * shellSpinMult;
    });

    // Orbiting hex swarm — drifts around the whole structure in 3D continuously, each node
    // also slowly tumbling on its own.
    this.arxLogosSwarmNodes.forEach(node => {
        node.userData.angle += node.userData.speed * 0.01;
        node.position.x = Math.cos(node.userData.angle) * node.userData.radius;
        node.position.z = Math.sin(node.userData.angle) * node.userData.radius;
        node.position.y = node.userData.heightOffset + Math.sin(elapsedTime * 1.5 + node.userData.bobPhase) * 4;
        node.rotation.x += 0.015;
        node.rotation.y += 0.02;
    });
};

// ==============================================================
// R.E.D. 9000: CENTRAL SPHERE + TWO ORBIT CIRCLES
// ==============================================================
HologramAvatar.prototype.animateRed9000 = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';

    if (this.redGroup) {
        if (isThinking || isSpeaking) {
            this.redGroup.rotation.y = Math.sin(elapsedTime * 0.3) * 0.15;
            this.redGroup.rotation.x = Math.sin(elapsedTime * 0.2) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod, plus the
            // blink below (a click also triggers an immediate blink; otherwise it blinks on
            // its own every few seconds so the eye never reads as a fixed prop).
            this.redGroup.rotation.y += (clickPulse * 0.12 - this.redGroup.rotation.y) * 0.15;
            this.redGroup.rotation.x += (clickPulse * 0.08 - this.redGroup.rotation.x) * 0.15;

            if (this.lastClickTime !== this.redLastHandledBlinkClick) {
                this.redLastHandledBlinkClick = this.lastClickTime;
                this.redBlinkStartTime = elapsedTime;
                this.redNextBlinkTime = elapsedTime + 4 + Math.random() * 4;
            }
            if (elapsedTime >= this.redNextBlinkTime) {
                this.redBlinkStartTime = elapsedTime;
                this.redNextBlinkTime = elapsedTime + 4 + Math.random() * 5;
            }
        }
    }

    // Central Sphere Pulse with Audio / State
    if (this.redCoreSphere) {
        let coreScale = 1.0;
        if (isSpeaking) {
            coreScale = 1.0 + audioIntensity * 1.3;
        } else if (isThinking) {
            coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.3;
        } else {
            coreScale = 1.0 + clickPulse * 0.35;
        }
        this.redCoreSphere.scale.set(coreScale, coreScale, coreScale);
    }

    if (this.redLensOuter) {
        const lensSpin = isThinking ? 0.02 : (isSpeaking ? 0.012 : clickPulse * 0.02);
        this.redLensOuter.rotation.y += lensSpin;
        this.redLensOuter.rotation.x += lensSpin * 0.6;
        const lensScale = isSpeaking ? 1.0 + audioIntensity * 0.6 : 1.0 + clickPulse * 0.2;
        this.redLensOuter.scale.set(lensScale, lensScale, lensScale);
    }

    // Eyelid arcs stay put, cupping the core, aside from a quick blink: both lids scale
    // toward the eye's equator (y = 0) and back, meeting in the middle over the core sphere
    // for an instant -- triggered by a click or, at rest, on their own every few seconds
    // (see the idle branch above). A faint audio-reactive opacity flicker continues while
    // speaking, brightened further for the instant of a blink.
    const BLINK_DURATION = 0.28;
    const blinkAge = elapsedTime - this.redBlinkStartTime;
    const blinkCloseness = (blinkAge >= 0 && blinkAge < BLINK_DURATION)
        ? Math.sin((blinkAge / BLINK_DURATION) * Math.PI)
        : 0;
    const lidScaleY = 1 - blinkCloseness * 0.96;
    const lidOpacity = (isSpeaking ? 0.85 + audioIntensity * 0.15 : 0.85) + blinkCloseness * 0.15;
    if (this.redBlueCircle) {
        this.redBlueCircle.scale.y = lidScaleY;
        this.redBlueCircle.material.opacity = lidOpacity;
    }
    if (this.redCyanCircle) {
        this.redCyanCircle.scale.y = lidScaleY;
        this.redCyanCircle.material.opacity = lidOpacity;
    }

    // Lens glow + eye light -- pulse together with the same state-driven signal used
    // elsewhere on this avatar, plus a bright pop at the peak of a blink (a glint off the
    // lens as the eyelids sweep past it).
    let glowIntensity;
    if (isSpeaking) {
        glowIntensity = 0.6 + audioIntensity * 1.3 + Math.sin(elapsedTime * 14) * 0.08;
    } else if (isThinking) {
        glowIntensity = 0.6 + Math.sin(elapsedTime * 16) * 0.35;
    } else {
        glowIntensity = 0.5 + clickPulse * 0.4;
    }
    glowIntensity += blinkCloseness * 0.5;

    if (this.redLensGlow) {
        const glowScale = 30 * (1 + glowIntensity * 0.18);
        this.redLensGlow.scale.set(glowScale, glowScale, 1);
        this.redLensGlow.material.opacity = Math.min(1, 0.35 + glowIntensity * 0.4);
    }
    if (this.redEyeLight) {
        this.redEyeLight.intensity = 2 + glowIntensity * 5;
    }
};

// ==============================================================
// THE NEXUS CREW (v3): WIREFRAME HULL + CHAIN TENTACLES + NEXUS LETTER RAIN
// ==============================================================
HologramAvatar.prototype.animateNexus = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';
    const mouseIdleFor = elapsedTime - this.lastMouseMoveTime;
    const mouseActive = mouseIdleFor < 1.2;
    // Idle and alone, the Crew just drifts (jellyfish-like tentacle sway, calm head bob).
    // The moment it's thinking, speaking, or actively tracking the cursor, it counts as
    // "aggressive": tentacles spread and point forward at whatever it's locked onto.
    const isAggressive = isThinking || isSpeaking || mouseActive;

    // Rain falls straight down and wraps top-to-bottom -- ambient background, always
    // active (it's the scene behind the creature, not the creature reacting). The radar
    // grid sprite behind it sweeps slowly and continuously, like a live CRT scan.
    if (this.nexusGridMat) this.nexusGridMat.rotation += 0.0015;
    const rainSpeedMult = isThinking ? 1.8 : (isSpeaking ? 1.3 : 1.0);
    this.nexusRainDrops.forEach(drop => {
        drop.position.y -= drop.userData.speed * 0.016 * rainSpeedMult;
        if (drop.position.y < -110) {
            drop.position.y = 110;
            drop.position.x = (Math.random() - 0.5) * 260;
        }
        // Mostly-dim field with occasional brighter glyphs standing out, rather than a
        // smooth sine flicker that spends equal time bright and dim. Raising a clamped sine
        // to a power keeps it near the dim floor most of the cycle and only spikes toward
        // the ceiling briefly, near the peak -- reads as a live sensor feed's static.
        const flicker = Math.max(0, Math.sin(elapsedTime * 4 + drop.userData.flickerPhase));
        drop.material.opacity = 0.22 + Math.pow(flicker, 6) * 0.68;
    });

    // A shared, fast, low-amplitude jitter applied to every wireframe/point material's
    // opacity below -- the "slight flickering... live sensor feed" digital-noise look
    // asked for, without needing a full post-processing shader pass.
    const crtFlicker = 0.88 + 0.12 * Math.sin(elapsedTime * 41 + Math.sin(elapsedTime * 6.7) * 3);

    // Looks around while thinking, gives a slight attentive tilt while speaking, and
    // tracks the cursor directly while it's active over the viewport. The instant none of
    // that applies, the head eases into a slow, gentle drift instead of hunting for
    // anything -- the calm half of the idle/aggressive split.
    if (this.nexusCreatureGroup) {
        let targetYaw = 0;
        let targetPitch = 0;
        let targetRoll = 0;
        if (isThinking) {
            targetYaw = Math.sin(elapsedTime * 0.8) * 0.35;
            targetPitch = Math.cos(elapsedTime * 0.6) * 0.2;
        } else if (isSpeaking) {
            targetYaw = Math.sin(elapsedTime * 1.4) * 0.15;
            targetPitch = Math.sin(elapsedTime * 1.1) * 0.1;
        } else if (mouseActive) {
            targetYaw = this.nexusMouseNX * 0.55;
            targetPitch = -this.nexusMouseNY * 0.38;
        } else {
            const driftPhase = elapsedTime * 0.18;
            targetYaw = Math.sin(driftPhase) * 0.14;
            targetPitch = Math.sin(driftPhase * 0.7 + 1.1) * 0.1;
            targetRoll = Math.sin(driftPhase * 0.5 + 0.6) * 0.06;
        }
        targetYaw += clickPulse * 0.3;
        targetPitch -= clickPulse * 0.15;
        this.nexusFacing.yaw += (targetYaw - this.nexusFacing.yaw) * 0.05;
        this.nexusFacing.pitch += (targetPitch - this.nexusFacing.pitch) * 0.05;
        this.nexusFacing.roll += (targetRoll - this.nexusFacing.roll) * 0.05;
        this.nexusCreatureGroup.rotation.y = this.nexusFacing.yaw;
        this.nexusCreatureGroup.rotation.x = this.nexusFacing.pitch;
        this.nexusCreatureGroup.rotation.z = this.nexusFacing.roll;
        this.nexusCreatureGroup.position.y = clickPulse * 6;
    }

    // The hull's fill/wireframe/outline/points all share one geometry but are separate
    // siblings (not parent/child), so the "breathing" pulse is applied to all four
    // explicitly, and the flicker/depth-cue opacities are re-applied every frame.
    if (this.nexusHeadMesh) {
        let headScale = 1.0;
        if (isSpeaking) {
            headScale = 1.0 + audioIntensity * 0.3;
        } else if (isThinking) {
            headScale = 1.0 + Math.sin(elapsedTime * 10) * 0.08;
        } else {
            headScale = 1.0 + clickPulse * 0.12;
        }
        const headScaleY = headScale * this.nexusHeadHeightScale; // keep the oval (60%-height) proportions through the pulse
        [this.nexusHeadMesh, this.nexusHeadInnerWire, this.nexusHeadOutline, this.nexusHeadPoints].forEach(obj => {
            if (obj) obj.scale.set(headScale, headScaleY, headScale);
        });
        if (this.nexusHullInnerWireMat) this.nexusHullInnerWireMat.opacity = 0.22 * crtFlicker;
        if (this.nexusHeadOutlineMat) this.nexusHeadOutlineMat.opacity = 0.9 * crtFlicker;
        if (this.nexusHullPointsMat) this.nexusHullPointsMat.opacity = 0.85 * crtFlicker;
        if (this.nexusRibMat) this.nexusRibMat.opacity = 0.55 * crtFlicker;
    }

    // Sensor-node cluster blinks together every few seconds while idle -- a quick
    // vertical squash-and-recover. Held open while thinking/speaking/tracking so it
    // doesn't compete with the more reactive motion.
    if (this.nexusEyes.length) {
        let closeAmt = 0;
        if (!isAggressive) {
            const blinkPeriod = 4.2;
            const blinkWindow = 0.18;
            const blinkCycle = elapsedTime % blinkPeriod;
            if (blinkCycle > blinkPeriod - blinkWindow) {
                const t = (blinkCycle - (blinkPeriod - blinkWindow)) / blinkWindow;
                closeAmt = Math.sin(t * Math.PI);
            }
        }
        const eyeScaleY = 1 - closeAmt * 0.9;
        const ringSpinSpeed = isThinking ? 0.028 : (isSpeaking ? 0.02 : 0.008);
        if (this.nexusEyeRingMat) this.nexusEyeRingMat.rotation += ringSpinSpeed;
        this.nexusEyes.forEach(({ mesh, glow, ring, highlight }) => {
            mesh.scale.y = eyeScaleY;
            glow.scale.y = glow.scale.x * eyeScaleY;
            if (ring) ring.scale.y = ring.scale.x * eyeScaleY;
            if (highlight) highlight.scale.y = highlight.scale.x * eyeScaleY;
        });
    }

    // Tentacles: idle, a slow single-frequency wave curving them gently backward with a
    // light vertical bob -- jellyfish drifting in water. Aggressive (thinking, speaking,
    // or the cursor actively directing the head), the wave sharpens into angular
    // zigzagging bends, the tentacles spread wider, and they swing from trailing behind
    // toward pointing forward at the tracked target (the cursor, when available). Every
    // tentacle eases its own aggroT independently toward the shared target so the whole
    // cluster doesn't snap in lockstep.
    const aimX = mouseActive ? this.nexusMouseNX : 0;
    const aimY = mouseActive ? -this.nexusMouseNY : 0;
    const tailR = 6.5;
    const tailZ = -20;
    const idleSpeed = 0.9, aggroSpeed = 5.5;
    const idleSway = 0.55, aggroSway = 1.05;
    const idleSpreadPerAlong = 1.1, aggroSpreadPerAlong = 3.4;
    const idleForwardPerAlong = -2.1, aggroForwardPerAlong = 3.0;
    const idleSharpness = 1.0, aggroSharpness = 0.35;
    const tmpTip = new THREE.Vector3();
    const tmpPrev = new THREE.Vector3();
    const tmpLinkA = new THREE.Vector3();
    const tmpLinkB = new THREE.Vector3();
    const tmpLinkGap = new THREE.Vector3();
    const LINK_UP = new THREE.Vector3(0, 1, 0);
    const LINK_BASE_RADIUS = 0.55; // "slightly thicker" than the previous hairline
    const LINK_TIP_RADIUS = 0.22;
    this.nexusTentacles.forEach(tentacle => {
        tentacle.aggroT += ((isAggressive ? 1 : 0) - tentacle.aggroT) * 0.06;
        const at = tentacle.aggroT;
        const tSpeed = (idleSpeed + (aggroSpeed - idleSpeed) * at) * tentacle.speedMult;
        const swayScale = idleSway + (aggroSway - idleSway) * at;
        const spreadPerAlong = idleSpreadPerAlong + (aggroSpreadPerAlong - idleSpreadPerAlong) * at;
        const forwardPerAlong = idleForwardPerAlong + (aggroForwardPerAlong - idleForwardPerAlong) * at;
        const sharpness = idleSharpness + (aggroSharpness - idleSharpness) * at;

        const dirX = Math.cos(tentacle.baseAngle);
        const dirY = Math.sin(tentacle.baseAngle) * 0.6;
        const perpX = -dirY;
        const perpY = dirX;
        const segCount = tentacle.positions.length / 3;

        for (let sIdx = 0; sIdx < segCount; sIdx++) {
            const along = sIdx + 1;
            const wavePhase = elapsedTime * tSpeed + tentacle.baseAngle * 3 + tentacle.phaseSeed;
            const raw = Math.sin(wavePhase - along * 0.6);
            // sign(raw) * |raw|^sharpness == raw when sharpness is 1 (a plain smooth sine,
            // the idle/jellyfish case) and squares it toward an angular zigzag as sharpness
            // drops toward the aggressive value -- one formula covers both extremes and
            // everything eased between them.
            const shaped = Math.sign(raw) * Math.pow(Math.abs(raw), sharpness);
            const bend = shaped * along * swayScale;
            const bobV = Math.sin(wavePhase * 0.7 + along * 0.3 + 1.6) * along * swayScale * 0.5;

            const px = dirX * (tailR + along * spreadPerAlong) + perpX * bend + aimX * along * 1.6 * at;
            const py = dirY * (tailR * 0.6) - along * 0.4 * (1 - at) + bobV + aimY * along * 1.1 * at;
            const pz = tailZ + along * forwardPerAlong;

            tentacle.positions[sIdx * 3] = px;
            tentacle.positions[sIdx * 3 + 1] = py;
            tentacle.positions[sIdx * 3 + 2] = pz;
        }
        tentacle.geom.attributes.position.needsUpdate = true;
        tentacle.geom.computeBoundingSphere();

        // Chain links: one tapered cylinder per gap between consecutive joints, rescaled
        // and reoriented to span its two endpoints every frame -- the same technique the
        // Crew's earlier build used for its (since-removed) mandible rods.
        tentacle.links.forEach((link, lIdx) => {
            tmpLinkA.set(tentacle.positions[lIdx * 3], tentacle.positions[lIdx * 3 + 1], tentacle.positions[lIdx * 3 + 2]);
            tmpLinkB.set(tentacle.positions[(lIdx + 1) * 3], tentacle.positions[(lIdx + 1) * 3 + 1], tentacle.positions[(lIdx + 1) * 3 + 2]);
            tmpLinkGap.subVectors(tmpLinkB, tmpLinkA);
            const len = tmpLinkGap.length();
            const t = lIdx / (tentacle.links.length - 1); // 0 at the base, 1 at the tip
            const radius = LINK_BASE_RADIUS + (LINK_TIP_RADIUS - LINK_BASE_RADIUS) * t;
            link.position.copy(tmpLinkA).addScaledVector(tmpLinkGap, 0.5);
            link.scale.set(radius, len, radius);
            if (len > 0.0001) {
                link.quaternion.setFromUnitVectors(LINK_UP, tmpLinkGap.normalize());
            }
        });

        // Clawed pincers at the tip -- three prongs fanned around the last segment,
        // oriented to continue the tentacle's current direction of travel.
        tmpTip.set(tentacle.positions[(segCount - 1) * 3], tentacle.positions[(segCount - 1) * 3 + 1], tentacle.positions[(segCount - 1) * 3 + 2]);
        tmpPrev.set(tentacle.positions[(segCount - 2) * 3], tentacle.positions[(segCount - 2) * 3 + 1], tentacle.positions[(segCount - 2) * 3 + 2]);
        const dir = new THREE.Vector3().subVectors(tmpTip, tmpPrev).normalize();
        const arbitrary = Math.abs(dir.y) < 0.9 ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(1, 0, 0);
        const fanA = new THREE.Vector3().crossVectors(dir, arbitrary).normalize();
        const fanB = new THREE.Vector3().crossVectors(dir, fanA).normalize();
        tentacle.claws.forEach((claw, ci) => {
            const clawAngle = (ci / tentacle.claws.length) * Math.PI * 2;
            const spread = fanA.clone().multiplyScalar(Math.cos(clawAngle) * 1.1)
                .add(fanB.clone().multiplyScalar(Math.sin(clawAngle) * 1.1));
            claw.position.copy(tmpTip).addScaledVector(dir, 1.6).add(spread);
            claw.quaternion.setFromUnitVectors(
                new THREE.Vector3(0, 1, 0),
                dir.clone().addScaledVector(spread, 0.35).normalize()
            );
        });
    });
    if (this.nexusTentacleLinkMat) this.nexusTentacleLinkMat.opacity = 0.65 * crtFlicker;
    if (this.nexusTentaclePointMat) this.nexusTentaclePointMat.opacity = 0.9 * crtFlicker;
};

// ==========================================
// A.R.X.LIMES: FLOATING HUB + FRACTURED DOME PLATES
// ==========================================
HologramAvatar.prototype.animateArxLimes = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';

    if (this.arxLimesGroup) {
        if (isThinking || isSpeaking) {
            // No Y-axis spin -- the eye stays facing forward, only tilting to "look around".
            this.arxLimesGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.12;
            this.arxLimesGroup.rotation.z = Math.sin(elapsedTime * 0.3) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            this.arxLimesGroup.rotation.x += (clickPulse * 0.1 - this.arxLimesGroup.rotation.x) * 0.15;
            this.arxLimesGroup.rotation.z += (clickPulse * 0.08 - this.arxLimesGroup.rotation.z) * 0.15;
        }
    }

    if (this.arxLimesHubOutline) {
        let hubScale = 1.0;
        if (isSpeaking) {
            hubScale = 1.0 + audioIntensity * 0.9;
        } else if (isThinking) {
            hubScale = 1.0 + Math.sin(elapsedTime * 18) * 0.35;
        } else {
            hubScale = 1.0 + clickPulse * 0.3;
        }
        this.arxLimesHubOutline.scale.set(hubScale, hubScale, hubScale);
        if (this.arxLimesHubMesh) this.arxLimesHubMesh.scale.set(hubScale, hubScale, hubScale);

        const hubSpin = isThinking ? 0.03 : (isSpeaking ? 0.02 : clickPulse * 0.02);
        this.arxLimesHubOutline.rotation.x += hubSpin;
        if (this.arxLimesHubMesh) {
            this.arxLimesHubMesh.rotation.x = this.arxLimesHubOutline.rotation.x;
        }
    }

    // A click makes it blink -- the side wing plates flutter shut and open again,
    // like eyelashes blinking. Top/bottom "eyelids" stay still. No idle auto-blink.
    const blinkScale = 1.0 - clickPulse * 0.9;

    // Plates stay put -- static, floating in fixed position -- with only a faint
    // audio-reactive nudge while speaking, or a faint pop on click.
    this.arxLimesPlates.forEach((plate, idx) => {
        let radiusMult = 1.0;
        if (isSpeaking) {
            const fVal = (this.audioData[idx % 16] || 0) / 255;
            radiusMult = 1.0 + fVal * 0.06;
        } else if (clickPulse > 0) {
            radiusMult = 1.0 + clickPulse * 0.05;
        }
        const r = plate.baseRadius * radiusMult;
        plate.group.position.set(Math.cos(plate.baseAngle) * r, Math.sin(plate.baseAngle) * r, 0);

        if (plate.tier === 'wing') {
            // Collapse to a thin sliver and back -- closer to how a blinking eyelash
            // reads than shrinking the whole blade toward the hub.
            plate.group.scale.set(blinkScale, 1, 1);
        }
    });
};

// ==========================================
// HALCY / DEFAULT PARTICLE ANIMATIONS
// ==========================================
HologramAvatar.prototype.animateHalcy = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';

    if (this.particleSystem) {
        const positions = this.particleSystem.geometry.attributes.position.array;

        for (let i = 0; i < this.particleCount; i++) {
            const base = this.basePositions[i];
            let displacement = 0;

            if (isSpeaking) {
                const freqIdx = i % 32;
                const freqVal = (this.audioData[freqIdx] || 0) / 255;
                displacement = Math.sin(elapsedTime * 8 + i * 0.1) * (8 + freqVal * 25);
            } else if (isThinking) {
                displacement = Math.sin(elapsedTime * 12 + base.x * 0.2) * Math.cos(elapsedTime * 8 + base.y * 0.2) * 9;
            } else {
                // Slow, uniform breathing so the lattice never reads as frozen, plus
                // the click bump on top.
                displacement = clickPulse * 10 + Math.sin(elapsedTime * 0.5) * 2.5;
            }

            const scale = 1 + displacement / this.halcyLatticeRadius;
            positions[i * 3] = base.x * scale;
            positions[i * 3 + 1] = base.y * scale;
            positions[i * 3 + 2] = base.z * scale;
        }

        this.particleSystem.geometry.attributes.position.needsUpdate = true;

        let rotSpeed = clickPulse * 0.01;
        if (isThinking) rotSpeed = 0.025;
        if (isSpeaking) rotSpeed = 0.01;
        this.particleSystem.rotation.y += rotSpeed;

        if (isThinking || isSpeaking) {
            this.particleSystem.rotation.x = Math.sin(elapsedTime * 0.5) * 0.1;
            this.particleSystem.rotation.z = Math.cos(elapsedTime * 0.4) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            this.particleSystem.rotation.x += (clickPulse * 0.12 - this.particleSystem.rotation.x) * 0.15;
            this.particleSystem.rotation.z += (clickPulse * 0.1 - this.particleSystem.rotation.z) * 0.15;
        }
    }

    // Peak (not average) of the same 32-bin range the particles (freqIdx = i % 32, below)
    // and inner-ring segments (idx % 32, below) actually read while speaking -- using the
    // shared audioIntensity here instead (an average of only bins 0-15) let a loud bin
    // anywhere in 16-31 push a particle or segment out further than this bulge/envelope
    // accounted for, since that quieter-looking average under-reported the real peak.
    let maxSpeakingFreqVal = 0;
    if (isSpeaking) {
        for (let i = 0; i < 32; i++) {
            const v = (this.audioData[i] || 0) / 255;
            if (v > maxSpeakingFreqVal) maxSpeakingFreqVal = v;
        }
    }

    // How far the lattice is currently bulging beyond its resting radius -- used to push
    // both rings outward in step so they never overlap it, even mid-click or mid-speech.
    let latticeBulge = 2.5 + clickPulse * 10;
    if (isSpeaking) {
        latticeBulge = 8 + maxSpeakingFreqVal * 25;
    } else if (isThinking) {
        latticeBulge = 9;
    }

    // Inner ultramarine equalizer ring — each segment thickens along the circumference
    // to the live audio frequencies while speaking. The segments are children of the
    // rotating group, so the thickening pattern rotates together with the ring itself.
    let segReachEnvelope = 1.0 + clickPulse * 0.3;
    if (isSpeaking) {
        segReachEnvelope = 1.0 + maxSpeakingFreqVal * 2.4;
    } else if (isThinking) {
        segReachEnvelope = 1.35;
    }

    let innerRingSpinDelta = 0;
    if (this.halcyInnerRingGroup) {
        const targetInnerRadius = this.halcyLatticeRadius + this.halcyInnerRingGap + latticeBulge;
        this.halcyInnerRingRadius += (targetInnerRadius - this.halcyInnerRingRadius) * 0.12;

        this.halcyInnerSegments.forEach((seg, idx) => {
            let lenScale = 1.0;
            if (isSpeaking) {
                const fVal = (this.audioData[idx % 32] || 0) / 255;
                lenScale = 1.0 + fVal * 2.4;
            } else if (isThinking) {
                lenScale = 1.0 + Math.sin(elapsedTime * 14 + seg.userData.angle * 6) * 0.35;
            } else {
                lenScale = 1.0 + clickPulse * 0.3;
            }
            seg.scale.y = lenScale;
            seg.position.set(
                Math.cos(seg.userData.angle) * this.halcyInnerRingRadius,
                Math.sin(seg.userData.angle) * this.halcyInnerRingRadius,
                0
            );
        });

        const spinMultiplier = isThinking ? 3.0 : (isSpeaking ? 1.6 : 0.4 + clickPulse * 1.1);
        innerRingSpinDelta = this.halcyInnerRingGroup.userData.speed * spinMultiplier;
        this.halcyInnerRingGroup.rotation.z += innerRingSpinDelta;

        if (isSpeaking) {
            this.halcyInnerRingGroup.rotation.x = Math.sin(elapsedTime * 2.4) * 0.24;
        } else if (isThinking) {
            this.halcyInnerRingGroup.rotation.x = Math.sin(elapsedTime * 0.6) * 0.05;
        } else {
            this.halcyInnerRingGroup.rotation.x += (clickPulse * 0.1 - this.halcyInnerRingGroup.rotation.x) * 0.15;
        }
    }

    // Static outer ring — kept perfectly flat (a true circle, not tilted into an ellipse),
    // spins clockwise at exactly half the inner ring's current speed (so it stays in visible
    // lockstep through every state), and stays a little tighter to the inner ring than before
    // while still tracking its outward dodge. Its baked-in swell rides around the
    // circumference as it spins, reading as a wave of motion.
    if (this.halcyOuterRing) {
        const targetOuterRadius = this.halcyInnerRingRadius + 6 * segReachEnvelope + this.halcyOuterRingGap;
        this.halcyOuterRingRadius += (targetOuterRadius - this.halcyOuterRingRadius) * 0.12;
        this.halcyOuterRing.scale.setScalar(this.halcyOuterRingRadius / this.halcyOuterRingBaseRadius);

        this.halcyOuterRing.rotation.z -= innerRingSpinDelta * 0.5;
    }

    if (this.coreOrb) {
        let coreScale = 1.0;
        if (isSpeaking) {
            coreScale = 1.0 + audioIntensity * 0.5;
        } else if (isThinking) {
            coreScale = 1.0 + Math.sin(elapsedTime * 15) * 0.2;
        } else {
            coreScale = 1.0 + clickPulse * 0.25;
        }
        this.coreOrb.scale.set(coreScale, coreScale, coreScale);

        const orbSpin = isThinking ? 0.05 : (isSpeaking ? 0.03 : clickPulse * 0.03);
        this.coreOrb.rotation.y -= orbSpin;
    }
};

// ==============================================================
// A1TER_NUL (CUNNINGHAM): BLACK WALL GLASS-SHARD EQUALIZER + FIREWALL/ICE RING
// ==============================================================
HologramAvatar.prototype.animateAlt = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';
    const isAlert = isThinking || isSpeaking;

    if (this.altGroup) {
        if (isAlert) {
            this.altGroup.rotation.y = Math.sin(elapsedTime * 0.5) * 0.12;
            this.altGroup.rotation.x = Math.sin(elapsedTime * 0.35) * 0.08;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            this.altGroup.rotation.y += (clickPulse * 0.1 - this.altGroup.rotation.y) * 0.15;
            this.altGroup.rotation.x += (clickPulse * 0.06 - this.altGroup.rotation.x) * 0.15;
        }
    }

    // Whole-stack breathing pulse -- subtle, so it doesn't fight the per-shard equalizer.
    if (this.altShardGroup) {
        const stackPulse = 1.0 + audioIntensity * 0.03 + clickPulse * 0.04;
        this.altShardGroup.scale.set(stackPulse, stackPulse, stackPulse);
    }

    // Each shard's glowing edge is its own equalizer bar: idle shards shimmer gently and
    // out of phase with each other (an inert relic still flickering faintly); speaking
    // drives each shard's brightness off its own audio frequency bin, so the stack reads
    // as a real spectrum analyzer built out of broken glass; thinking is a fast, uniform
    // flicker across the whole stack instead of a per-bin readout.
    this.altShards.forEach(shard => {
        let opacity;
        if (isSpeaking) {
            const fVal = (this.audioData[shard.binIndex] || 0) / 255;
            opacity = 0.2 + fVal * 0.85 + Math.sin(elapsedTime * 6 + shard.phase) * 0.04;
        } else if (isThinking) {
            opacity = shard.baseOpacity + Math.sin(elapsedTime * 13 + shard.phase) * 0.35;
        } else {
            opacity = shard.baseOpacity + Math.sin(elapsedTime * 1.1 + shard.phase) * 0.15 + clickPulse * 0.3;
        }
        shard.outlineMat.opacity = Math.max(0.05, Math.min(1, opacity));
    });

    // Firewall perimeter ring -- a slow idle scan that snaps into a fast active sweep.
    if (this.altFirewallRing) {
        const spinSpeed = isThinking ? 0.03 : (isSpeaking ? 0.02 : 0.004 + clickPulse * 0.01);
        this.altFirewallRing.rotation.z += spinSpeed;
        const ringOpacity = isSpeaking ? 0.5 + audioIntensity * 0.4 : (isThinking ? 0.5 + Math.sin(elapsedTime * 10) * 0.25 : 0.4 + clickPulse * 0.3);
        this.altFirewallRingMat.opacity = ringOpacity;
    }

    // Shield/ICE tiles -- a sequential scan sweep around the perimeter, sped up while alert.
    if (this.altShieldGroup) {
        const groupSpin = isAlert ? 0.012 : 0.0025;
        this.altShieldGroup.rotation.z -= groupSpin;
    }
    const scanSpeed = isAlert ? 6.0 : 2.0;
    this.altShieldTiles.forEach(tile => {
        const sweep = 0.5 + 0.5 * Math.sin(elapsedTime * scanSpeed - tile.userData.phase);
        const litFactor = isAlert ? sweep : sweep * 0.5;
        const tileScale = 0.85 + litFactor * 0.45 + clickPulse * 0.15;
        tile.scale.set(tileScale, tileScale, tileScale);
    });
};

// A1 (the monogram placeholder) is not a built-in avatar dispatched from here -- it is
// registered through HologramAvatar.registerAvatar() like any other avatar file (see
// avatar-a1.js and js/hologram/README.md), so its own animate() runs via the plugin path
// at the top of animate() above instead of an animateX() branch in this list.
