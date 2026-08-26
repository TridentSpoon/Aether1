// Shared geometry/texture helpers used by the avatar-*.js builders. No per-avatar state --
// pure construction helpers hung off HologramAvatar.prototype.

// Shared soft glow sprite texture (theme-neutral white so vertex colors tint it cleanly)
HologramAvatar.prototype.createGlowSpriteTexture = function(size = 32) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    const gradient = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    gradient.addColorStop(0, 'rgba(255,255,255,1)');
    gradient.addColorStop(0.3, 'rgba(255,255,255,0.8)');
    gradient.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, size, size);
    return new THREE.CanvasTexture(canvas);
};

// A single glowing glyph on a transparent canvas (used by The Nexus's letter rain).
// Rendered white so material.color can tint it per the active color theme.
HologramAvatar.prototype.createLetterSpriteTexture = function(char, size = 64) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    ctx.font = `bold ${Math.round(size * 0.7)}px "Share Tech Mono", monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.shadowColor = 'rgba(255,255,255,0.9)';
    ctx.shadowBlur = size * 0.15;
    ctx.fillStyle = '#ffffff';
    ctx.fillText(char, size / 2, size / 2 + size * 0.03);
    return new THREE.CanvasTexture(canvas);
};

// Regular hexagon helpers (used by A.R.X.LOGOS). rotationOffset=0 gives flat top/bottom,
// pointy left/right; rotationOffset=Math.PI/2 gives pointy top/bottom, flat left/right.
HologramAvatar.prototype.hexVertices = function(radius, rotationOffset, cx = 0, cy = 0) {
    const pts = [];
    for (let i = 0; i < 6; i++) {
        const angle = rotationOffset + i * (Math.PI / 3);
        pts.push(new THREE.Vector3(cx + Math.cos(angle) * radius, cy + Math.sin(angle) * radius, 0));
    }
    return pts;
};

HologramAvatar.prototype.buildHexOutline = function(radius, rotationOffset, material, cx = 0, cy = 0) {
    const geom = new THREE.BufferGeometry().setFromPoints(this.hexVertices(radius, rotationOffset, cx, cy));
    return new THREE.LineLoop(geom, material);
};

HologramAvatar.prototype.buildHexFill = function(radius, rotationOffset, material, cx = 0, cy = 0) {
    const geom = new THREE.CircleGeometry(radius, 6, rotationOffset);
    const mesh = new THREE.Mesh(geom, material);
    mesh.position.set(cx, cy, 0);
    return mesh;
};

// Builds one flat-ish irregular polygon "shard" (used by A.R.X.LIMES). points2D form a
// convex loop in local space; the loop is bulged along Z (root at y=0 stays flat, the
// outward tip recedes) so a cluster of shards reads as facets of one convex dome. Returns
// a Group containing both the translucent fill and a bright edge outline.
HologramAvatar.prototype.buildPolygonShard = function(points2D, bulge, fillMat, outlineMat) {
    // Shapes are drawn with their near (hub-facing) edge at y=0 and extend toward +y.
    // Bulge by y alone (not radial distance) so the whole near edge sits flush at z=0
    // and only the far edge angles backward -- not the near edge's side corners too.
    const maxY = Math.max(...points2D.map(p => p.y), 1);
    const verts = points2D.map(p => {
        return new THREE.Vector3(p.x, p.y, -bulge * (p.y / maxY));
    });

    const positions = [];
    for (let i = 1; i < verts.length - 1; i++) {
        positions.push(verts[0].x, verts[0].y, verts[0].z);
        positions.push(verts[i].x, verts[i].y, verts[i].z);
        positions.push(verts[i + 1].x, verts[i + 1].y, verts[i + 1].z);
    }
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    geom.computeVertexNormals();
    const fillMesh = new THREE.Mesh(geom, fillMat);

    const outlineGeom = new THREE.BufferGeometry().setFromPoints(verts);
    const outline = new THREE.LineLoop(outlineGeom, outlineMat);

    const group = new THREE.Group();
    group.add(fillMesh);
    group.add(outline);
    return group;
};
