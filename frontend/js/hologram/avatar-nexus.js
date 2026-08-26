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
        opacity: 0.8,
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
    // own sub-group so it can rotate independently to hunt the cursor without spinning
    // the rain field along with it. ---
    this.nexusCreatureGroup = new THREE.Group();

    const headGeom = new THREE.IcosahedronGeometry(17, 1);
    headGeom.scale(1, 0.82, 1.2);
    const headMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.22, blending: THREE.AdditiveBlending
    });
    this.nexusHeadMesh = new THREE.Mesh(headGeom, headMat);
    this.nexusCreatureGroup.add(this.nexusHeadMesh);

    const headEdges = new THREE.EdgesGeometry(headGeom, 15);
    const headOutlineMat = new THREE.LineBasicMaterial({ color: 0x00ffaa, transparent: true, opacity: 0.9 });
    this.nexusHeadOutline = new THREE.LineSegments(headEdges, headOutlineMat);
    this.nexusCreatureGroup.add(this.nexusHeadOutline);

    // Tentacles trail behind (local +Z) while the head faces local -Z toward the cursor.
    const tentacleCount = 6;
    const segmentsPerTentacle = 7;
    this.nexusTentacleMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
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
        this.nexusTentacles.push({ segments, baseAngle, spreadRadius: 9 });
    }

    this.nexusGroup.add(this.nexusCreatureGroup);
    this.scene.add(this.nexusGroup);
};
