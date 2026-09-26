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

// A two-color radial gradient on a transparent canvas (used by R.E.D. 9000's lens glow) --
// baked with real CSS colors rather than left white, since this represents a fixed physical
// glow (like an obsidian core) rather than a themeable UI accent, so material.color tinting
// doesn't apply here.
HologramAvatar.prototype.createRadialGlowTexture = function(size, centerColor, edgeColor) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    const gradient = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    gradient.addColorStop(0, centerColor);
    gradient.addColorStop(0.55, edgeColor);
    gradient.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, size, size);
    return new THREE.CanvasTexture(canvas);
};

// A thin stroked ring on a transparent canvas (used by The Nexus's eye-lens outlines) --
// rendered white so material.color can tint it. A sprite rather than flat geometry so the
// ring always faces the camera regardless of how the parent head is rotated.
HologramAvatar.prototype.createRingSpriteTexture = function(size = 64, thickness = 0.14) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    const r = size / 2 - 2;
    ctx.strokeStyle = 'rgba(255,255,255,1)';
    ctx.lineWidth = size * thickness;
    ctx.beginPath();
    ctx.arc(size / 2, size / 2, r, 0, Math.PI * 2);
    ctx.stroke();
    return new THREE.CanvasTexture(canvas);
};

// A soft radial-fade band on a transparent canvas, meant for a flat THREE.RingGeometry
// annulus (used by A.R.X.LIMES's accretion disc) rather than a solid-shaded tube -- an
// unlit MeshBasicMaterial on a torus reads as flat painted plastic, not light. A ring's
// UV.v runs radially (inner edge to outer edge) and UV.u runs around the angle, so a
// gradient that only varies down the canvas's height produces a soft glowing band with a
// bright core and feathered inner/outer edges, tiling seamlessly around the angle since
// nothing varies horizontally. Rendered white so material.color/opacity still tint it.
HologramAvatar.prototype.createRadialBandTexture = function(size = 128) {
    const canvas = document.createElement('canvas');
    canvas.width = 8;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    const gradient = ctx.createLinearGradient(0, 0, 0, size);
    gradient.addColorStop(0, 'rgba(255,255,255,0)');
    gradient.addColorStop(0.35, 'rgba(255,255,255,0.9)');
    gradient.addColorStop(0.5, 'rgba(255,255,255,1)');
    gradient.addColorStop(0.65, 'rgba(255,255,255,0.9)');
    gradient.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, 8, size);
    return new THREE.CanvasTexture(canvas);
};

// A faint tiled grid on a transparent canvas (used by The Nexus's CRT-radar backdrop) --
// rendered white so material.color can tint it per the active color theme.
HologramAvatar.prototype.createGridSpriteTexture = function(size = 256, divisions = 10) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    ctx.strokeStyle = 'rgba(255,255,255,0.5)';
    ctx.lineWidth = 1;
    const step = size / divisions;
    for (let i = 0; i <= divisions; i++) {
        const p = i * step;
        ctx.beginPath();
        ctx.moveTo(p, 0);
        ctx.lineTo(p, size);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(0, p);
        ctx.lineTo(size, p);
        ctx.stroke();
    }
    // A brighter border ring reads as a radar bezel rather than a plain grid tile.
    ctx.strokeStyle = 'rgba(255,255,255,0.9)';
    ctx.lineWidth = 2;
    ctx.strokeRect(1, 1, size - 2, size - 2);
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

// A real 3D hex prism (front + back hex caps and 6 rectangular side walls), not a
// single-sided flat pane -- used for A.R.X.LOGOS's central core-eye, which needs to read
// as a solid gem with actual depth and stay visible (not vanish) when viewed from behind,
// e.g. after the scene is drag-rotated around. CylinderGeometry's axis defaults to Y,
// with its own end caps at y = +-thickness/2; rotating -90deg about X and then shifting
// back by half the thickness lays it flat in the XY plane with its front cap at z=0 (so
// anything already positioned in front of the old flat fill, like the pupil/catchlight,
// stays exactly where it was) and its depth receding into -z, matching hexVertices'
// (radius, rotationOffset) vertex layout on the front cap so the two stay interchangeable.
HologramAvatar.prototype.buildHexPrism = function(radius, rotationOffset, material, thickness, cx = 0, cy = 0) {
    const geom = new THREE.CylinderGeometry(radius, radius, thickness, 6, 1, false, rotationOffset);
    geom.rotateX(-Math.PI / 2);
    geom.translate(0, 0, -thickness / 2);
    const mesh = new THREE.Mesh(geom, material);
    mesh.position.set(cx, cy, 0);
    return mesh;
};

