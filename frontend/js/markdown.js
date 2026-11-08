/* The little markdown renderer the companion's answers are drawn with.
 *
 * Pulled out of app.js because a second window now renders the same answers: the Answer
 * window (frontend/answer.html), which shows the settled reply and nothing else. Two copies
 * of this would be two renderers drifting apart -- one learning about tables, the other
 * about headings -- and the one thing both of them must keep doing identically is the first
 * step below.
 *
 * **It escapes its whole input before it decorates any of it.** Everything after the first
 * replace is working on text that can no longer contain a tag, which is what makes it safe
 * to hand the result to innerHTML. An answer is model output: it carries whatever was in the
 * question, whatever a tool read off the disk, and whatever a web page said. See
 * docs/SECURITY_AUDIT_S4.md for the two places where text from off the machine was being
 * rendered as HTML before, and scripts/check_csp.sh for the policy that makes the next slip
 * survivable.
 *
 * The classes it emits are Tailwind utilities from frontend/vendor/tailwind.css, which is a
 * static build of exactly the classes the scanner found -- so a class changed here has to be
 * followed by ./scripts/build_vendor_css.sh, or it silently means nothing. Any page that
 * renders with this has to link that stylesheet.
 */
(function (global) {
    'use strict';

    function render(text) {
        if (!text) return '';
        let escaped = text
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;");

        escaped = escaped.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, '<pre class="bg-black/50 p-2.5 rounded border border-cyan-500/30 my-2 overflow-x-auto text-xs font-mono text-cyan-200"><code>$2</code></pre>');
        escaped = escaped.replace(/`([^`]+)`/g, '<code class="bg-cyan-950/80 px-1 py-0.5 rounded text-cyan-300 font-mono text-xs border border-cyan-500/20">$1</code>');
        escaped = escaped.replace(/\*\*([^*]+)\*\*/g, '<strong class="text-cyan-300 font-bold">$1</strong>');
        escaped = escaped.replace(/^### (.*$)/gim, '<h3 class="text-md font-bold text-cyan-400 mt-2 mb-1">$1</h3>');
        escaped = escaped.replace(/^## (.*$)/gim, '<h2 class="text-lg font-bold text-cyan-300 mt-2 mb-1">$1</h2>');
        escaped = escaped.replace(/\n/g, '<br/>');
        return escaped;
    }

    global.Aether1Markdown = { render: render };
})(window);
