// Per-frame animation loop. animate() computes the shared per-frame values (elapsedTime,
// audioIntensity) then dispatches to exactly one animateX() method for the active avatar.

HologramAvatar.prototype.animate = function() {
    requestAnimationFrame(() => this.animate());

    const elapsedTime = this.clock.getElapsedTime();

    let audioSum = 0;
    for (let i = 0; i < 16; i++) {
        audioSum += this.audioData[i] || 0;
    }
    const audioIntensity = audioSum / (16 * 255);

    if (this.currentAvatar === 'arx-logos') {
        this.animateArxLogos(elapsedTime, audioIntensity);
    } else if (this.currentAvatar === 'red' || this.currentAvatar === 'crimson') {
        this.animateRed9000(elapsedTime, audioIntensity);
    } else if (this.currentAvatar === 'nexus' || this.currentAvatar === 'matrix') {
        this.animateNexus(elapsedTime, audioIntensity);
    } else if (this.currentAvatar === 'arx-limes') {
        this.animateArxLimes(elapsedTime, audioIntensity);
    } else {
        this.animateHalcy(elapsedTime, audioIntensity);
    }

    this.renderer.render(this.scene, this.camera);
};

// ==============================================================
// A.R.X.LOGOS: CENTRAL HEXAGON WITH SIX SPIRALING HEXAGON ARMS
// ==============================================================
HologramAvatar.prototype.animateArxLogos = function(elapsedTime, audioIntensity) {
    if (this.arxLogosGroup) {
        const spinSpeed = this.state === 'THINKING' ? 0.02 : (this.state === 'SPEAKING' ? 0.01 : 0.004);
        this.arxLogosGroup.rotation.z -= spinSpeed; // clockwise, matching the arm winding
        this.arxLogosGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.1 + this.mouseY;
        this.arxLogosGroup.rotation.y = Math.cos(elapsedTime * 0.35) * 0.1 + this.mouseX;
    }

    if (this.arxLogosCentralFill && this.arxLogosCentralOutline) {
        let coreScale = 1.0;
        if (this.state === 'SPEAKING') {
            coreScale = 1.0 + audioIntensity * 0.5;
        } else if (this.state === 'THINKING') {
            coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.18;
        } else {
            coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
        }
        this.arxLogosCentralFill.scale.set(coreScale, coreScale, coreScale);
        this.arxLogosCentralOutline.scale.set(coreScale, coreScale, coreScale);
    }

    // Arm hexagons — energy pulses outward along each arm while speaking, gentle
    // synchronized breathing otherwise.
    this.arxLogosArmHexes.forEach(hex => {
        let pulseFactor = 1.0;
        if (this.state === 'SPEAKING') {
            const fVal = (this.audioData[hex.userData.stepIndex % 16] || 0) / 255;
            pulseFactor = 1.0 + fVal * 0.6 + Math.sin(elapsedTime * 10 + hex.userData.phase) * 0.15;
        } else if (this.state === 'THINKING') {
            pulseFactor = 1.0 + Math.sin(elapsedTime * 12 + hex.userData.phase) * 0.3;
        } else {
            pulseFactor = 1.0 + Math.sin(elapsedTime * 2.5 + hex.userData.phase) * 0.08;
        }
        hex.scale.set(pulseFactor, pulseFactor, pulseFactor);
    });

    // Outer dotted boundary ring — subtle shimmer
    this.arxLogosOuterDots.forEach(dot => {
        const shimmer = 1.0 + Math.sin(elapsedTime * 2 + dot.userData.phase) * (this.state === 'SPEAKING' ? 0.35 : 0.15);
        dot.scale.set(shimmer, shimmer, shimmer);
    });
};

