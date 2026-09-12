/* Avatar: Operator -- a terminal prompt sitting in a clear centre, filling the whole
 * frame with code streaming outward past the viewer -- the "hyperspace starfield" trick:
 * points in fixed angular slots that only grow their radius over time read as motion
 * straight at the camera even though nothing ever moves in depth. Two ideas, borrowed
 * as pure technique rather than as any specific film's artwork: a "|>" caret that blinks
 * at the end of a line perpetually typing itself out, and a field of digit glyphs that
 * never stops sliding from the clear centre out past the edges of the screen.
 *
 * The field sits inside a deep box of depth (see OPERATOR_Z_NEAR/FAR below) rather than
 * a thin shell close to one plane, so the parallax that used to only read clearly when
 * the hologram was dragged to an angle -- near glyphs large and fast against far ones
 * small and dim -- now reads head-on too. And the box is locked to the camera, not to
 * the hologram's own drag-to-spin: see the rotation counter at the top of animate()
 * below. A tunnel that spun with every drag stopped reading as a tunnel at all.
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

// The depth box each glyph's fixed z sits somewhere inside -- near enough to the camera
// (which sits at world z=240, see core.js) that the closest glyphs read as passing right
// by the viewer, far enough back that the furthest ones sit well behind the prompt and
// dim into the distance. Wide on purpose: this range, not view angle, is what should now
// carry the depth-of-field look.
const OPERATOR_Z_NEAR = 95;
const OPERATOR_Z_FAR = -240;
const OPERATOR_Z_SPAN = OPERATOR_Z_NEAR - OPERATOR_Z_FAR;

// A short vertical dash on a transparent square canvas, soft-edged top and bottom, for
// use as a point-sprite map. A field of round glyph dots reads as scattered static; the
// same field built from vertical dashes packed close together reads as a woven wall of
// code instead -- the texture a code-rain effect actually needs, without reproducing any
// specific character, font or composition.
function operatorTickTexture(size, widthFrac, heightFrac) {
    const canvas = document.createElement('canvas');
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext('2d');
    const w = size * widthFrac;
    const h = size * heightFrac;
    const x = (size - w) / 2;
    const y = (size - h) / 2;
    const r = w / 2;
    const gradient = ctx.createLinearGradient(0, y, 0, y + h);
    gradient.addColorStop(0, 'rgba(255,255,255,0)');
    gradient.addColorStop(0.18, 'rgba(255,255,255,1)');
    gradient.addColorStop(0.82, 'rgba(255,255,255,1)');
    gradient.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = gradient;
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
    ctx.fill();
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
            z: OPERATOR_Z_FAR + Math.random() * OPERATOR_Z_SPAN,
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
// the ring reading as a static frame rather than a spinning cylinder. zoomPulse is a
// bounded radial offset (not a rate) so it can oscillate quickly without the runaway
// growth a time-varying multiplier would cause inside a ctx.time-based phase formula;
// blink is a bounded brightness multiplier, safe to vary freely since it isn't part of
// any accumulated position.
function updateOperatorStream(stream, ctx, speedMult, zoomPulse, blink) {
    const { positions, colors, state } = stream;
    for (let i = 0; i < state.length; i++) {
        const s = state[i];
        const flow = (ctx.time * s.speed * speedMult + s.offset + zoomPulse) % OPERATOR_RING_SPAN;
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
        const v = s.brightness * fade * blink;
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

        const primaryTick = operatorTickTexture(64, 0.34, 0.92);
        const accentTick = operatorTickTexture(64, 0.22, 0.6);

        // The field: a dense primary layer of long, thick dashes, with a sparser accent
        // layer of shorter, thinner ones threaded through it, a shade dimmer -- packed
        // close enough (see counts/size below) that neighbouring dashes overlap into a
        // continuous woven texture rather than reading as scattered individual dots.
        const zeros = buildOperatorStream(primaryTick, 5200, api.palette.hex, 13, 0.95);
        const ones = buildOperatorStream(accentTick, 2000, api.palette.hex2, 8, 0.75);
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
            color: api.palette.hex3, map: accentTick, size: 5.5, vertexColors: true,
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
        // The tunnel is a fixed box in front of the camera, not something that spins when
        // the hologram is dragged -- countering the scene's own drag-yaw here keeps it
        // facing the viewer no matter how far the view has been turned. See core.js's
        // pointermove handler for where ctx.viewRotationY comes from.
        model.group.rotation.y = -ctx.viewRotationY;

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

        // A rhythmic "zoom" punch -- a bounded outward radius offset (not a rate, so it
        // can't destabilise the ctx.time-based flow formula), sharply peaked so it reads
        // as a periodic push toward the viewer rather than a smooth wobble. Livelier
        // states get a slightly bigger punch. Layered with a fast, sharply-peaked
        // "blink" brightness strobe across the whole field, on top of each glyph's own
        // independent flicker -- together these are the title-sequence's own zoom/blink
        // rhythm, borrowed as a motion quality rather than any of its actual imagery.
        const zoomPulse = Math.pow(Math.max(0, Math.sin(ctx.time * 0.55)), 5) * 34 * (0.6 + Math.min(speedMult, 3) * 0.25);
        const blink = 0.72 + Math.pow(Math.max(0, Math.sin(ctx.time * 3.1 + 0.7)), 14) * 0.55;

        updateOperatorStream(model.zeros, ctx, speedMult, zoomPulse, blink);
        updateOperatorStream(model.ones, ctx, speedMult, zoomPulse, blink);

        // The prompt line sits still -- already printed, not typing itself out -- with
        // a CRT-style flicker instead of a sweep: each tick breathes a little brighter
        // then a little dimmer, each on its own slightly offset phase so the row shimmers
        // rather than pulsing in lockstep. Thinking breathes faster, same as before.
        const flickerSpeed = isThinking ? 3.2 : 1.4;
        for (let i = 0; i < model.promptCount; i++) {
            const phase = i * 0.6;
            const brightness = 0.32 + (Math.sin(ctx.time * flickerSpeed + phase) * 0.5 + 0.5) * 0.22;
            model.promptColors[i * 3] = model.promptColors[i * 3 + 1] = model.promptColors[i * 3 + 2] = brightness;
        }
        model.promptPoints.geometry.attributes.color.needsUpdate = true;
        // The caret's own fixed resting spot, at the end of the (static) line.
        const cursorX = model.promptStartX + (model.promptCount - 1) * model.promptSpacing;
        model.cursor.position.set(cursorX, -2, model.promptZ + 4);
        model.cursorGlow.position.set(cursorX, -2, model.promptZ + 2);

        // The caret breathes with the same CRT flicker while waiting -- slightly
        // brighter, then dimmer, not a hard on/off blink -- and holds a steady
        // voice-reactive glow while actually speaking.
        let cursorOpacity;
        if (isSpeaking) {
            cursorOpacity = 0.75 + ctx.audio * 0.4;
        } else {
            cursorOpacity = 0.72 + (Math.sin(ctx.time * flickerSpeed) * 0.5 + 0.5) * 0.28;
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
