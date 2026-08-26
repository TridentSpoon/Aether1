// Avatar 3: The Nexus -- Squid/Brain Creature with Trailing Tentacles that Hunts the Cursor,
// Set Against Falling NEXUS Letter Rain.

HologramAvatar.prototype.buildNexusAvatar = function() {
    this.nexusGroup = new THREE.Group();

    // --- NEXUS letter rain: real glyphs (not abstract points), falling straight down
    // and wrapping top-to-bottom. This replaces the old inward-spiral vortex entirely. ---
    const letters = ['N', 'E', 'X', 'U', 'S'];
    this.nexusRainMaterials = letters.map(ch => new THREE.SpriteMaterial({
        map: this.createLetterSpriteTexture(ch),
        color: 0x00ff66,
        transparent: true,
        opacity: 0.72, // 10% dimmer than the original 0.8
        blending: THREE.AdditiveBlending,
        depthWrite: false
    }));

    const rainCount = 90;
    const rainHalfWidth = 130;
    const rainTopY = 110;
    const rainBottomY = -110;
    for (let i = 0; i < rainCount; i++) {
        const mat = this.nexusRainMaterials[Math.floor(Math.random() * letters.length)];
        const sprite = new THREE.Sprite(mat);
        const scale = 9 + Math.random() * 7;
        sprite.scale.set(scale, scale, 1);
        sprite.position.set(
            (Math.random() - 0.5) * rainHalfWidth * 2,
            rainTopY + Math.random() * (rainTopY - rainBottomY),
            (Math.random() - 0.5) * 110
        );
        sprite.userData = {
            speed: 26 + Math.random() * 38,
            flickerPhase: Math.random() * Math.PI * 2
        };
        this.nexusRainDrops.push(sprite);
        this.nexusGroup.add(sprite);
    }

    // --- Squid/brain creature: a mantle-like head with trailing tentacles, held in its
    // own sub-group so it can rotate independently without spinning the rain field
    // along with it. Sits front-facing toward the camera (+Z) at rest. ---
    this.nexusCreatureGroup = new THREE.Group();

    const headGeom = new THREE.IcosahedronGeometry(17, 1);
    headGeom.scale(1, 0.82, 1.2);

    // Vertical gradient (dim underside fading to a bright crown) via vertex colors --
    // this multiplies with material.color, so theme tinting (see applyColorPalette in
    // core.js) still works, it just tints a gradient instead of a flat fill.
    const headPos = headGeom.attributes.position;
    const headBounds = new THREE.Box3().setFromBufferAttribute(headPos);
    const headMinY = headBounds.min.y, headMaxY = headBounds.max.y;
    const headColors = new Float32Array(headPos.count * 3);
    for (let i = 0; i < headPos.count; i++) {
        const t = (headPos.getY(i) - headMinY) / (headMaxY - headMinY); // 0 (bottom) .. 1 (top)
        const lum = 0.32 + t * 0.68;
        headColors[i * 3] = lum;
        headColors[i * 3 + 1] = lum;
        headColors[i * 3 + 2] = lum;
    }
    headGeom.setAttribute('color', new THREE.BufferAttribute(headColors, 3));

    // Solid, gradient-shaded, and normally blended (not additive) so it reads as the
    // clear visual focus, occluding the wireframe tentacles trailing behind it instead
    // of competing with them for attention.
    const headMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, vertexColors: true, transparent: true, opacity: 0.92
    });
    this.nexusHeadMesh = new THREE.Mesh(headGeom, headMat);
    this.nexusCreatureGroup.add(this.nexusHeadMesh);

    const headEdges = new THREE.EdgesGeometry(headGeom, 15);
    const headOutlineMat = new THREE.LineBasicMaterial({ color: 0x00ffaa, transparent: true, opacity: 0.9 });
    this.nexusHeadOutline = new THREE.LineSegments(headEdges, headOutlineMat);
    this.nexusCreatureGroup.add(this.nexusHeadOutline);

    // Tentacles trail behind the head, away from the camera (local -Z), so the head
    // faces the viewer (+Z) and the tentacles correctly read as further back in depth.
    // Wireframed rather than solid-filled so they read as thin structure/motion lines
    // and never visually compete with or cover the gradient-shaded head.
    const tentacleCount = 6;
    const segmentsPerTentacle = 7;
    this.nexusTentacleMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, wireframe: true
    });
    for (let t = 0; t < tentacleCount; t++) {
        const baseAngle = (t / tentacleCount) * Math.PI * 2;
        const segments = [];
        for (let s = 0; s < segmentsPerTentacle; s++) {
            const size = 4.5 * (1 - s / segmentsPerTentacle) + 1;
            const geom = new THREE.SphereGeometry(size, 8, 8);
            const seg = new THREE.Mesh(geom, this.nexusTentacleMat);
            this.nexusCreatureGroup.add(seg);
            segments.push(seg);
        }
        this.nexusTentacles.push({
            segments,
            baseAngle,
            spreadRadius: 9,
            // Per-tentacle speed/phase variance so they writhe independently rather
            // than moving as one synchronized wave.
            speedMult: 0.8 + Math.random() * 0.5,
            phaseSeed: Math.random() * Math.PI * 2
        });
    }

    this.nexusGroup.add(this.nexusCreatureGroup);
    this.scene.add(this.nexusGroup);
};
