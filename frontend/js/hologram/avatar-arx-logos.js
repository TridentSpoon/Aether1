// Avatar 5: A.R.X.LOGOS -- Central Hexagon with Six Clockwise Spiraling Hexagon Arms &
// Dotted Hex Frame. The center reads as an eye (a fixed dark pupil + catchlight nested in
// the hex), and the arm hexes double as camera-aperture blades that periodically pull in
// and twist shut over it (see animateArxLogos). Two hex-frame rings and a small orbiting
// swarm add depth beyond the single flat plane everything else lives in.

HologramAvatar.prototype.buildArxLogosAvatar = function() {
    this.arxLogosGroup = new THREE.Group();

    // --- Centerpiece: large regular hexagon, flat top/bottom, pointy left/right ---
    const centralRadius = 26;
    const centralFillMat = new THREE.MeshBasicMaterial({
        color: 0xe024c3,
        transparent: true,
        opacity: 0.22,
        blending: THREE.AdditiveBlending
    });
    const centralOutlineMat = new THREE.LineBasicMaterial({
        color: 0xe024c3,
        transparent: true,
        opacity: 0.95
    });
    this.arxLogosCentralFill = this.buildHexFill(centralRadius, 0, centralFillMat);
    this.arxLogosCentralOutline = this.buildHexOutline(centralRadius, 0, centralOutlineMat);
    this.arxLogosGroup.add(this.arxLogosCentralFill);
    this.arxLogosGroup.add(this.arxLogosCentralOutline);

    // --- Core eye: a fixed dark pupil nested in the central hex, plus a small catchlight,
    // so the centerpiece reads as an eye rather than just a glowing panel -- fixed color
    // regardless of theme, same real-material convention as every other avatar's obsidian
    // core. The catchlight also flashes brighter at the peak of an aperture-close (below).
    // Both sit a hair in front of the central hex (explicit z, not just draw order) so they
    // never flicker against it in Three.js's transparent-object depth sort -- three flat
    // panes would otherwise sit at the exact same z.
    const pupilMat = new THREE.MeshBasicMaterial({ color: 0x050208, transparent: true, opacity: 0.92 });
    this.arxLogosPupil = this.buildHexFill(9, 0, pupilMat);
    this.arxLogosPupil.position.z = 0.5;
    this.arxLogosGroup.add(this.arxLogosPupil);

    const catchlightMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.5 });
    this.arxLogosCatchlight = this.buildHexFill(2.4, 0, catchlightMat, -4, 4);
    this.arxLogosCatchlight.position.z = 1;
    this.arxLogosGroup.add(this.arxLogosCatchlight);

    // --- Spiraling arms: 6 identical arms radiating from the central hex's 6 flat edges ---
    this.arxLogosArmMatNear = new THREE.LineBasicMaterial({ color: 0xe024c3, transparent: true, opacity: 0.9 });
    this.arxLogosArmMatMid = new THREE.LineBasicMaterial({ color: 0x9d00ff, transparent: true, opacity: 0.85 });
    this.arxLogosArmMatFar = new THREE.LineBasicMaterial({ color: 0xff00ff, transparent: true, opacity: 0.8 });

    const hexPerArm = 8;
    const apothem = centralRadius * Math.cos(Math.PI / 6); // distance from center to a flat edge

    for (let a = 0; a < 6; a++) {
        const baseAngle = (Math.PI / 6) + a * (Math.PI / 3); // edge-normal directions: 30,90,...,330 deg
        let curAngle = baseAngle;
        let hexSize = 15;
        let curDist = apothem + hexSize * 0.8;

        for (let i = 0; i < hexPerArm; i++) {
            const mat = i < 3 ? this.arxLogosArmMatNear : (i < 6 ? this.arxLogosArmMatMid : this.arxLogosArmMatFar);
            const x = Math.cos(curAngle) * curDist;
            const y = Math.sin(curAngle) * curDist;
            const hex = this.buildHexOutline(hexSize, 0, mat, x, y);
            hex.userData = { armIndex: a, stepIndex: i, baseX: x, baseY: y, phase: a * 0.9 + i * 0.5 };
            this.arxLogosArmHexes.push(hex);
            this.arxLogosGroup.add(hex);

            // Clockwise spiral: tight curve near the core, straightening toward the tail
            const angleStep = (0.62 * Math.pow(0.7, i));
            curAngle -= angleStep;
            curDist += hexSize * 1.3;
            hexSize *= 0.8;
        }
    }

    // --- Outer boundary ring: large hex frame, rotated 90 deg, traced by tiny dot-hexagons ---
    const outerRadius = 112;
    const dotsPerEdge = 10;
    this.arxLogosOuterDotMat = new THREE.MeshBasicMaterial({
        color: 0xff00ff,
        transparent: true,
        opacity: 0.75,
        blending: THREE.AdditiveBlending
    });
    const outerVerts = this.hexVertices(outerRadius, Math.PI / 2);
    for (let e = 0; e < 6; e++) {
        const v0 = outerVerts[e];
        const v1 = outerVerts[(e + 1) % 6];
        for (let d = 0; d < dotsPerEdge; d++) {
            const t = d / dotsPerEdge;
            const x = v0.x + (v1.x - v0.x) * t;
            const y = v0.y + (v1.y - v0.y) * t;
            const dot = this.buildHexFill(2.6, 0, this.arxLogosOuterDotMat, x, y);
            dot.userData = { baseScale: 1.0, phase: (e * dotsPerEdge + d) * 0.35 };
            this.arxLogosOuterDots.push(dot);
            this.arxLogosGroup.add(dot);
        }
    }

    // --- Two hex-frame rings beyond the dotted boundary, each tilted onto its own axis and
    // tumbling at its own speed (see animateArxLogos) -- unlike everything above, which is
    // flat in one plane, these read as depth wrapped around the whole structure.
    const shellConfigs = [
        { radius: 132, rotOffset: 0, tiltX: 0.5, tiltY: 0.15, color: 0x9d00ff, speed: { x: 0.006, y: 0.010, z: 0.0 } },
        { radius: 156, rotOffset: Math.PI / 6, tiltX: -0.35, tiltY: 0.4, color: 0xff00ff, speed: { x: -0.004, y: 0.008, z: 0.005 } }
    ];
    shellConfigs.forEach(cfg => {
        const mat = new THREE.LineBasicMaterial({ color: cfg.color, transparent: true, opacity: 0.5 });
        const ring = this.buildHexOutline(cfg.radius, cfg.rotOffset, mat);
        ring.rotation.x = cfg.tiltX;
        ring.rotation.y = cfg.tiltY;
        ring.userData = { speed: cfg.speed };
        this.arxLogosShellRingMats.push(mat);
        this.arxLogosShellRings.push(ring);
        this.arxLogosGroup.add(ring);
    });

    // --- Orbiting hex swarm: small nodes drifting freely in 3D (x/z from an orbit angle,
    // y from a fixed height plus a gentle bob) around the whole structure, unlike the arm
    // hexes which are locked to the flat spiral -- real depth, not just another flat layer.
    this.arxLogosSwarmGroup = new THREE.Group();
    this.arxLogosSwarmMat = new THREE.MeshBasicMaterial({
        color: 0xe024c3, transparent: true, opacity: 0.65, blending: THREE.AdditiveBlending
    });
    const swarmCount = 20;
    for (let i = 0; i < swarmCount; i++) {
        const radius = 60 + Math.random() * 70;
        const angle = Math.random() * Math.PI * 2;
        const heightOffset = (Math.random() - 0.5) * 70;
        const node = this.buildHexFill(2.2 + Math.random() * 1.6, Math.random() * Math.PI, this.arxLogosSwarmMat);
        node.position.set(Math.cos(angle) * radius, heightOffset, Math.sin(angle) * radius);
        node.userData = {
            radius, angle, heightOffset,
            speed: 0.15 + Math.random() * 0.25,
            bobPhase: Math.random() * Math.PI * 2
        };
        this.arxLogosSwarmNodes.push(node);
        this.arxLogosSwarmGroup.add(node);
    }
    this.arxLogosGroup.add(this.arxLogosSwarmGroup);

    this.scene.add(this.arxLogosGroup);
};
