// Avatar 6: A1ter_nul -- modelled on Cyberpunk 2077's Black Wall / relic aesthetic: a
// horizontal row of irregular dark-glass shards, each standing edge-on as a thin vertical
// bar, tapering shortest at the two ends of the row and tallest in the middle (so a vector
// from the shortest shard to the shortest shard runs horizontal) -- not a solid body, gaps
// visible between shards -- each a fixed obsidian-glass pane with a theme-tinted glowing
// edge. While speaking, each shard's edge brightness is driven by its own audio frequency
// bin -- the row reads as a classic vertical-bar equalizer built out of broken relic glass.
// Colour-theme-neutral by design (see core.js) so any palette is one applyColorPalette()
// call away, same as every other avatar shape here.

HologramAvatar.prototype.buildAltAvatar = function() {
    this.altGroup = new THREE.Group();

    // --- The Black Wall: a row of jittered, irregular glass-pane shards with gaps
    // between them, tapering shortest at both ends like the in-game relic chip's lens
    // profile, just oriented as a horizontal spectrum-analyzer row instead of a column. ---
    this.altShardGroup = new THREE.Group();
    const shardCount = 9;
    const totalSpan = 84; // the row's span along the stacking axis (now horizontal, X)
    const slotSpan = totalSpan / shardCount;
    const gap = 2.6;
    const shardThickness = slotSpan - gap; // the thin dimension, collapsed into depth below
    const maxLength = 54; // the tapered, visible (vertical) bar length
    const minLength = 24;

    for (let i = 0; i < shardCount; i++) {
        const t = shardCount > 1 ? i / (shardCount - 1) : 0.5;
        const taper = 1 - Math.pow(Math.abs(t - 0.5) * 2, 1.6); // lens/oval profile, 0..1
        const length = minLength + (maxLength - minLength) * taper;
        const x = -totalSpan / 2 + i * slotSpan + slotSpan / 2;

        // Irregular quad -- jittered corners so each pane reads as a broken glass shard
        // rather than a tidy rectangle in a row.
        const hw = length / 2, hh = shardThickness / 2;
        const jitter = () => (Math.random() - 0.5) * 3.5;
        const pts = [
            new THREE.Vector3(-hw + jitter(), -hh + jitter(), 0),
            new THREE.Vector3(hw + jitter(), -hh + jitter(), 0),
            new THREE.Vector3(hw + jitter(), hh + jitter(), 0),
            new THREE.Vector3(-hw + jitter(), hh + jitter(), 0)
        ];

        const fillGeom = new THREE.BufferGeometry();
        const positions = [
            pts[0].x, pts[0].y, 0, pts[1].x, pts[1].y, 0, pts[2].x, pts[2].y, 0,
            pts[0].x, pts[0].y, 0, pts[2].x, pts[2].y, 0, pts[3].x, pts[3].y, 0
        ];
        fillGeom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));

        // Obsidian glass fill -- fixed, not theme-tinted (real glass doesn't change hue
        // with the UI theme), same fixed-material pattern as R.E.D. 9000's core sphere.
        const fillMat = new THREE.MeshBasicMaterial({
            color: 0x0a0a0c, transparent: true, opacity: 0.55, side: THREE.DoubleSide
        });
        const fillMesh = new THREE.Mesh(fillGeom, fillMat);

        // The glowing edge -- theme-tinted, and the one thing animateAlt drives per frame
        // to turn the stack into an equalizer while speaking.
        const outlineMat = new THREE.LineBasicMaterial({
            color: 0xfcee0a, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
        });
        const outline = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(pts), outlineMat);

        const shardPivot = new THREE.Group();
        // Each shard's quad is built flat in local XY (see pts above), its long axis
        // (hw) along local X and its thin axis (hh) along local Y. Rotating 90 deg about
        // Z swings the long axis onto world Y (a vertical bar); rotating a further 90 deg
        // about Y then tips the now-thin local axis into depth (Z), so the bar is seen
        // edge-on from the front instead of as a face-on rectangle. Net effect: a row of
        // thin vertical glass bars, not a shelf of upright picture frames. The small extra
        // jitter on top keeps them askew rather than a tidy, perfectly level row.
        shardPivot.rotation.z = Math.PI / 2 + (Math.random() - 0.5) * 0.09;
        shardPivot.rotation.y = Math.PI / 2 + (Math.random() - 0.5) * 0.16;
        shardPivot.position.set(x, (Math.random() - 0.5) * 4, (Math.random() - 0.5) * 3);
        shardPivot.add(fillMesh);
        shardPivot.add(outline);
        this.altShardGroup.add(shardPivot);

        this.altShards.push({
            fillMat, outlineMat,
            baseOpacity: 0.3 + Math.random() * 0.15,
            phase: i * 0.9,
            binIndex: i % 16
        });
    }
    this.altGroup.add(this.altShardGroup);

    // --- Firewall perimeter: a static boundary, not a scanning halo -- many concentric
    // true circles (no elliptical stretch), packed close enough together to read as a
    // continuous dissipating field rather than a handful of discrete hoops, the
    // outermost reaching out toward the window's own edges. Each ring is fainter than
    // the one inside it, so the boundary reads as fading outward from a bright core. ---
    const firewallRingCount = 28;
    const firewallInnerRadius = 34;
    const firewallOuterRadius = 260;
    for (let i = 0; i < firewallRingCount; i++) {
        const t = i / (firewallRingCount - 1); // 0 (innermost) .. 1 (outermost)
        const radius = firewallInnerRadius + (firewallOuterRadius - firewallInnerRadius) * t;
        const fade = Math.pow(1 - t, 1.6); // brightest near the core, fading toward the rim
        const mat = new THREE.MeshBasicMaterial({
            color: 0xfcee0a, transparent: true, opacity: 0.5, side: THREE.DoubleSide, blending: THREE.AdditiveBlending
        });
        const mesh = new THREE.Mesh(new THREE.RingGeometry(radius, radius + radius * 0.012, 96), mat);
        this.altGroup.add(mesh);
        this.altFirewallRings.push({ mesh, mat, fade });
    }

    // --- Shield tiles: small hex "ICE nodes" ringing the stack, individually lit in
    // sequence like a security scanner sweeping the perimeter. ---
    this.altShieldGroup = new THREE.Group();
    this.altShieldFillMat = new THREE.MeshBasicMaterial({
        color: 0xfcee0a, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending
    });
    this.altShieldOutlineMat = new THREE.LineBasicMaterial({ color: 0xff003c, transparent: true, opacity: 0.85 });

    const tileCount = 9;
    const tileRadius = 56;
    for (let i = 0; i < tileCount; i++) {
        const angle = (i / tileCount) * Math.PI * 2;
        const x = Math.cos(angle) * tileRadius;
        const y = Math.sin(angle) * tileRadius;
        const tileGroup = new THREE.Group();
        const fill = this.buildHexFill(5.5, Math.PI / 2, this.altShieldFillMat, x, y);
        const outline = this.buildHexOutline(5.5, Math.PI / 2, this.altShieldOutlineMat, x, y);
        tileGroup.add(fill);
        tileGroup.add(outline);
        tileGroup.userData = { fill, outline, baseAngle: angle, phase: i * 0.7 };
        this.altShieldTiles.push(tileGroup);
        this.altShieldGroup.add(tileGroup);
    }
    this.altGroup.add(this.altShieldGroup);

    this.scene.add(this.altGroup);
};
