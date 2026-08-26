// Avatar 5: A.R.X.LOGOS -- Central Hexagon with Six Clockwise Spiraling Hexagon Arms &
// Dotted Hex Frame.

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

    this.scene.add(this.arxLogosGroup);
};