// Builds one flat-ish irregular polygon "shard" (used by A.R.X.LIMES). points2D form a
// convex loop in local space; the loop is bulged along Z (root at y=0 stays flat, the
// outward tip recedes) so a cluster of shards reads as facets of one convex dome. Returns
// a Group containing both the translucent fill and a bright edge outline.
HologramAvatar.prototype.buildPolygonShard = function(points2D, bulge, fillMat, outlineMat, thickness = 0) {
    // Shapes are drawn with their near (hub-facing) edge at y=0 and extend toward +y.
    // Bulge by y alone (not radial distance) so the whole near edge sits flush at z=0
    // and only the far edge angles backward -- not the near edge's side corners too.
    const maxY = Math.max(...points2D.map(p => p.y), 1);
    const frontVerts = points2D.map(p => new THREE.Vector3(p.x, p.y, -bulge * (p.y / maxY)));

    const positions = [];
    const pushTri = (a, b, c) => positions.push(a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z);
    for (let i = 1; i < frontVerts.length - 1; i++) {
        pushTri(frontVerts[0], frontVerts[i], frontVerts[i + 1]);
    }

    // With a thickness, the shard becomes a real solid slab -- a back face parallel to
    // the front (same per-vertex bulge, just pushed further from the camera) plus side
    // walls closing the loop between them -- rather than a single zero-depth surface, so
    // it actually shows an edge/depth once the plate turns away from face-on.
    let backVerts = null;
    if (thickness > 0) {
        backVerts = frontVerts.map((v) => new THREE.Vector3(v.x, v.y, v.z - thickness));
        for (let i = 1; i < backVerts.length - 1; i++) {
            pushTri(backVerts[0], backVerts[i + 1], backVerts[i]);
        }
        for (let i = 0; i < frontVerts.length; i++) {
            const j = (i + 1) % frontVerts.length;
            pushTri(frontVerts[i], frontVerts[j], backVerts[j]);
            pushTri(frontVerts[i], backVerts[j], backVerts[i]);
        }
    }

    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    geom.computeVertexNormals();
    const fillMesh = new THREE.Mesh(geom, fillMat);

    const outlineGeom = new THREE.BufferGeometry().setFromPoints(frontVerts);
    const outline = new THREE.LineLoop(outlineGeom, outlineMat);

    const group = new THREE.Group();
    group.add(fillMesh);
    group.add(outline);
    if (backVerts) {
        const backOutlineGeom = new THREE.BufferGeometry().setFromPoints(backVerts);
        group.add(new THREE.LineLoop(backOutlineGeom, outlineMat));
    }
    return group;
};

// A faceted "rounded" cube: square-shouldered enough to read as a cube, with its edges and
// corners cut back into facets instead of meeting at a point.
//
// Built by spherifying a subdivided cube rather than by chamfering one. Each face is a
// grid of `segments` x `segments` quads, and every grid point is pushed part of the way
// (`round`, 0 = cube, 1 = sphere) toward the sphere it sits on; a face's centre is already
// on that sphere and does not move, so the corners come in while the flat faces stay put.
// A chamfer would have wanted a convex hull, and three.js's own RoundedBoxGeometry is an
// addon this vendored build does not carry.
//
// Returns the fill geometry and, separately, the facet grid as line segments. The grid is
// built here rather than left to EdgesGeometry because the facets near a face's centre
// meet at a couple of degrees and the ones at a corner at twenty: no single threshold
// draws them all, and any threshold low enough to try also draws the diagonal each quad is
// split along, which is not a facet edge.
//
// `radius` is the distance to the furthest (corner) point, matching IcosahedronGeometry's
// radius, so a rounded cube can be swapped in for a geodesic at the same number.
HologramAvatar.prototype.buildRoundedCube = function(radius, segments = 3, round = 0.45) {
    // Each face as an origin corner and the two edge vectors spanning it, in a cube
    // running -1..1 on every axis.
    const faces = [
        { o: [-1, -1,  1], du: [ 2, 0,  0], dv: [0,  2,  0] }, // +Z
        { o: [ 1, -1, -1], du: [-2, 0,  0], dv: [0,  2,  0] }, // -Z
        { o: [ 1, -1,  1], du: [ 0, 0, -2], dv: [0,  2,  0] }, // +X
        { o: [-1, -1, -1], du: [ 0, 0,  2], dv: [0,  2,  0] }, // -X
        { o: [-1,  1,  1], du: [ 2, 0,  0], dv: [0,  0, -2] }, // +Y
        { o: [-1, -1, -1], du: [ 2, 0,  0], dv: [0,  0,  2] }, // -Y
    ];

    // The corner is the furthest point and the one that moves, so the scale that lands it
    // on `radius` is only known after the lerp.
    const corner = new THREE.Vector3(1, 1, 1);
    const cornerLen = corner.clone().lerp(corner.clone().normalize(), round).length();
    const scale = radius / cornerLen;

    const point = (face, u, v) => {
        const p = new THREE.Vector3(
            face.o[0] + face.du[0] * u + face.dv[0] * v,
            face.o[1] + face.du[1] * u + face.dv[1] * v,
            face.o[2] + face.du[2] * u + face.dv[2] * v
        );
        p.lerp(p.clone().normalize(), round).multiplyScalar(scale);
        return p;
    };

    const tris = [];
    const lines = [];
    const push = (arr, ...points) => points.forEach(p => arr.push(p.x, p.y, p.z));

    faces.forEach((face) => {
        const grid = [];
        for (let i = 0; i <= segments; i++) {
            grid.push([]);
            for (let j = 0; j <= segments; j++) {
                grid[i].push(point(face, i / segments, j / segments));
            }
        }
        for (let i = 0; i < segments; i++) {
            for (let j = 0; j < segments; j++) {
                const a = grid[i][j], b = grid[i + 1][j], c = grid[i + 1][j + 1], d = grid[i][j + 1];
                push(tris, a, b, c);
                push(tris, a, c, d);
                // Two sides per quad, plus the far sides of the last row and column, so
                // every facet boundary is drawn exactly once within this face.
                push(lines, a, b);
                push(lines, a, d);
                if (i === segments - 1) push(lines, b, c);
                if (j === segments - 1) push(lines, d, c);
            }
        }
    });

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(tris, 3));
    geometry.computeVertexNormals();
    const edges = new THREE.BufferGeometry();
    edges.setAttribute('position', new THREE.Float32BufferAttribute(lines, 3));
    return { geometry, edges };
};
