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

    if (this.currentAvatar === 'arx-logos') {
        this.animateArxLogos(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'red' || this.currentAvatar === 'crimson') {
        this.animateRed9000(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'nexus' || this.currentAvatar === 'matrix') {
        this.animateNexus(elapsedTime, audioIntensity, clickPulse);
    } else if (this.currentAvatar === 'arx-limes') {
        this.animateArxLimes(elapsedTime, audioIntensity, clickPulse);
    } else {
        this.animateHalcy(elapsedTime, audioIntensity, clickPulse);
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
            // Front-facing and still at rest -- only a faint click-triggered nod.
            this.arxLogosGroup.rotation.x += (clickPulse * 0.08 - this.arxLogosGroup.rotation.x) * 0.15;
            this.arxLogosGroup.rotation.y += (0 - this.arxLogosGroup.rotation.y) * 0.15;
        }
    }

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

    // Arm hexagons — energy pulses outward along each arm while speaking/thinking,
    // otherwise still, with just a brief pulse on click.
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
    });

    // Outer dotted boundary ring — shimmers while speaking, otherwise still.
    this.arxLogosOuterDots.forEach(dot => {
        const shimmer = isSpeaking
            ? 1.0 + Math.sin(elapsedTime * 2 + dot.userData.phase) * 0.35
            : 1.0 + clickPulse * 0.25;
        dot.scale.set(shimmer, shimmer, shimmer);
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
            // Front-facing and still at rest -- only a faint click-triggered nod.
            this.redGroup.rotation.y += (clickPulse * 0.12 - this.redGroup.rotation.y) * 0.15;
            this.redGroup.rotation.x += (clickPulse * 0.08 - this.redGroup.rotation.x) * 0.15;
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

    // Eyelid arcs stay static, cupping the core -- only a faint audio-reactive
    // opacity flicker while speaking, no continuous rotation.
    const lidOpacity = isSpeaking ? 0.85 + audioIntensity * 0.15 : 0.85;
    if (this.redBlueCircle) this.redBlueCircle.material.opacity = lidOpacity;
    if (this.redCyanCircle) this.redCyanCircle.material.opacity = lidOpacity;
};

// ==============================================================
// THE NEXUS: SQUID/BRAIN FACING FORWARD + NEXUS LETTER RAIN
// ==============================================================
HologramAvatar.prototype.animateNexus = function(elapsedTime, audioIntensity, clickPulse) {
    const isThinking = this.state === 'THINKING';
    const isSpeaking = this.state === 'SPEAKING';

    // Rain falls straight down and wraps top-to-bottom -- ambient background, always
    // active (it's the scene behind the creature, not the creature reacting).
    const rainSpeedMult = isThinking ? 1.8 : (isSpeaking ? 1.3 : 1.0);
    this.nexusRainDrops.forEach(drop => {
        drop.position.y -= drop.userData.speed * 0.016 * rainSpeedMult;
        if (drop.position.y < -110) {
            drop.position.y = 110;
            drop.position.x = (Math.random() - 0.5) * 260;
        }
        // Mostly-dim field with occasional brighter glyphs standing out (reference: a dense
        // rain grid where a few characters flash brighter against a dim majority), rather
        // than a smooth sine flicker that spends equal time bright and dim. Raising a
        // clamped sine to a power keeps it near the dim floor most of the cycle and only
        // spikes toward the ceiling briefly, near the peak.
        const flicker = Math.max(0, Math.sin(elapsedTime * 4 + drop.userData.flickerPhase));
        drop.material.opacity = 0.22 + Math.pow(flicker, 6) * 0.68;
    });

    // Looks around while thinking, gives a slight attentive tilt while speaking. Otherwise
    // the pointer can steer the head directly while it's active over the viewport; the
    // instant it stops (mouseleave included), the head falls back to an autonomous
    // "hunting" search. isHunting/huntPhase are read again below in the tentacle loop, which
    // layers a phase-lagged version of this same writhe onto every segment of every tentacle
    // -- so the head doesn't just rotate as one rigid block while the tentacles trail along
    // for the ride, but the whole body (head through tentacle tips) bends as one continuous,
    // snake-like curve, each point further back lagging a little more than the one before it.
    let isHunting = false;
    let huntPhase = 0;
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
        } else {
            const mouseIdleFor = elapsedTime - this.lastMouseMoveTime;
            if (mouseIdleFor < 1.2) {
                targetYaw = this.nexusMouseNX * 0.55;
                targetPitch = -this.nexusMouseNY * 0.38;
            } else {
                isHunting = true;
                huntPhase = elapsedTime * 0.3;
                targetYaw = Math.sin(huntPhase) * 0.4 + Math.sin(huntPhase * 2.3 + 1.1) * 0.22;
                targetPitch = Math.sin(huntPhase * 0.6 + 1.2) * 0.18 + Math.sin(huntPhase * 1.7 + 0.4) * 0.12;
                targetRoll = Math.sin(huntPhase * 1.4 + 0.8) * 0.15;
            }
            targetYaw += clickPulse * 0.3;
            targetPitch -= clickPulse * 0.15;
        }
        this.nexusFacing.yaw += (targetYaw - this.nexusFacing.yaw) * 0.06;
        this.nexusFacing.pitch += (targetPitch - this.nexusFacing.pitch) * 0.06;
        this.nexusFacing.roll += (targetRoll - this.nexusFacing.roll) * 0.06;
        this.nexusCreatureGroup.rotation.y = this.nexusFacing.yaw;
        this.nexusCreatureGroup.rotation.x = this.nexusFacing.pitch;
        this.nexusCreatureGroup.rotation.z = this.nexusFacing.roll;
        // Perched on its own curled-forward tentacles during the click reaction (see the
        // tentacle loop below) -- the body lifts to sell standing up on them.
        this.nexusCreatureGroup.position.y = clickPulse * 6;
    }

    if (this.nexusHeadMesh) {
        let headScale = 1.0;
        if (isSpeaking) {
            headScale = 1.0 + audioIntensity * 0.3;
        } else if (isThinking) {
            headScale = 1.0 + Math.sin(elapsedTime * 10) * 0.08;
        } else {
            headScale = 1.0 + clickPulse * 0.12;
        }
        this.nexusHeadMesh.scale.set(headScale, headScale, headScale);
        if (this.nexusHeadOutline) this.nexusHeadOutline.scale.set(headScale, headScale, headScale);
    }

    // Eye-lens cluster blinks together every few seconds while idle -- a quick vertical
    // squash-and-recover. Held open while thinking/speaking so it doesn't compete with
    // the more reactive audio/thought motion.
    if (this.nexusEyes.length) {
        let closeAmt = 0;
        if (!isThinking && !isSpeaking) {
            const blinkPeriod = 4.2;
            const blinkWindow = 0.18;
            const blinkCycle = elapsedTime % blinkPeriod;
            if (blinkCycle > blinkPeriod - blinkWindow) {
                const t = (blinkCycle - (blinkPeriod - blinkWindow)) / blinkWindow;
                closeAmt = Math.sin(t * Math.PI);
            }
        }
        const eyeScaleY = 1 - closeAmt * 0.9;
        // Bezel outlines spin slowly and continuously -- an animated scan rather than a
        // static cartoon ring -- picking up pace while thinking/speaking.
        const ringSpinSpeed = isThinking ? 0.028 : (isSpeaking ? 0.02 : 0.008);
        if (this.nexusEyeRingMat) this.nexusEyeRingMat.rotation += ringSpinSpeed;
        this.nexusEyes.forEach(({ mesh, glow, ring, highlight }) => {
            mesh.scale.y = eyeScaleY;
            glow.scale.y = glow.scale.x * eyeScaleY;
            if (ring) ring.scale.y = ring.scale.x * eyeScaleY;
            if (highlight) highlight.scale.y = highlight.scale.x * eyeScaleY;
        });
    }

    // Tentacles fan outward from the head and trail behind it (-Z, away from the
    // camera) so they read as further back in depth. Restless and independently
    // writhing at all times -- like a Sentinel's mechanical feelers -- rather than a
    // single synchronized wave, with more energy while speaking.
    // While hunting, each tentacle's wave picks up toward the same energy as
    // thinking/speaking (each already runs at its own speed/phase, so they ripple past
    // each other rather than moving in lockstep) -- closer to the many-limbed
    // paddling/rippling gait real Sentinels move with, instead of a gentle idle sway.
    // While THINKING, the last few segments of each tentacle curl into a small dish/rim
    // shape that independently pans and tilts, like a cluster of little satellite dishes
    // searching for a signal, instead of just whipping faster.
    // A click perches the creature up on its own tentacles -- they curl forward/under the
    // body to brace it (see the body lift in the facing block above) instead of trailing.
    const isReacting = isThinking || isSpeaking;
    const waveSpeed = isSpeaking ? 6 : (isThinking ? 4.5 : (isHunting ? 3.8 : 2.4));
    const swayAmp = isReacting ? 1.0 : (isHunting ? 0.95 : (0.65 + clickPulse * 0.45));
    const perchT = clickPulse;
    const dishSegCount = 5;
    const tmpAim = new THREE.Vector3();
    const tmpUp = new THREE.Vector3();
    const tmpBasisA = new THREE.Vector3();
    const tmpBasisB = new THREE.Vector3();
    const tmpDishAnchor = new THREE.Vector3();
    this.nexusTentacles.forEach(tentacle => {
        const dirX = Math.cos(tentacle.baseAngle);
        const dirY = Math.sin(tentacle.baseAngle) * 0.6;
        const perpX = -dirY;
        const perpY = dirX;
        const tSpeed = waveSpeed * tentacle.speedMult;
        const totalSegs = tentacle.segments.length;
        const dishStartIdx = totalSegs - dishSegCount;
        // Independent slow pan/tilt per tentacle so each "dish" searches on its own rather
        // than moving in lockstep.
        const scanPan = Math.sin(elapsedTime * 0.5 + tentacle.phaseSeed) * 0.5;
        const scanTilt = Math.sin(elapsedTime * 0.35 + tentacle.phaseSeed * 1.6) * 0.3;
        let dishAnchorSet = false;

        tentacle.segments.forEach((seg, sIdx) => {
            const along = sIdx + 1;
            const wavePhase = elapsedTime * tSpeed + tentacle.baseAngle * 3 + tentacle.phaseSeed;
            // Secondary, faster wriggle layered on the primary wave so each tentacle
            // coils and whips instead of tracing one clean sine curve.
            const wriggle = Math.sin(wavePhase * 1.8 + along * 1.3) * 0.4 * along;
            // The first 3 segments stay virtually straight back along -Z (a tight bundle
            // close to the head, minimal sway) for the elongated reference look; segments
            // 4-6 flare outward and ramp the sway back in, then behave as before.
            const flareT = Math.max(0, Math.min(1, (along - 3) / 3));
            // Segments feeding into a forming dish hold steadier than a whipping tentacle.
            const dishCalm = (isThinking && sIdx >= dishStartIdx - 2) ? 0.35 : 1;
            const sway = (Math.sin(wavePhase - along * 0.7) * (along * 0.9) + wriggle) * swayAmp * flareT * dishCalm;
            // Angled back more steeply than a wide sideways fan: less outward (XY) spread
            // per segment, more depth (-Z) per segment.
            const bundleRadius = 2.5;
            const fullOutDist = tentacle.spreadRadius + along * 2.4;
            const outDist = bundleRadius + (fullOutDist - bundleRadius) * flareT;
            let px = dirX * outDist + perpX * sway;
            let py = dirY * outDist - along * 1.0 + perpY * sway * 0.5;
            let pz = -(along * 7.2 + Math.sin(wavePhase * 0.6) * 2 * swayAmp * flareT * dishCalm);

            // Whole-body snake writhe while hunting: the same two-frequency wave driving the
            // head's yaw/roll above is echoed here as a lateral bend, phase-lagged more the
            // further a segment sits from the head, and growing in reach with distance --
            // so every tentacle curves in a coordinated S along its own length rather than
            // just being dragged along rigidly by the head's rotation.
            if (isHunting) {
                const bendLag = along * 0.12;
                const bendAngle = Math.sin(huntPhase - bendLag) * 0.4 + Math.sin(huntPhase * 2.3 - bendLag * 1.6 + 1.1) * 0.22;
                const bendReach = along * 1.3;
                px += perpX * bendAngle * bendReach;
                py += perpY * bendAngle * bendReach * 0.6;
            }

            if (isThinking && sIdx >= dishStartIdx) {
                if (!dishAnchorSet) {
                    tmpDishAnchor.set(px, py, pz);
                    dishAnchorSet = true;
                }
                tmpAim.set(dirX + scanPan * perpX, dirY + scanPan * perpY, -1 + scanTilt).normalize();
                tmpUp.set(0, 1, 0);
                if (Math.abs(tmpAim.dot(tmpUp)) > 0.9) tmpUp.set(1, 0, 0);
                tmpBasisA.crossVectors(tmpAim, tmpUp).normalize();
                tmpBasisB.crossVectors(tmpAim, tmpBasisA).normalize();

                const dishIdx = sIdx - dishStartIdx;
                const rimAngle = (dishIdx / dishSegCount) * Math.PI * 2 + tentacle.phaseSeed;
                const rimRadius = 2.5 + dishIdx * 0.5;
                const rimDepth = dishIdx * 0.6; // slight cup curvature -- rim segments sit a touch behind center
                const cosA = Math.cos(rimAngle);
                const sinA = Math.sin(rimAngle);
                px = tmpDishAnchor.x + (tmpBasisA.x * cosA + tmpBasisB.x * sinA) * rimRadius - tmpAim.x * rimDepth;
                py = tmpDishAnchor.y + (tmpBasisA.y * cosA + tmpBasisB.y * sinA) * rimRadius - tmpAim.y * rimDepth;
                pz = tmpDishAnchor.z + (tmpBasisA.z * cosA + tmpBasisB.z * sinA) * rimRadius - tmpAim.z * rimDepth;
            }

            if (perchT > 0.001) {
                const perchX = dirX * (3 + along * 1.1);
                const perchY = -6 - along * 2.6;
                const perchZ = 5 + along * 1.6;
                px = px * (1 - perchT) + perchX * perchT;
                py = py * (1 - perchT) + perchY * perchT;
                pz = pz * (1 - perchT) + perchZ * perchT;
            }

            seg.position.set(px, py, pz);
        });

        // Clawed talons at the tip -- three red prongs fanned around the last segment,
        // oriented to continue the tentacle's current direction of travel.
        if (tentacle.claws) {
            const tip = tentacle.segments[tentacle.segments.length - 1].position;
            const prevSeg = tentacle.segments[tentacle.segments.length - 2].position;
            const dir = new THREE.Vector3().subVectors(tip, prevSeg).normalize();
            const arbitrary = Math.abs(dir.y) < 0.9 ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(1, 0, 0);
            const fanA = new THREE.Vector3().crossVectors(dir, arbitrary).normalize();
            const fanB = new THREE.Vector3().crossVectors(dir, fanA).normalize();
            tentacle.claws.forEach((claw, ci) => {
                const clawAngle = (ci / tentacle.claws.length) * Math.PI * 2;
                const spread = fanA.clone().multiplyScalar(Math.cos(clawAngle) * 1.1)
                    .add(fanB.clone().multiplyScalar(Math.sin(clawAngle) * 1.1));
                claw.position.copy(tip).addScaledVector(dir, 1.6).add(spread);
                claw.quaternion.setFromUnitVectors(
                    new THREE.Vector3(0, 1, 0),
                    dir.clone().addScaledVector(spread, 0.35).normalize()
                );
            });
        }
    });

    // Front mandible/arm cluster -- short, restless segmented arms hanging from below the
    // head. Each hangs mostly downward with a distinct knee-like bend (outward at the
    // middle joint, curling back in at the foot) so the cluster reads as individual bent
    // legs, not one continuous sideways sweep. Rigid rods are stretched and rotated to
    // connect each consecutive pair every frame, which is what reads as a jointed arm
    // rather than a soft tentacle. A fast, tiny, always-on twitch is layered on top of the
    // slower wriggle -- reads as little mandibles working/chewing, not just idle sway.
    const legWaveSpeed = isSpeaking ? 5.5 : (isThinking ? 4 : (isHunting ? 3.2 : 2));
    const legSwayAmp = isReacting ? 0.7 : (isHunting ? 0.6 : (0.35 + clickPulse * 0.25));
    const legOutProfile = [0.55, 1.0, 0.6]; // hip -> knee -> foot; foot curls back toward center
    const legAnchor = new THREE.Vector3();
    const legGap = new THREE.Vector3();
    this.nexusLegs.forEach(leg => {
        const tSpeed = legWaveSpeed * leg.speedMult;
        const outAngle = leg.spread * 0.85;
        const outDirX = Math.sin(outAngle);
        const outDirZ = 0.3 + Math.cos(outAngle) * 0.15;
        legAnchor.set(leg.spread * 3, -8, 11);
        let prevPoint = legAnchor;
        leg.joints.forEach((joint, sIdx) => {
            const along = sIdx + 1;
            const outFactor = legOutProfile[sIdx] !== undefined ? legOutProfile[sIdx] : 1;
            const wavePhase = elapsedTime * tSpeed + leg.spread * 4 + leg.phaseSeed;
            const wriggle = Math.sin(wavePhase * 1.6 + along * 1.4) * 0.35 * legSwayAmp;
            const twitch = Math.sin(elapsedTime * 9 + leg.phaseSeed * 3 + along * 2.2) * 0.4 * along;
            joint.position.set(
                leg.spread * 3 + outDirX * (3 + outFactor * 6) + wriggle + twitch,
                -8 - along * 3.6,
                11 + outDirZ * along * 2.2
            );

            const rod = leg.rods[sIdx];
            legGap.subVectors(joint.position, prevPoint);
            const len = legGap.length();
            rod.position.copy(prevPoint).addScaledVector(legGap, 0.5);
            rod.scale.set(1, len, 1);
            if (len > 0.0001) {
                rod.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), legGap.clone().normalize());
            }

            prevPoint = joint.position;
        });
    });
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
