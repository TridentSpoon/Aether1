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

    // Static outer ring — fixed cyan, unaffected by color theme
    const outerRingGeom = new THREE.RingGeometry(76, 78.5, 64);
    const outerRingMat = new THREE.MeshBasicMaterial({
        color: 0x00f0ff,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.5,
        blending: THREE.AdditiveBlending
    });
    this.halcyOuterRing = new THREE.Mesh(outerRingGeom, outerRingMat);
    this.halcyOuterRing.rotation.x = 0.6;
    this.halcyOuterRing.rotation.y = 0.2;
    this.halcyOuterRing.userData = { speed: 0.012, baseRotX: 0.6, baseRotY: 0.2 };
    this.scene.add(this.halcyOuterRing);

    // Inner ultramarine equalizer ring — segmented so its circumference can "thicken" per-bar
    // like an audio equalizer while speaking, and the whole ring can sway on its Z axis.
    this.halcyInnerRingGroup = new THREE.Group();
    this.halcyInnerRingGroup.userData = { speed: 0.01 };
    const innerRingRadius = 58;
    const segmentCount = 40;
    const segMat = new THREE.MeshBasicMaterial({
        color: 0x2b3eff,
        transparent: true,
        opacity: 0.85,
        blending: THREE.AdditiveBlending
    });
    for (let i = 0; i < segmentCount; i++) {
        const angle = (i / segmentCount) * Math.PI * 2;
        const segGeom = new THREE.BoxGeometry(2.4, 6, 1.6);
        segGeom.translate(0, 3, 0); // pivot at inner edge so it only extends outward when scaled
        const seg = new THREE.Mesh(segGeom, segMat);
        seg.position.set(Math.cos(angle) * innerRingRadius, Math.sin(angle) * innerRingRadius, 0);
        seg.rotation.z = angle - Math.PI / 2;
        seg.userData = { angle };
        this.halcyInnerSegments.push(seg);
        this.halcyInnerRingGroup.add(seg);
    }
    this.scene.add(this.halcyInnerRingGroup);
};
