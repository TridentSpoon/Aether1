// Avatar 1: hAlcy -- Harmonic Particle Lattice & Orbital Rings.

HologramAvatar.prototype.buildHalcyAvatar = function() {
    const geometry = new THREE.BufferGeometry();
    const positions = new Float32Array(this.particleCount * 3);
    const colors = new Float32Array(this.particleCount * 3);
    const radius = 34;
    this.halcyLatticeRadius = radius;

    for (let i = 0; i < this.particleCount; i++) {
        const phi = Math.acos(-1 + (2 * i) / this.particleCount);
        const theta = Math.sqrt(this.particleCount * Math.PI) * phi;

        const x = radius * Math.cos(theta) * Math.sin(phi);
        const y = radius * Math.sin(theta) * Math.sin(phi);
        const z = radius * Math.cos(phi);

        positions[i * 3] = x;
        positions[i * 3 + 1] = y;
        positions[i * 3 + 2] = z;

        this.basePositions.push({ x, y, z });

        colors[i * 3] = 0.0;
        colors[i * 3 + 1] = 0.85 + Math.random() * 0.15;
        colors[i * 3 + 2] = 1.0;
    }

    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

    const texture = this.createGlowSpriteTexture(32);

    const material = new THREE.PointsMaterial({
        size: 3.2,
        vertexColors: true,
        map: texture,
        transparent: true,
        opacity: 0.9,
        blending: THREE.AdditiveBlending,
        depthWrite: false
    });

    this.particleSystem = new THREE.Points(geometry, material);
    this.scene.add(this.particleSystem);

    // Obsidian inner sphere — solid glossy dark core, fixed color regardless of color theme
    const coreGeom = new THREE.SphereGeometry(18, 32, 32);
    const coreMat = new THREE.MeshPhongMaterial({
        color: 0x0a0a0f,
        specular: 0x8fa8ff,
        shininess: 90,
        transparent: true,
        opacity: 0.97
    });
    this.coreOrb = new THREE.Mesh(coreGeom, coreMat);
    this.scene.add(this.coreOrb);

    // Inner ring's resting radius (a few pixels clear of the lattice) and its baseline
    // outward reach once its equalizer segments are included -- both are re-derived every
    // frame in animate.js so the rings dodge outward in step with the lattice bulging
    // during clicks/thinking/speaking, instead of a fixed gap that would get swallowed.
    this.halcyInnerRingGap = 5;
    this.halcyInnerRingRadius = radius + this.halcyInnerRingGap;
    const restingSegReach = 6; // segment length before any equalizer lenScale is applied
    this.halcyOuterRingGap = 4;
    this.halcyOuterRingBaseRadius = this.halcyInnerRingRadius + restingSegReach + this.halcyOuterRingGap;
    this.halcyOuterRingRadius = this.halcyOuterRingBaseRadius;

    // Static outer ring — fixed cyan, unaffected by color theme. Authored at its resting
    // radius, then scaled at runtime to track halcyOuterRingRadius. Kept perfectly flat
    // (no x/y tilt) so it reads as a true circle around the inner ring rather than an ellipse.
    const outerRingThickness = 2.5;
    const outerRingGeom = new THREE.RingGeometry(
        this.halcyOuterRingBaseRadius - outerRingThickness / 2,
        this.halcyOuterRingBaseRadius + outerRingThickness / 2,
        128
    );

    // Bake a single broad, gradual swell into the band at local angle 0 -- since the mesh
    // spins on its Z axis every frame, this rides around the circumference as a traveling
    // wave that reads as motion, instead of the ring just looking like a static disc.
    // Only the outer edge moves for this -- the inner edge (facing the inner ring) stays put
    // so its distance from the inner ring never varies, and the band's thickness swells instead.
    const bulgeHalfAngle = Math.PI / 2.6;
    const bulgeMaxAmount = 3.5;
    const outerPos = outerRingGeom.attributes.position;
    for (let vi = 0; vi < outerPos.count; vi++) {
        const vx = outerPos.getX(vi);
        const vy = outerPos.getY(vi);
        const vAngle = Math.atan2(vy, vx);
        const baseR = Math.sqrt(vx * vx + vy * vy);
        const isOuterEdge = baseR > this.halcyOuterRingBaseRadius;
        let bulge = 0;
        if (isOuterEdge) {
            const angleDiff = Math.atan2(Math.sin(vAngle), Math.cos(vAngle));
            if (Math.abs(angleDiff) < bulgeHalfAngle) {
                bulge = bulgeMaxAmount * 0.5 * (1 + Math.cos((angleDiff / bulgeHalfAngle) * Math.PI));
            }
        }
        const r = baseR + bulge;
        outerPos.setXY(vi, Math.cos(vAngle) * r, Math.sin(vAngle) * r);
    }
    outerPos.needsUpdate = true;

    const outerRingMat = new THREE.MeshBasicMaterial({
        color: 0x00f0ff,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.5,
        blending: THREE.AdditiveBlending
    });
    this.halcyOuterRing = new THREE.Mesh(outerRingGeom, outerRingMat);
    this.scene.add(this.halcyOuterRing);

    // Inner ultramarine equalizer ring — segmented so its circumference can "thicken" per-bar
    // like an audio equalizer while speaking, and the whole ring can sway on its Z axis.
    // Dodges a few pixels clear of the lattice surface and spins slowly even at rest.
    this.halcyInnerRingGroup = new THREE.Group();
    this.halcyInnerRingGroup.userData = { speed: 0.01 };
    const innerRingRadius = this.halcyInnerRingRadius;
    const segmentCount = 40;
    const segMat = new THREE.MeshBasicMaterial({
        color: 0x2b3eff,
        vertexColors: true,
        transparent: true,
        opacity: 0.85,
        blending: THREE.AdditiveBlending
    });
    for (let i = 0; i < segmentCount; i++) {
        const angle = (i / segmentCount) * Math.PI * 2;
        const segGeom = new THREE.BoxGeometry(3.6, 6, 2.6);
        segGeom.translate(0, 3, 0); // pivot at inner edge so it only extends outward when scaled

        // Radial gradient -- brightest where the segment meets the lattice (local y = 0),
        // fading darker toward its outer tip (local y = 6).
        const segPos = segGeom.attributes.position;
        const segColors = new Float32Array(segPos.count * 3);
        for (let vi = 0; vi < segPos.count; vi++) {
            const t = THREE.MathUtils.clamp(segPos.getY(vi) / 6, 0, 1);
            const brightness = 1.0 - t * 0.7;
            segColors[vi * 3] = brightness;
            segColors[vi * 3 + 1] = brightness;
            segColors[vi * 3 + 2] = brightness;
        }
        segGeom.setAttribute('color', new THREE.BufferAttribute(segColors, 3));

        const seg = new THREE.Mesh(segGeom, segMat);
        seg.position.set(Math.cos(angle) * innerRingRadius, Math.sin(angle) * innerRingRadius, 0);
        seg.rotation.z = angle - Math.PI / 2;
        seg.userData = { angle };
        this.halcyInnerSegments.push(seg);
        this.halcyInnerRingGroup.add(seg);
    }
    this.scene.add(this.halcyInnerRingGroup);
};