// ==============================================================
// R.E.D. 9000: CENTRAL SPHERE + TWO ORBIT CIRCLES
// ==============================================================
HologramAvatar.prototype.animateRed9000 = function(elapsedTime, audioIntensity) {
    if (this.redGroup) {
        this.redGroup.rotation.y = Math.sin(elapsedTime * 0.3) * 0.15 + this.mouseY;
        this.redGroup.rotation.x = Math.sin(elapsedTime * 0.2) * 0.1 + this.mouseX;
    }

    // Central Sphere Pulse with Audio / State
    if (this.redCoreSphere) {
        let coreScale = 1.0;
        if (this.state === 'SPEAKING') {
            coreScale = 1.0 + audioIntensity * 1.3;
        } else if (this.state === 'THINKING') {
            coreScale = 1.0 + Math.sin(elapsedTime * 16) * 0.3;
        } else {
            coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
        }
        this.redCoreSphere.scale.set(coreScale, coreScale, coreScale);
    }

    if (this.redLensOuter) {
        this.redLensOuter.rotation.y += 0.008;
        this.redLensOuter.rotation.x += 0.005;
        const lensScale = 1.0 + audioIntensity * 0.6;
        this.redLensOuter.scale.set(lensScale, lensScale, lensScale);
    }

    // Eyelid arcs stay static, cupping the core -- only a faint audio-reactive
    // opacity flicker while speaking, no continuous rotation.
    const lidOpacity = this.state === 'SPEAKING' ? 0.85 + audioIntensity * 0.15 : 0.85;
    if (this.redBlueCircle) this.redBlueCircle.material.opacity = lidOpacity;
    if (this.redCyanCircle) this.redCyanCircle.material.opacity = lidOpacity;
};

// ==============================================================
// THE NEXUS: SQUID/BRAIN HUNTING THE CURSOR + NEXUS LETTER RAIN
// ==============================================================
HologramAvatar.prototype.animateNexus = function(elapsedTime, audioIntensity) {
    // Rain falls straight down and wraps top-to-bottom -- no inward spiral/vortex.
    const rainSpeedMult = this.state === 'THINKING' ? 1.8 : (this.state === 'SPEAKING' ? 1.3 : 1.0);
    this.nexusRainDrops.forEach(drop => {
        drop.position.y -= drop.userData.speed * 0.016 * rainSpeedMult;
        if (drop.position.y < -110) {
            drop.position.y = 110;
            drop.position.x = (Math.random() - 0.5) * 260;
        }
        drop.material.opacity = 0.55 + Math.sin(elapsedTime * 4 + drop.userData.flickerPhase) * 0.25;
    });

    // Hunt the cursor: ease the creature's facing toward the mouse instead of
    // snapping to it, for a predatory tracking feel. No idle spin.
    if (this.nexusCreatureGroup) {
        const targetYaw = this.mouseX * 2.2;
        const targetPitch = this.mouseY * 1.6;
        this.nexusFacing.yaw += (targetYaw - this.nexusFacing.yaw) * 0.04;
        this.nexusFacing.pitch += (targetPitch - this.nexusFacing.pitch) * 0.04;
        this.nexusCreatureGroup.rotation.y = this.nexusFacing.yaw;
        this.nexusCreatureGroup.rotation.x = this.nexusFacing.pitch;
    }

    if (this.nexusHeadMesh) {
        let headScale = 1.0;
        if (this.state === 'SPEAKING') {
            headScale = 1.0 + audioIntensity * 0.3;
        } else if (this.state === 'THINKING') {
            headScale = 1.0 + Math.sin(elapsedTime * 10) * 0.08;
        } else {
            headScale = 1.0 + Math.sin(elapsedTime * 2) * 0.04;
        }
        this.nexusHeadMesh.scale.set(headScale, headScale, headScale);
        if (this.nexusHeadOutline) this.nexusHeadOutline.scale.set(headScale, headScale, headScale);
    }

    // Tentacles fan outward from the head along their own angle, each undulating
    // perpendicular to its own length, and all trailing backward (+Z, away from
    // whatever the head is currently facing) and slightly down, like flowing behind
    // a creature swimming through the code rain.
    const waveSpeed = this.state === 'SPEAKING' ? 6 : (this.state === 'THINKING' ? 4.5 : 3);
    this.nexusTentacles.forEach(tentacle => {
        const dirX = Math.cos(tentacle.baseAngle);
        const dirY = Math.sin(tentacle.baseAngle) * 0.6;
        const perpX = -dirY;
        const perpY = dirX;
        tentacle.segments.forEach((seg, sIdx) => {
            const along = sIdx + 1;
            const wavePhase = elapsedTime * waveSpeed + tentacle.baseAngle * 3;
            const sway = Math.sin(wavePhase - along * 0.7) * (along * 0.9);
            const outDist = tentacle.spreadRadius + along * 2.8;
            seg.position.set(
                dirX * outDist + perpX * sway,
                dirY * outDist - along * 1.0 + perpY * sway * 0.5,
                along * 4.0 + Math.sin(wavePhase * 0.6) * 2
            );
        });
    });
};

