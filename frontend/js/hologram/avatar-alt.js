// Avatar 6: A1ter_nul -- a security-focused netrunner ghost (Alt Cunningham), rendered as a
// faceted, semi-transparent "digital ghost" bust behind a rotating firewall/ICE perimeter.
// Colour-theme-neutral by design (see core.js) so the classic Cyberpunk 2077 palette
// (night-city: phosphor yellow + cyan + red) is one applyColorPalette() call away, same
// as every other avatar shape here.

HologramAvatar.prototype.buildAltAvatar = function() {
    this.altGroup = new THREE.Group();

    // --- Bust: a low-poly faceted silhouette (an unstable data-construct/engram, not a
    // solid body) -- stretched taller than wide so it reads as a head-and-shoulders ghost. ---
    const bustGeom = new THREE.IcosahedronGeometry(34, 1);

    // The oval head-and-shoulders proportions live on this sub-group so animateAlt's
    // breathing pulse can scale the group uniformly without fighting the baked-in squash.
    this.altBustGroup = new THREE.Group();
    this.altBustGroup.scale.set(0.82, 1.28, 0.82);
    this.altGroup.add(this.altBustGroup);

    this.altBustFillMat = new THREE.MeshBasicMaterial({
        color: 0xfcee0a, transparent: true, opacity: 0.08, blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.altBustMesh = new THREE.Mesh(bustGeom, this.altBustFillMat);
    this.altBustGroup.add(this.altBustMesh);

    this.altBustWireMat = new THREE.LineBasicMaterial({ color: 0xfcee0a, transparent: true, opacity: 0.2 });
    this.altBustInnerWire = new THREE.LineSegments(new THREE.WireframeGeometry(bustGeom), this.altBustWireMat);
    this.altBustGroup.add(this.altBustInnerWire);

    // Chromatic-split silhouette: the same facet edges traced twice, offset in opposite
    // directions and fixed to a cyan/red split regardless of color theme -- the classic
    // VHS/glitch "digital ghost" look, same fixed-accent pattern as The Nexus's red eyes.
    const bustEdges = new THREE.EdgesGeometry(bustGeom, 15);
    this.altGlitchCyanMat = new THREE.LineBasicMaterial({
        color: 0x00e5ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending
    });
    this.altGlitchMagentaMat = new THREE.LineBasicMaterial({
        color: 0xff003c, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending
    });
    this.altGlitchCyanOutline = new THREE.LineSegments(bustEdges, this.altGlitchCyanMat);
    this.altGlitchMagentaOutline = new THREE.LineSegments(bustEdges, this.altGlitchMagentaMat);
    this.altBustGroup.add(this.altGlitchCyanOutline);
    this.altBustGroup.add(this.altGlitchMagentaOutline);

    this.altBustPointsMat = new THREE.PointsMaterial({
        color: 0xff003c, size: 2.4, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending
    });
    this.altBustPoints = new THREE.Points(bustGeom, this.altBustPointsMat);
    this.altBustGroup.add(this.altBustPoints);

    // --- Firewall perimeter ring: a thin scanning halo, like an active ICE boundary. ---
    this.altFirewallRingMat = new THREE.MeshBasicMaterial({
        color: 0xfcee0a, transparent: true, opacity: 0.5, side: THREE.DoubleSide, blending: THREE.AdditiveBlending
    });
    this.altFirewallRing = new THREE.Mesh(new THREE.RingGeometry(76, 78.5, 64), this.altFirewallRingMat);
    this.altGroup.add(this.altFirewallRing);

    // --- Shield tiles: small hex "ICE nodes" ringing the bust, individually lit in
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
