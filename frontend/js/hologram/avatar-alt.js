// Avatar 6: A1ter_nul -- modelled on Cyberpunk 2077's Black Wall / relic aesthetic: a
// column of irregular dark-glass shards lying flat (horizontal panes, not standing plates
// facing the camera), stacked on top of each other with visible gaps between them -- not a
// solid body -- each shard a fixed obsidian-glass pane with a theme-tinted glowing edge.
// While speaking, each shard's edge brightness is driven by its own audio frequency bin --
// the stack reads as a vertical equalizer built out of broken relic glass. Colour-theme-
// neutral by design (see core.js) so any palette is one applyColorPalette() call away,
// same as every other avatar shape here.

HologramAvatar.prototype.buildAltAvatar = function() {
    this.altGroup = new THREE.Group();

    // --- The Black Wall: a stack of jittered, irregular glass-pane shards with gaps
    // between them, tapering top and bottom like the in-game relic chip's lens profile. ---
    this.altShardGroup = new THREE.Group();
    const shardCount = 9;
    const totalHeight = 84;
    const slotHeight = totalHeight / shardCount;
    const gap = 2.6;
    const shardHeight = slotHeight - gap;
    const maxWidth = 54;
    const minWidth = 24;

    for (let i = 0; i < shardCount; i++) {
        const t = shardCount > 1 ? i / (shardCount - 1) : 0.5;
        const taper = 1 - Math.pow(Math.abs(t - 0.5) * 2, 1.6); // lens/oval profile, 0..1
        const width = minWidth + (maxWidth - minWidth) * taper;
        const y = -totalHeight / 2 + i * slotHeight + slotHeight / 2;

        // Irregular quad -- jittered corners so each pane reads as a broken glass shard
        // rather than a tidy rectangle in a tower.
        const hw = width / 2, hh = shardHeight / 2;
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
        // Each shard's quad is built flat in local XY (see pts above); rotating 90 deg
        // about X tips it from "standing plate facing the camera" to "lying flat, seen
        // edge-on from the front" -- so the stack reads as horizontal panes resting on
        // top of each other, like real stacked glass sheets, rather than a shelf of
        // upright picture frames. The small extra X/Z jitter on top keeps them askew
        // rather than a tidy, perfectly level tower.
        shardPivot.rotation.x = Math.PI / 2 + (Math.random() - 0.5) * 0.16;
        shardPivot.rotation.z = (Math.random() - 0.5) * 0.09;
        shardPivot.position.set((Math.random() - 0.5) * 4, y, (Math.random() - 0.5) * 3);
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

    // --- Firewall perimeter ring: a thin scanning halo, like an active ICE boundary. ---
    this.altFirewallRingMat = new THREE.MeshBasicMaterial({
        color: 0xfcee0a, transparent: true, opacity: 0.5, side: THREE.DoubleSide, blending: THREE.AdditiveBlending
    });
    this.altFirewallRing = new THREE.Mesh(new THREE.RingGeometry(76, 78.5, 64), this.altFirewallRingMat);
    this.altGroup.add(this.altFirewallRing);

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