// ==========================================
// A.R.X.LIMES: FLOATING HUB + FRACTURED DOME PLATES
// ==========================================
HologramAvatar.prototype.animateArxLimes = function(elapsedTime, audioIntensity) {
    if (this.arxLimesGroup) {
        // No Y-axis spin -- the eye stays facing forward, only tilting to "look around".
        this.arxLimesGroup.rotation.x = Math.sin(elapsedTime * 0.4) * 0.12 + this.mouseY;
        this.arxLimesGroup.rotation.z = this.mouseX * 0.5;
    }

    if (this.arxLimesHubOutline) {
        let hubScale = 1.0;
        if (this.state === 'SPEAKING') {
            hubScale = 1.0 + audioIntensity * 0.9;
        } else if (this.state === 'THINKING') {
            hubScale = 1.0 + Math.sin(elapsedTime * 18) * 0.35;
        } else {
            hubScale = 1.0 + Math.sin(elapsedTime * 3) * 0.12;
        }
        this.arxLimesHubOutline.scale.set(hubScale, hubScale, hubScale);
        if (this.arxLimesHubMesh) this.arxLimesHubMesh.scale.set(hubScale, hubScale, hubScale);
        this.arxLimesHubOutline.rotation.x += 0.015;
        if (this.arxLimesHubMesh) {
            this.arxLimesHubMesh.rotation.x = this.arxLimesHubOutline.rotation.x;
        }
    }

    // A periodic stylised blink -- the side wing plates flutter shut and open again
    // every few seconds, like eyelashes blinking. Top/bottom "eyelids" stay still.
    const blinkCycle = 4.5;
    const blinkDuration = 0.28;
    const tInCycle = elapsedTime % blinkCycle;
    let blinkScale = 1.0;
    if (tInCycle < blinkDuration) {
        blinkScale = 1.0 - Math.sin((tInCycle / blinkDuration) * Math.PI) * 0.92;
    }

    // Plates stay put -- static, floating in fixed position -- with only a faint
    // audio-reactive nudge while speaking. No idle/thinking bob.
    this.arxLimesPlates.forEach((plate, idx) => {
        let radiusMult = 1.0;
        if (this.state === 'SPEAKING') {
            const fVal = (this.audioData[idx % 16] || 0) / 255;
            radiusMult = 1.0 + fVal * 0.06;
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
HologramAvatar.prototype.animateHalcy = function(elapsedTime, audioIntensity) {
    if (this.particleSystem) {
        const positions = this.particleSystem.geometry.attributes.position.array;

        for (let i = 0; i < this.particleCount; i++) {
            const base = this.basePositions[i];
            let displacement = 0;

            if (this.state === 'SPEAKING') {
                const freqIdx = i % 32;
                const freqVal = (this.audioData[freqIdx] || 0) / 255;
                displacement = Math.sin(elapsedTime * 8 + i * 0.1) * (8 + freqVal * 25);
            } else if (this.state === 'LISTENING') {
                displacement = Math.sin(elapsedTime * 6 - Math.sqrt(base.x**2 + base.y**2 + base.z**2) * 0.1) * 6;
            } else if (this.state === 'THINKING') {
                displacement = Math.sin(elapsedTime * 12 + base.x * 0.2) * Math.cos(elapsedTime * 8 + base.y * 0.2) * 9;
            } else {
                displacement = Math.sin(elapsedTime * 2 + base.y * 0.05) * 3;
            }

            const scale = 1 + displacement / this.halcyLatticeRadius;
            positions[i * 3] = base.x * scale;
            positions[i * 3 + 1] = base.y * scale;
            positions[i * 3 + 2] = base.z * scale;
        }

        this.particleSystem.geometry.attributes.position.needsUpdate = true;

        let rotSpeed = 0.004;
        if (this.state === 'THINKING') rotSpeed = 0.025;
        if (this.state === 'SPEAKING') rotSpeed = 0.01;

        this.particleSystem.rotation.y += rotSpeed;
        this.particleSystem.rotation.x = Math.sin(elapsedTime * 0.5) * 0.1 + this.mouseY;
        this.particleSystem.rotation.z = this.mouseX;
    }

    // Static outer ring — gentle idle rotation, fixed cyan
    if (this.halcyOuterRing) {
        const speedMultiplier = this.state === 'THINKING' ? 3.5 : (this.state === 'SPEAKING' ? 1.8 : 1.0);
        this.halcyOuterRing.rotation.z += this.halcyOuterRing.userData.speed * speedMultiplier;
        this.halcyOuterRing.rotation.x = this.halcyOuterRing.userData.baseRotX + Math.sin(elapsedTime * 0.8) * 0.08 + this.mouseY;
        this.halcyOuterRing.rotation.y = this.halcyOuterRing.userData.baseRotY + Math.cos(elapsedTime * 0.8) * 0.08 + this.mouseX;
    }

    // Inner ultramarine equalizer ring — each segment thickens along the circumference
    // to the live audio frequencies while speaking. The segments are children of the
    // rotating group, so the thickening pattern rotates together with the ring itself.
    this.halcyInnerSegments.forEach((seg, idx) => {
        let lenScale = 1.0;
        if (this.state === 'SPEAKING') {
            const fVal = (this.audioData[idx % 32] || 0) / 255;
            lenScale = 1.0 + fVal * 2.4;
        } else if (this.state === 'THINKING') {
            lenScale = 1.0 + Math.sin(elapsedTime * 14 + seg.userData.angle * 6) * 0.35;
        } else if (this.state === 'LISTENING') {
            lenScale = 1.0 + Math.sin(elapsedTime * 6 + seg.userData.angle * 4) * 0.15;
        } else {
            lenScale = 1.0 + Math.sin(elapsedTime * 2 + seg.userData.angle * 3) * 0.08;
        }
        seg.scale.y = lenScale;
    });

    if (this.halcyInnerRingGroup) {
        const spinMultiplier = this.state === 'THINKING' ? 3.0 : (this.state === 'SPEAKING' ? 1.6 : 1.0);
        this.halcyInnerRingGroup.rotation.z += this.halcyInnerRingGroup.userData.speed * spinMultiplier;

        const swayAmplitude = this.state === 'SPEAKING' ? 0.24 : 0.05;
        const swaySpeed = this.state === 'SPEAKING' ? 2.4 : 0.6;
        this.halcyInnerRingGroup.rotation.x = Math.sin(elapsedTime * swaySpeed) * swayAmplitude;
    }

    if (this.coreOrb) {
        let coreScale = 1.0;
        if (this.state === 'SPEAKING') {
            coreScale = 1.0 + audioIntensity * 0.5;
        } else if (this.state === 'THINKING') {
            coreScale = 1.0 + Math.sin(elapsedTime * 15) * 0.2;
        } else {
            coreScale = 1.0 + Math.sin(elapsedTime * 3) * 0.08;
        }
        this.coreOrb.scale.set(coreScale, coreScale, coreScale);
        this.coreOrb.rotation.y -= 0.02;
    }
};
