/* Avatar: Operator -- a terminal prompt sitting in a clear centre, filling the whole
 * frame with code streaming outward past the viewer -- the "hyperspace starfield" trick:
 * points in fixed angular slots that only grow their radius over time read as motion
 * straight at the camera even though nothing ever moves in depth. Two ideas, borrowed
 * as pure technique rather than as any specific film's artwork: a "|>" caret that blinks
 * at the end of a line perpetually typing itself out, and a field of digit glyphs that
 * never stops sliding from the clear centre out past the edges of the screen.
 *
 * No obsidian core, no fixed hot accent: like A1 and White Rabbit, this reads as a
 * projection rather than a physical creature, so it's fully theme-tinted throughout.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

// Outer radius reaches past the frustum's edge even at the widest panel aspect the HUD
// allows (core.js widens the camera's fov up to maxFov=100 for a short, wide panel), so
// the field always fills the visible frame rather than floating in it as a small disc.
const OPERATOR_RING_INNER = 24;
const OPERATOR_RING_OUTER = 260;
const OPERATOR_RING_SPAN = OPERATOR_RING_OUTER - OPERATOR_RING_INNER;

// Draws a single character onto a transparent square canvas for use as a point-sprite
// map -- the same idiom as the built-in glow textures, just with a glyph baked in
// instead of a gradient.
function operatorGlyphTexture(char, size) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    ctx.font = `bold ${Math.floor(size * 0.8)}px "Courier New", Consolas, monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = '#ffffff';
    ctx.fillText(char, size / 2, size / 2 + size * 0.05);
    return new THREE.CanvasTexture(canvas);
}

// Same idea, but on a wider canvas for a short multi-character string like "|>" so the
// glyphs aren't squeezed into a square cell. Returns the texture plus its width/height
// ratio, so a Sprite using it can be scaled without distorting it.
function operatorTextTexture(text, size, aspect) {
    const width = Math.floor(size * aspect);
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    ctx.font = `bold ${Math.floor(size * 0.74)}px "Courier New", Consolas, monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillStyle = '#ffffff';
    ctx.fillText(text, width / 2, size / 2 + size * 0.04);
    return { texture: new THREE.CanvasTexture(canvas), aspect: width / size };
}

// One "stream" of glyphs in the vignette ring. Each glyph keeps a fixed angular slot
// (the frame itself is static) but its radius is a pure function of ctx.time -- it
// slides from OPERATOR_RING_INNER out to OPERATOR_RING_OUTER on a loop and fades in
// near the centre and out near the rim, then reappears at the centre. Nothing here is
// integrated frame to frame, so there's no per-frame state to drift or reset.
function buildOperatorStream(texture, count, color, pointSize, baseOpacity) {
    const positions = new Float32Array(count * 3);
    const colors = new Float32Array(count * 3);
    const state = new Array(count);
    for (let i = 0; i < count; i++) {
        const brightness = 0.75 + Math.random() * 0.55;
        state[i] = {
            angle: Math.random() * Math.PI * 2,
            speed: 40 + Math.random() * 65,
            offset: Math.random() * OPERATOR_RING_SPAN,
            z: -14 + Math.random() * 40,
            nextFlicker: Math.random() * 2,
            brightness,
        };
        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = brightness;
    }
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
    const material = new THREE.PointsMaterial({
        color, map: texture, size: pointSize, vertexColors: true,
        transparent: true, opacity: baseOpacity, depthWrite: false,
        blending: THREE.AdditiveBlending, sizeAttenuation: true,
    });
    const points = new THREE.Points(geometry, material);
    return { points, material, positions, colors, state };
}

// Rewrites one stream's positions/brightness for this frame. speedMult scales how fast
// glyphs slide outward; the angular slots themselves never move, which is what keeps
// the ring reading as a static frame rather than a spinning cylinder.
function updateOperatorStream(stream, ctx, speedMult) {
    const { positions, colors, state } = stream;
    for (let i = 0; i < state.length; i++) {
        const s = state[i];
        const flow = (ctx.time * s.speed * speedMult + s.offset) % OPERATOR_RING_SPAN;
        const radius = OPERATOR_RING_INNER + flow;
        const p = flow / OPERATOR_RING_SPAN;
        // Fade in just past the centre, hold, then fade out approaching the rim --
        // so glyphs emerge and dissolve rather than popping in and out.
        const fade = p < 0.12 ? p / 0.12 : (p > 0.72 ? Math.max(0, (1 - p) / 0.28) : 1);

        if (ctx.time > s.nextFlicker) {
            s.brightness = 0.65 + Math.random() * 0.85;
            s.nextFlicker = ctx.time + 0.15 + Math.random() * 1.1;
        }

        positions[i * 3] = Math.cos(s.angle) * radius;
        positions[i * 3 + 1] = Math.sin(s.angle) * radius;
        positions[i * 3 + 2] = s.z;
        const v = s.brightness * fade;
        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = v;
    }
    stream.points.geometry.attributes.position.needsUpdate = true;
    stream.points.geometry.attributes.color.needsUpdate = true;
}

HologramAvatar.registerAvatar({
    id: 'operator',
    label: 'Operator',

    build(api) {
        const group = new THREE.Group();

        const zeroTexture = operatorGlyphTexture('0', 64);
        const oneTexture = operatorGlyphTexture('1', 64);

        // The field: mostly "0"s, the densest and brightest stream, with a sparser
        // accent stream of "1"s threaded through it, a shade dimmer -- the two-glyph
        // mix a digital rain needs to read as code rather than static. Counts and sizes
        // are scaled up to keep the same density now that the field reaches the frame's
        // edges instead of stopping at a small central disc.
        const zeros = buildOperatorStream(zeroTexture, 3600, api.palette.hex, 9, 0.95);
        const ones = buildOperatorStream(oneTexture, 1200, api.palette.hex2, 7, 0.75);
        // The HUD panel this renders into is much wider than it is tall, so a circular
        // field reaches the top/bottom edges while leaving the corners bare. Stretching
        // the field horizontally (not the individual glyph sprites, just their layout)
        // turns it into an ellipse that actually reaches the left/right edges too.
        zeros.points.scale.set(1.9, 1, 1);
        ones.points.scale.set(1.9, 1, 1);
        group.add(zeros.points, ones.points);

        // The prompt: a row of ticks that perpetually "types" itself out left to right,
        // sitting in the ring's clear centre, with a "|>" caret that blinks at wherever
        // the line currently ends.
        const promptCount = 20;
        const promptSpacing = 2.3;
        const promptStartX = -((promptCount - 1) * promptSpacing) / 2;
        const promptZ = 30;
        const promptPositions = new Float32Array(promptCount * 3);
        const promptColors = new Float32Array(promptCount * 3);
        for (let i = 0; i < promptCount; i++) {
            promptPositions[i * 3] = promptStartX + i * promptSpacing;
            promptPositions[i * 3 + 1] = -2;
            promptPositions[i * 3 + 2] = promptZ;
        }
        const promptGeometry = new THREE.BufferGeometry();
        promptGeometry.setAttribute('position', new THREE.BufferAttribute(promptPositions, 3));
        promptGeometry.setAttribute('color', new THREE.BufferAttribute(promptColors, 3));
        const promptMaterial = new THREE.PointsMaterial({
            color: api.palette.hex3, map: zeroTexture, size: 5.5, vertexColors: true,
            transparent: true, opacity: 0.95, depthWrite: false,
            blending: THREE.AdditiveBlending, sizeAttenuation: true,
        });
        const promptPoints = new THREE.Points(promptGeometry, promptMaterial);
        group.add(promptPoints);

        // A soft halo right behind the caret, purely so it reads clearly against the
        // ring instead of getting lost in it.
        const cursorGlowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(48), color: api.palette.hex, transparent: true,
            opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const cursorGlow = new THREE.Sprite(cursorGlowMaterial);
        cursorGlow.scale.set(30, 30, 1);
        group.add(cursorGlow);

        // The caret itself: an actual "|>" glyph, not just a bar, rendered the same way
        // the ring's digits are -- a canvas glyph on a sprite -- but bigger and additive
        // so it stays legible over the ring behind it.
        const cursorTextureInfo = operatorTextTexture('|>', 72, 1.7);
        const cursorMaterial = new THREE.SpriteMaterial({
            map: cursorTextureInfo.texture, color: api.palette.hex, transparent: true,
            opacity: 1, depthWrite: false, blending: THREE.AdditiveBlending,
        });
        const cursor = new THREE.Sprite(cursorMaterial);
        const cursorWidth = 26;
        cursor.scale.set(cursorWidth, cursorWidth / cursorTextureInfo.aspect, 1);
        group.add(cursor);

        // Soft ambient glow behind everything, theme-tinted like the rest.
        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true,
            opacity: 0.14, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(220, 220, 1);
        glow.position.z = -40;
        group.add(glow);

        return {
            group, zeros, ones,
            promptPoints, promptMaterial, promptPositions, promptColors,
            promptCount, promptStartX, promptSpacing, promptZ,
            cursor, cursorMaterial, cursorGlow, cursorGlowMaterial,
            glow, glowMaterial,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isListening = ctx.state === 'LISTENING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // The ring never stops flowing outward -- like the tentacle avatars' constant
        // swimming, this is meant to always read as "live data," just calmer at rest.
        // The frame itself (each glyph's angular slot) never moves, so it stays a
        // static vignette rather than a spinning cylinder.
        let speedMult = 1;
        if (isThinking) speedMult = 2.4;
        else if (isListening) speedMult = 1.5;
        else if (isSpeaking) speedMult = 1.15 + ctx.audio * 0.9;
        speedMult += ctx.click * 1.4;

        updateOperatorStream(model.zeros, ctx, speedMult);
        updateOperatorStream(model.ones, ctx, speedMult);

        // The prompt line "types" itself out over a repeating cycle, faster while
        // thinking -- the terminal actively working rather than idling.
        const period = isThinking ? 1.8 : 3.6;
        const cyclePos = (ctx.time % period) / period;
        const leadIndex = cyclePos * (model.promptCount - 1);
        for (let i = 0; i < model.promptCount; i++) {
            const dist = leadIndex - i;
            let brightness;
            if (dist < 0) brightness = 0.03; // not typed yet
            else if (dist < 1.2) brightness = 1.0; // the leading edge, writing now
            else brightness = 0.3 + (isSpeaking ? ctx.audio * 0.3 : 0); // already printed
            model.promptColors[i * 3] = model.promptColors[i * 3 + 1] = model.promptColors[i * 3 + 2] = brightness;
        }
        model.promptPoints.geometry.attributes.color.needsUpdate = true;
        const cursorX = model.promptStartX + leadIndex * model.promptSpacing;
        model.cursor.position.set(cursorX, -2, model.promptZ + 4);
        model.cursorGlow.position.set(cursorX, -2, model.promptZ + 2);

        // The caret blinks while waiting, and holds a steady voice-reactive glow while
        // actually speaking -- it stops "waiting" once it has something to say.
        let cursorOpacity;
        if (isSpeaking) {
            cursorOpacity = 0.75 + ctx.audio * 0.4;
        } else {
            const blinkRate = isThinking ? 4.5 : 1.4;
            cursorOpacity = Math.floor(ctx.time * blinkRate) % 2 === 0 ? 1.0 : 0.3;
        }
        model.cursorMaterial.opacity = cursorOpacity;
        model.cursorGlowMaterial.opacity = 0.35 + cursorOpacity * 0.35;

        let glowIntensity;
        if (isSpeaking) glowIntensity = 0.12 + ctx.audio * 0.26;
        else if (isThinking) glowIntensity = 0.12 + Math.abs(Math.sin(ctx.time * 6)) * 0.1;
        else glowIntensity = 0.1 + ctx.click * 0.18;
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.zeros.material.color.setHex(palette.hex);
        model.ones.material.color.setHex(palette.hex2);
        model.promptMaterial.color.setHex(palette.hex3);
        model.cursorMaterial.color.setHex(palette.hex);
        model.cursorGlowMaterial.color.setHex(palette.hex);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
