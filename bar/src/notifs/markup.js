/**
 * A notification body as a list of styled runs of text.
 *
 * The body is written by whoever sent the notification, and that includes any
 * website the browser lets send them. This page can run commands on the
 * machine, so nothing from a body is ever handed to the HTML parser: it is
 * read here into plain runs, and the template draws those with ordinary text
 * interpolation. The worst a hostile body can do is look strange.
 *
 * The spec allows a small subset of markup. `<b>`, `<i>`, `<u>` and `<a>` are
 * honoured, `<br>` is a line break, and every other tag is dropped with its
 * text kept.
 */

const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

const LINKS = /^(https?:\/\/|mailto:)/i;

function decode(text) {
    return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (whole, name) => {
        if (name[0] !== "#") return ENTITIES[name.toLowerCase()] ?? whole;
        const code = name[1].toLowerCase() === "x" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
        return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : whole;
    });
}

function hrefOf(attributes) {
    const match = /href\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(attributes);
    const href = decode(match?.[1] ?? match?.[2] ?? match?.[3] ?? "").trim();
    return LINKS.test(href) ? href : "";
}

export function runs(body) {
    const out = [];
    const style = { bold: 0, italic: 0, underline: 0, href: [] };
    const flags = { b: "bold", strong: "bold", i: "italic", em: "italic", u: "underline" };

    const push = text => {
        if (!text) return;
        out.push({
            text: decode(text),
            bold: style.bold > 0,
            italic: style.italic > 0,
            underline: style.underline > 0,
            href: style.href.at(-1) ?? ""
        });
    };

    const tag = /<(\/?)([a-z][a-z0-9]*)\b([^>]*)>/gi;
    let last = 0;
    for (let match = tag.exec(body); match; match = tag.exec(body)) {
        push(body.slice(last, match.index));
        last = tag.lastIndex;

        const [, closing, rawName, attributes] = match;
        const name = rawName.toLowerCase();
        if (name === "br") push("\n");
        else if (name === "a") closing ? style.href.pop() : style.href.push(hrefOf(attributes));
        else if (flags[name]) style[flags[name]] = Math.max(0, style[flags[name]] + (closing ? -1 : 1));
    }
    push(body.slice(last));
    return out;
}

/** The same body as one line of plain text, for the collapsed preview. */
export function plain(body) {
    return runs(body)
        .map(run => run.text)
        .join("")
        .replace(/\s+/g, " ")
        .trim();
}
