import assert from "node:assert/strict";
import { plain, runs } from "./markup.js";

// Styling is carried, tags are not.
let r = runs("Hello <b>bold <i>both</i></b> plain");
assert.deepEqual(r.map(x => [x.text, x.bold, x.italic]), [
    ["Hello ", false, false], ["bold ", true, false], ["both", true, true], [" plain", false, false]
]);

// Only real links become links; a javascript: one is text and nothing more.
r = runs('<a href="https://example.com">ok</a> <a href="javascript:alert(1)">bad</a> <a href=\'mailto:a@b.c\'>mail</a>');
assert.equal(r[0].href, "https://example.com");
assert.equal(r.find(x => x.text === "bad").href, "");
assert.equal(r.find(x => x.text === "mail").href, "mailto:a@b.c");

// Nothing a body contains survives as markup: every run is text.
r = runs('<img src=x onerror="invoke(\'run\')"><script>alert(1)</script><svg onload=x>hi');
assert.ok(r.every(x => typeof x.text === "string"));
assert.equal(plain('<img src=x onerror="pwn()"><script>alert(1)</script>hi'), "alert(1)hi");

// Entities, including the numeric ones, and an entity cannot smuggle a tag in.
assert.equal(plain("a &amp; b &lt;i&gt; &#39;q&#x27; &unknown;"), "a & b <i> 'q' &unknown;");
assert.equal(runs("&lt;b&gt;not bold&lt;/b&gt;")[0].bold, false);

// Line breaks are kept for the full body and flattened for the preview.
assert.equal(runs("one<br>two").map(x => x.text).join(""), "one\ntwo");
assert.equal(plain("one<br/>two\n\nthree"), "one two three");

// An unclosed or stray closing tag never goes negative or throws.
assert.equal(runs("</b></b>text<b>")[0].bold, false);
assert.deepEqual(runs(""), []);
console.log("markup: all assertions passed");
