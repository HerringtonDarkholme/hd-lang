// Client behavior for the hd-lang site: the mobile navigation toggle, the
// theme toggle, heading search, copy buttons on code blocks, passing
// `#code=` from the playground page to the playground, and the home page's
// claims explorers, linked lines, widening cards, and the hero's animated sky.

const base = document.body.dataset.base ?? "/";

function setupMenu() {
  const button = document.querySelector(".menu-button");
  if (!button) {
    return;
  }
  const close = () => {
    document.body.classList.remove("nav-open");
    button.setAttribute("aria-expanded", "false");
  };
  button.addEventListener("click", () => {
    const open = document.body.classList.toggle("nav-open");
    button.setAttribute("aria-expanded", String(open));
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      close();
    }
  });
  document.querySelector(".content")?.addEventListener("click", close);
  document.getElementById("sidebar")?.addEventListener("click", (event) => {
    if (event.target instanceof Element && event.target.closest("a")) {
      close();
    }
  });
}

/** The localStorage key layout.ts's head script reads before the first paint. */
const THEME_KEY = "hd-theme";

function setupTheme() {
  const button = document.querySelector(".theme-toggle");
  if (!button) {
    return;
  }
  const root = document.documentElement;
  const dark = () =>
    root.dataset.theme === "dark" ||
    (root.dataset.theme !== "light" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  const label = () => {
    const next = dark() ? "light" : "dark";
    button.setAttribute("aria-label", `Switch to the ${next} theme`);
    button.setAttribute("title", `Switch to the ${next} theme`);
  };
  label();
  button.addEventListener("click", () => {
    const theme = dark() ? "light" : "dark";
    root.dataset.theme = theme;
    try {
      localStorage.setItem(THEME_KEY, theme);
    } catch {
      // Storage can be blocked; the choice then lasts for this page only.
    }
    // The framed playground reads the stored choice when it loads; tell it now too.
    for (const frame of document.querySelectorAll("iframe")) {
      try {
        frame.contentDocument.documentElement.dataset.theme = theme;
      } catch {
        // A frame from another origin keeps its own theme.
      }
    }
    label();
  });
}

/**
 * Gives every code block a Copy button. It shares a row with the block's Try in
 * REPL or playground link; a plain ```text block gets a wrapper to hold it.
 */
function setupCopy() {
  if (!navigator.clipboard) {
    return;
  }
  for (const pre of document.querySelectorAll("main pre.code")) {
    if (pre.closest(".code-window, .claim")) {
      continue;
    }
    let block = pre.parentElement;
    if (!block?.classList.contains("code-block")) {
      block = document.createElement("div");
      block.className = "code-block";
      pre.replaceWith(block);
      block.append(pre);
    }
    const actions = document.createElement("div");
    actions.className = "code-actions";
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "copy-button";
    copy.textContent = "Copy";
    copy.setAttribute("aria-label", "Copy the code");
    copy.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(pre.textContent ?? "");
        copy.textContent = "Copied";
        copy.dataset.copied = "";
      } catch {
        copy.textContent = "Failed";
      }
      setTimeout(() => {
        copy.textContent = "Copy";
        delete copy.dataset.copied;
      }, 1500);
    });
    const tryLink = block.querySelector(":scope > .try-link");
    if (tryLink) {
      actions.append(tryLink);
    }
    actions.append(copy);
    block.append(actions);
  }
}

function setupSearch() {
  const input = document.getElementById("search-input");
  const results = document.getElementById("search-results");
  if (!input || !results) {
    return;
  }
  let entries = null;
  let active = -1;
  const load = async () => {
    if (entries) {
      return entries;
    }
    try {
      const response = await fetch(`${base}search-index.json`);
      entries = await response.json();
    } catch {
      entries = [];
    }
    return entries;
  };
  const links = () => [...results.querySelectorAll("a")];
  const highlight = (index) => {
    const items = links();
    items.forEach((item, position) => item.classList.toggle("active", position === index));
    active = index;
    items[index]?.scrollIntoView({ block: "nearest" });
  };
  const render = async () => {
    const query = input.value.trim().toLowerCase();
    results.replaceChildren();
    active = -1;
    if (query === "") {
      results.hidden = true;
      return;
    }
    const words = query.split(/\s+/);
    const matches = (await load())
      .filter((entry) => {
        const text = `${entry.title} ${entry.page}`.toLowerCase();
        return words.every((word) => text.includes(word));
      })
      .sort((left, right) => {
        const leftStarts = left.title.toLowerCase().startsWith(query) ? 0 : 1;
        const rightStarts = right.title.toLowerCase().startsWith(query) ? 0 : 1;
        return leftStarts - rightStarts;
      })
      .slice(0, 20);
    for (const entry of matches) {
      const item = document.createElement("li");
      const link = document.createElement("a");
      link.href = entry.url;
      link.textContent = entry.title;
      if (entry.title !== entry.page) {
        const page = document.createElement("small");
        page.textContent = entry.page;
        link.append(page);
      }
      item.append(link);
      results.append(item);
    }
    if (matches.length === 0) {
      const item = document.createElement("li");
      item.textContent = "No matches";
      item.style.padding = "6px 10px";
      results.append(item);
    }
    results.hidden = false;
  };
  input.addEventListener("input", render);
  input.addEventListener("focus", () => void load());
  input.addEventListener("keydown", (event) => {
    const items = links();
    if (event.key === "ArrowDown" && items.length > 0) {
      event.preventDefault();
      highlight((active + 1) % items.length);
    } else if (event.key === "ArrowUp" && items.length > 0) {
      event.preventDefault();
      highlight((active - 1 + items.length) % items.length);
    } else if (event.key === "Enter") {
      const target = items[active] ?? items[0];
      if (target) {
        window.location.href = target.href;
      }
    } else if (event.key === "Escape") {
      input.value = "";
      results.hidden = true;
      input.blur();
    }
  });
  document.addEventListener("click", (event) => {
    if (!(event.target instanceof Node) || !results.parentElement?.contains(event.target)) {
      results.hidden = true;
    }
  });
  document.addEventListener("keydown", (event) => {
    const typing = event.target instanceof HTMLElement && event.target.matches("input, textarea");
    if (event.key === "/" && !typing) {
      event.preventDefault();
      input.focus();
    }
  });
}

function decodeCode(hash) {
  const match = /^#code=([A-Za-z0-9_-]*)/.exec(hash);
  if (!match) {
    return null;
  }
  try {
    const base64 = match[1].replaceAll("-", "+").replaceAll("_", "/");
    const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

function setupPlayground() {
  const frame = document.getElementById("playground-frame");
  const sync = () => {
    const hash = window.location.hash.startsWith("#code=") ? window.location.hash : "";
    if (frame) {
      const target = frame.dataset.src + hash;
      if (frame.getAttribute("src") !== target) {
        frame.setAttribute("src", target);
      }
      return;
    }
    const wrap = document.getElementById("playground-code-wrap");
    const code = document.getElementById("playground-code");
    const source = decodeCode(hash);
    if (wrap && code && source !== null) {
      code.textContent = source;
      wrap.hidden = false;
    }
  };
  if (!frame && !document.getElementById("playground-code")) {
    return;
  }
  sync();
  window.addEventListener("hashchange", sync);
}

function setupOutline() {
  const links = [...document.querySelectorAll(".toc a")];
  if (links.length === 0 || !("IntersectionObserver" in window)) {
    return;
  }
  const byId = new Map(links.map((link) => [decodeURIComponent(link.hash.slice(1)), link]));
  const visible = new Set();
  const observer = new IntersectionObserver(
    (records) => {
      for (const record of records) {
        if (record.isIntersecting) {
          visible.add(record.target.id);
        } else {
          visible.delete(record.target.id);
        }
      }
      const current = links.find((link) => visible.has(decodeURIComponent(link.hash.slice(1))));
      if (current) {
        links.forEach((link) => link.classList.toggle("active", link === current));
      }
    },
    { rootMargin: "0px 0px -70% 0px" },
  );
  for (const id of byId.keys()) {
    const heading = document.getElementById(id);
    if (heading) {
      observer.observe(heading);
    }
  }
}

/**
 * The home page's claims explorers: one claim shows at a time, chosen by its
 * tab, in a box as tall as the tallest claim. Arrow keys, Home, and End move
 * between tabs.
 */
function setupExplorer() {
  for (const explorer of document.querySelectorAll("[data-explorer]")) {
    const tabs = [...explorer.querySelectorAll('[role="tab"]')];
    const select = (tab, focus) => {
      for (const other of tabs) {
        const selected = other === tab;
        other.setAttribute("aria-selected", String(selected));
        other.tabIndex = selected ? 0 : -1;
        // The style sheet hides the other panels but keeps their height.
        document
          .getElementById(other.getAttribute("aria-controls"))
          ?.classList.toggle("is-current", selected);
      }
      if (focus) {
        tab.focus();
      }
    };
    for (const [index, tab] of tabs.entries()) {
      tab.addEventListener("click", () => select(tab, false));
      tab.addEventListener("keydown", (event) => {
        const last = tabs.length - 1;
        const next = {
          ArrowDown: index === last ? 0 : index + 1,
          ArrowRight: index === last ? 0 : index + 1,
          ArrowUp: index === 0 ? last : index - 1,
          ArrowLeft: index === 0 ? last : index - 1,
          Home: 0,
          End: last,
        }[event.key];
        if (next !== undefined) {
          event.preventDefault();
          select(tabs[next], true);
        }
      });
    }
    const current = tabs.find((tab) => tab.getAttribute("aria-selected") === "true") ?? tabs[0];
    if (current) {
      select(current, false);
      explorer.classList.add("is-ready");
    }
  }
}

/**
 * Linked parts: in a `[data-links]` group, pointing at or focusing a list
 * item, or pointing at a code or output line, lights every part that shares
 * its key. A line may carry several keys. The first item starts lit, and the
 * last one chosen stays lit. A lit item may grow, so its list
 * (`[data-link-items]`) is held at the height of its tallest lit state.
 */
function setupLinks() {
  for (const group of document.querySelectorAll("[data-links]")) {
    const parts = [...group.querySelectorAll("[data-link]")];
    const keys = [...new Set(parts.flatMap((part) => part.dataset.link.split(" ")))];
    let current = keys[0];
    const light = (key) => {
      for (const part of parts) {
        part.classList.toggle("is-linked", part.dataset.link.split(" ").includes(key));
      }
    };
    const items = group.querySelector("[data-link-items]");
    const reserve = () => {
      if (!items) {
        return;
      }
      group.classList.add("is-measuring");
      items.style.minHeight = "";
      let tallest = 0;
      for (const key of keys) {
        light(key);
        tallest = Math.max(tallest, items.offsetHeight);
      }
      light(current);
      items.style.minHeight = `${tallest}px`;
      // Lay out the final state before transitions return, so nothing animates.
      void items.offsetHeight;
      group.classList.remove("is-measuring");
    };
    for (const part of parts) {
      const choose = () => {
        current = part.dataset.link.split(" ")[0];
        light(current);
      };
      part.addEventListener("pointerenter", choose);
      part.addEventListener("focus", choose);
    }
    group.classList.add("is-ready");
    light(current);
    reserve();
    window.addEventListener("resize", reserve);
    document.fonts?.ready.then(reserve);
  }
}

/**
 * Widening cards: in a `[data-widen]` grid, pointing at, focusing, or
 * clicking a card's `.agent-pillar-more` button widens that card (the grid's
 * `data-open` names it, from 1) and marks the others squeezed; the style sheet
 * then sets the card's texts in two columns under their labels.
 * Leaving the grid restores it. Where the browser has view transitions, the
 * cards and buttons glide between the two layouts. Side by side, the grid is
 * held at the height of its tallest state, so nothing below moves; stacked
 * on a narrow screen, it grows instead.
 */
function setupWiden() {
  for (const grid of document.querySelectorAll("[data-widen]")) {
    const items = [...grid.querySelectorAll("[data-widen-item]")];
    let current = 0;
    let busy = false;
    const show = (open) => {
      if (open > 0) {
        grid.dataset.open = String(open);
      } else {
        delete grid.dataset.open;
      }
      items.forEach((item, index) => {
        item.classList.toggle("is-open", index + 1 === open);
        item.classList.toggle("is-squeezed", open > 0 && index + 1 !== open);
        item
          .querySelector(".agent-pillar-more")
          ?.setAttribute("aria-expanded", String(index + 1 === open));
      });
    };
    // Where the pointer last was; null until it moves, as with a keyboard.
    // Pointer events stop during a view transition, so a move afterwards
    // also closes a card the pointer left meanwhile.
    let pointer = null;
    document.addEventListener(
      "pointermove",
      (event) => {
        pointer = { x: event.clientX, y: event.clientY };
        if (!busy && current > 0 && event.target instanceof Node && !grid.contains(event.target)) {
          change(0);
        }
      },
      { passive: true },
    );
    // Whether the pointer is over the grid; an unknown answer (no pointer
    // yet, or the root while a transition's pictures still cover the page)
    // counts as yes, and the next move settles it.
    const pointerInside = () => {
      if (!pointer) {
        return false;
      }
      const under = document.elementFromPoint(pointer.x, pointer.y);
      return !under || under === document.documentElement || grid.contains(under);
    };
    const stacked = window.matchMedia("(max-width: 1000px)");
    const still = window.matchMedia("(prefers-reduced-motion: reduce)");
    const glide = Boolean(document.startViewTransition);
    grid.classList.toggle("has-view-transitions", glide);
    const change = (open) => {
      if (open === current) {
        return;
      }
      current = open;
      if (!glide || stacked.matches || still.matches) {
        show(open);
        return;
      }
      busy = true;
      const transition = document.startViewTransition(() => show(open));
      transition.finished.finally(() => {
        busy = false;
        // Pointer events stopped during the transition: close if it left.
        if (current > 0 && !pointerInside() && !grid.contains(document.activeElement)) {
          change(0);
        }
      });
    };
    const reserve = () => {
      grid.style.height = "";
      if (stacked.matches) {
        return;
      }
      grid.classList.add("is-measuring");
      let tallest = 0;
      for (let open = 0; open <= items.length; open += 1) {
        show(open);
        tallest = Math.max(tallest, grid.offsetHeight);
      }
      show(current);
      grid.style.height = `${tallest}px`;
      // Lay out the final state before transitions return, so nothing animates.
      void grid.offsetHeight;
      grid.classList.remove("is-measuring");
    };
    items.forEach((item, index) => {
      const button = item.querySelector(".agent-pillar-more");
      button?.addEventListener("pointerenter", () => change(index + 1));
      button?.addEventListener("focus", () => change(index + 1));
      button?.addEventListener("click", () => change(current === index + 1 ? 0 : index + 1));
    });
    grid.addEventListener("pointerleave", () => {
      if (!busy) {
        change(0);
      }
    });
    grid.addEventListener("focusout", (event) => {
      if (!(event.relatedTarget instanceof Node) || !grid.contains(event.relatedTarget)) {
        if (!pointerInside()) {
          change(0);
        }
      }
    });
    grid.classList.add("is-ready");
    reserve();
    window.addEventListener("resize", reserve);
    document.fonts?.ready.then(reserve);
  }
}

/**
 * The hero logo's satellite rests at the top of its orbit. Now and then, or
 * when the logo is hovered or clicked, it makes one eased lap round the ring
 * and settles home. On the far side it passes behind the letters: the logo
 * draws the ball twice, under the letters and over them, and the two
 * cross-fade where the orbit turns. With reduced motion it stays at rest.
 */
// Where the ball is, in the logo's own units; the sky's rose glow follows it.
const satellite = { x: 1162, y: 318 };

function setupOrbit() {
  const logo = document.querySelector(".hero-logo");
  const front = logo?.querySelector(".logo-ball-front");
  const back = logo?.querySelector(".logo-ball-back");
  if (!front || !back) {
    return;
  }
  const balls = [...logo.querySelectorAll(".logo-ball, .logo-ball-glow")];
  const still = window.matchMedia("(prefers-reduced-motion: reduce)");
  // The orbit's centre line as traced with the logo: an ellipse about
  // (cx, cy) with radii rx and ry, tilted, and a little wider on its near side.
  const [cx, cy, rx, ry, tilt, swell] = [621.4, 627.8, 630.2, 146.6, -0.508, 0.185];
  const along = (deg) => {
    const angle = (deg * Math.PI) / 180;
    const major = rx * Math.cos(angle);
    const minor = ry * Math.sin(angle) * (1 + swell * Math.cos(angle));
    return [
      cx + major * Math.cos(tilt) - minor * Math.sin(tilt),
      cy + major * Math.sin(tilt) + minor * Math.cos(tilt),
    ];
  };
  const home = [satellite.x, satellite.y];
  const start = along(0);
  const clamp = (value) => Math.min(1, Math.max(0, value));
  const place = (deg) => {
    const [atX, atY] = along(deg);
    satellite.x = atX + home[0] - start[0];
    satellite.y = atY + home[1] - start[1];
    // Nearer the viewer on the front of the orbit, so a little larger.
    const size = 1 + 0.16 * Math.sin((deg * Math.PI) / 180);
    const transform = `translate(${satellite.x} ${satellite.y}) scale(${size}) translate(${-home[0]} ${-home[1]})`;
    for (const ball of balls) {
      ball.setAttribute("transform", transform);
    }
    // In front from just before home round the near side, fading over ten
    // degrees at each turn.
    const turn = deg > 270 ? deg - 360 : deg;
    const shown = clamp((turn + 18) / 10) * clamp((198 - turn) / 10);
    front.setAttribute("opacity", String(shown));
    back.setAttribute("opacity", String((1 - shown) * 0.9));
  };

  const LAP = 3090;
  const REST = 12000;
  const ease = (part) => (part < 0.5 ? 4 * part ** 3 : 1 - (2 - 2 * part) ** 3 / 2);
  let visible = true;
  let started = -1;
  let timer = 0;
  const schedule = (delay) => {
    clearTimeout(timer);
    timer = setTimeout(lap, delay);
  };
  const step = (now) => {
    const done = Math.min(1, (now - started) / LAP);
    place((360 * ease(done)) % 360);
    if (done < 1) {
      requestAnimationFrame(step);
      return;
    }
    started = -1;
    place(0);
    schedule(REST);
  };
  function lap() {
    if (started >= 0) {
      return;
    }
    if (still.matches || document.hidden || !visible) {
      schedule(REST);
      return;
    }
    clearTimeout(timer);
    started = performance.now();
    requestAnimationFrame(step);
  }
  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
  }).observe(logo);
  logo.addEventListener("pointerenter", lap);
  logo.addEventListener("click", lap);
  schedule(3500);
}

/**
 * The hero's sky, drawn by a WebGL fragment shader behind the hero: a veil
 * of silk sweeping in from the lower left with a bright crest, rings about
 * the logo, stars, and a planet's horizon over a glossy floor. It drifts slowly and leans with the pointer, or with the
 * device's tilt where the browser gives it without asking. It draws only
 * while the hero is on screen, draws one still frame for reduced motion,
 * and leaves the style sheet's sky in place where WebGL is missing.
 */
const SKY_SHADER = `
precision highp float;
uniform vec2 u_res;
uniform float u_time;
uniform vec2 u_pointer;
uniform float u_light;
uniform vec3 u_logo;
uniform float u_floor;
uniform vec4 u_text;
uniform vec2 u_sat;

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
             mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 4; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

// Broken filaments running along a sheet: thin lines at whole numbers of x,
// each lit in patches that drift along the sheet.
float filaments(float x, float along, float t, float seed) {
  float id = floor(x + 0.5);
  float d = abs(x - id);
  float lit = noise(vec2(along * 4.0 - t * 1.5 + hash(vec2(id, seed)) * 20.0, id * 0.7 + seed));
  return exp(-d * d * 60.0) * smoothstep(0.45, 0.85, lit) * hash(vec2(seed, id));
}

// The distance from p to the curve y = a x^2 + b x + c, measured across it.
float across(vec2 p, vec3 k) {
  float slope = 2.0 * k.x * p.x + k.y;
  return (p.y - (k.x * p.x * p.x + k.y * p.x + k.z)) / sqrt(1.0 + slope * slope);
}

void main() {
  vec2 uv = gl_FragCoord.xy / u_res;
  float t = u_time * 0.05;
  // Positions measured from the logo's centre, in widths of its orbit.
  vec2 q = (gl_FragCoord.xy - u_logo.xy) / u_logo.z;
  float px = u_logo.z;

  // The veil: sheets of silk sweeping in from the lower left and meeting
  // by the logo, mostly dark and see-through, their edges lit. The lower
  // sheet runs from a faint edge up to the bright crest; the upper one from
  // the crest up to the swoosh.
  // Drawn a little narrower than measured, so the crest's hot stretch sits
  // between the headline and the logo.
  vec2 qv = vec2((q.x + 0.25) * 1.25 - 0.25, q.y) + u_pointer * vec2(0.018, 0.012);
  float sway = 0.012 * sin(t * 1.3);
  vec3 lowCurve = vec3(-0.304, -0.02, 0.159 + sway * 0.6);
  vec3 crestCurve = vec3(-0.219, 0.029, 0.181 + sway);
  vec3 swooshCurve = vec3(-0.599, -0.319, 0.287 + 0.015 * sin(t * 1.1 + 2.0));
  // The edges waver a little, like cloth.
  float waver = fbm(vec2(qv.x * 6.0 - t * 0.5, t * 0.2)) - 0.5;
  float dl = across(qv, lowCurve) + waver * 0.025;
  float dc = across(qv, crestCurve) + waver * 0.012;
  float ds = across(qv, swooshCurve) + (fbm(vec2(qv.x * 5.0 + 4.0, t * 0.2)) - 0.5) * 0.03;
  float span = smoothstep(-1.5, -1.05, qv.x) * (1.0 - smoothstep(-0.38, -0.2, qv.x));

  // The crest: a white-hot core, brightest a little left of the logo.
  float hotC = exp(-pow((qv.x + 0.72) * 3.2, 2.0));
  float crest = (exp(-pow(dc * px / (1.4 + 1.6 * hotC), 2.0)) * (0.35 + 1.8 * hotC)
    + exp(-abs(dc) * px / 7.0) * (0.10 + 0.6 * hotC)
    + exp(-abs(dc) * px / 40.0) * (0.04 + 0.3 * hotC)) * span;
  // A bloom about the hot stretch of the crest.
  float bloom = exp(-length((qv - vec2(-0.72, 0.018)) * vec2(1.6, 5.0)) * 2.2) * 0.32 * span;
  float lowEdge = (exp(-pow(dl * px / 1.2, 2.0)) * 0.3 + exp(-abs(dl) * px / 10.0) * 0.06)
    * span * (0.4 + 0.9 * fbm(vec2(qv.x * 5.0 - t * 0.5, 1.0)));
  float rimS = (exp(-pow(ds * px / 1.2, 2.0)) * 0.3 + exp(-abs(ds) * px / 12.0) * 0.05)
    * span * (0.3 + 0.9 * fbm(vec2(qv.x * 5.0 - t * 0.6, 3.0)));

  // Each sheet's fill: a faint fabric, lit toward the crest, with filaments
  // running along it.
  float sA = clamp(dl / max(dl - dc, 0.001), 0.0, 1.0);
  float inA = smoothstep(-0.003, 0.004, dl) * smoothstep(0.004, -0.003, dc);
  float fabricA = fbm(vec2(qv.x * 4.0 - t * 0.4, sA * 5.0)) ;
  float fA = filaments(sA * 26.0 + (fabricA - 0.5) * 2.0, qv.x, t, 1.0)
    + filaments(sA * 61.0 + (fabricA - 0.5) * 3.0, qv.x, t, 2.0) * 0.6;
  float sheetA = inA * span * ((0.06 + 0.42 * pow(sA, 2.0)) * (0.3 + fabricA)
    + fA * (0.1 + 0.45 * sA));

  float sB = clamp(dc / max(dc - ds, 0.001), 0.0, 1.0);
  float inB = smoothstep(-0.003, 0.004, dc) * smoothstep(0.004, -0.003, ds);
  float fabricB = fbm(vec2(qv.x * 3.0 + t * 0.3, sB * 4.0 + 7.0));
  float fB = filaments(sB * 18.0 + (fabricB - 0.5) * 2.5, qv.x, t, 5.0);
  float sheetB = inB * span * ((0.04 + 0.2 * pow(1.0 - sB, 3.0)) * (0.3 + fabricB)
    + fB * 0.18 * (1.0 - sB * 0.6));
  // Below the lower edge, a dimmer fold curls toward the floor.
  float fold = smoothstep(0.004, -0.01, dl) * exp(dl * 9.0) * span
    * (0.04 + 0.25 * filaments(dl * 40.0 + fbm(qv * 3.0) * 3.0, qv.x, t, 9.0));

  float sheetC = sheetA + fold + bloom;
  float sheetS = sheetB;

  // Glitter caught along the crest and the sheets' edges.
  vec2 cell = floor(gl_FragCoord.xy / 5.0);
  vec2 inCell = fract(gl_FragCoord.xy / 5.0) - 0.5;
  float g = hash(cell);
  float near = (exp(-abs(dc) * 16.0) + exp(-abs(dl) * 22.0) * 0.5 + inA * 0.25) * span;
  float glitter = step(0.965, g) * near
    * (0.5 + 0.5 * sin(u_time * (1.0 + g * 3.0) + g * 50.0))
    * exp(-dot(inCell, inCell) * 30.0);

  // Rings: wide ellipses about the logo, beyond its orbit, seen on the
  // right. Teal above, rose below, brightest at the far right.
  vec2 qr = q - vec2(0.04, -0.05) + u_pointer * vec2(0.006, 0.004);
  float tilt = 0.17;
  vec2 rr = vec2(cos(tilt) * qr.x + sin(tilt) * qr.y, -sin(tilt) * qr.x + cos(tilt) * qr.y);
  float r = length(rr * vec2(1.0, 3.1));
  float angle = atan(rr.y * 3.1, rr.x);
  float rings = 0.0;
  for (int i = 0; i < 5; i++) {
    float fi = float(i);
    float radius = 0.64 + fi * 0.035 + 0.004 * sin(t * 2.0 + fi);
    float line = exp(-pow((r - radius) * px / (1.0 + fi * 0.3), 2.0));
    float dust = 0.4 + 0.6 * noise(vec2(angle * 9.0 - t * (1.0 + fi * 0.3), fi * 5.0));
    rings += line * dust * (0.95 - fi * 0.14);
  }
  rings += exp(-pow((r - 0.7) * 9.0, 2.0)) * 0.05;
  float rosy = smoothstep(0.0, -0.08, rr.y);
  // The upper arcs show right of the logo; the rose lower arc sweeps on
  // under it, toward the floor.
  float ringSide = mix(smoothstep(0.05, 0.45, rr.x), smoothstep(-0.45, 0.1, rr.x), rosy)
    * (0.35 + 0.65 * exp(-abs(angle) * 1.6));
  float sweep = exp(-pow((r - 0.79) * px / 1.4, 2.0)) * rosy * smoothstep(-0.5, -0.1, rr.x)
    * (1.0 - smoothstep(0.2, 0.7, rr.x)) * 0.9;
  rings = rings * ringSide + sweep;

  // The horizon: a planet's rim, highest left of centre, falling away to
  // the right, over a glossy floor.
  float hx = uv.x + u_pointer.x * 0.004;
  float horizon = (u_floor - 0.07 * px * pow((q.x + 1.0) / 1.3, 2.0)) / u_res.y
    + u_pointer.y * 0.003;
  float hpx = (uv.y - horizon) * u_res.y;
  float above = smoothstep(-1.0, 1.0, hpx);
  float depth = max(horizon - uv.y, 0.0);
  float rimLit = 0.25 + 0.5 * exp(-pow((hx - 0.36) * 4.5, 2.0)) + 0.9 * smoothstep(0.75, 1.0, hx);
  float rim = (exp(-abs(hpx) / 1.3) * 0.7 + exp(-abs(hpx) / 10.0) * 0.12) * rimLit;
  // Glossy curves in the floor, and the crest's reflection near the rim.
  float qf = depth * 22.0 + (fbm(vec2(hx * 2.2 + t * 0.15, depth * 5.0 - t * 0.2)) - 0.5) * 2.6;
  float gloss = exp(-pow((fract(qf) - 0.5) * 8.0, 2.0))
    * pow(noise(vec2(hx * 5.0 - t * 0.2, floor(qf) * 3.1)), 2.0) * exp(-depth * 6.0);
  float reflection = exp(-pow((q.x + 0.95) * 2.6, 2.0)) * exp(-depth * u_res.y / 30.0)
    * (0.6 + 0.8 * noise(vec2(q.x * 14.0, depth * 60.0 - t * 2.0)));
  float corner = exp(-length((uv - vec2(1.0, 0.0)) * vec2(1.6, 3.4)) * 5.0);
  float floorLight = (1.0 - above) * (gloss * 0.6 * rimLit + reflection * 0.6) + corner * 0.9;

  // A faint teal haze behind it all, thickest around the veil.
  float haze = fbm(q * 1.6 + vec2(t * 0.15, 0.0))
    * exp(-length((q - vec2(-0.6, 0.05)) * vec2(0.9, 1.6)) * 1.6) * 0.22
    + exp(-length((q - vec2(0.0, -0.05)) * vec2(1.0, 1.3)) * 2.6) * 0.12
    + fbm(q * 2.2 - vec2(t * 0.1, 0.0)) * exp(-length((q - vec2(0.75, 0.05)) * vec2(1.0, 1.8)) * 2.4) * 0.2;
  float roseGlow = exp(-length(q - u_sat) * 6.0) * 0.3;

  // Stars, sparse, a few twinkling.
  vec2 sg = (gl_FragCoord.xy + u_pointer * 5.0) / 48.0;
  vec2 sc = floor(sg);
  float h = hash(sc);
  float star = 0.0;
  if (h > 0.9) {
    vec2 sp = vec2(hash(sc + 3.1), hash(sc + 7.7)) * 0.8 + 0.1;
    float dist = length(fract(sg) - sp) * 48.0;
    float tw = 0.6 + 0.4 * sin(u_time * (0.5 + h * 1.4) + h * 40.0);
    star = exp(-dist * dist / 1.1) * tw * (h - 0.9) * 7.0 * above;
  }

  vec3 mint = mix(vec3(0.36, 0.95, 0.90), vec3(0.05, 0.58, 0.54), u_light);
  vec3 teal = mix(vec3(0.06, 0.30, 0.32), vec3(0.45, 0.80, 0.78), u_light);
  vec3 rose = mix(vec3(1.0, 0.42, 0.72), vec3(0.85, 0.41, 0.60), u_light);
  vec3 white = mix(vec3(0.92, 1.0, 1.0), vec3(0.20, 0.62, 0.58), u_light);

  // The veil dims behind the hero's text, so the words stay clear.
  vec2 fromText = max(u_text.xy - gl_FragCoord.xy, gl_FragCoord.xy - u_text.zw);
  float behindText = smoothstep(0.12, -0.08, max(fromText.x, fromText.y) / px);
  float clear = 1.0 - 0.7 * behindText;
  float veil = (crest + rimS + lowEdge) * above * clear;
  float hot = smoothstep(0.7, 1.6, crest);
  vec3 col = mint * veil + white * hot
    + mint * (sheetC + sheetS) * above * clear
    + white * glitter * above * clear
    + mix(mint, rose, rosy) * rings * above
    + mint * (rim + floorLight) + white * rim * 0.3
    + teal * haze * mix(1.0, 0.3, u_light)
    + rose * roseGlow * mix(1.0, 0.5, u_light) + white * star;
  float a = clamp(max(max(col.r, col.g), col.b), 0.0, 1.0);
  // On a light page the light reads as ink: deeper teal, with more cover.
  float ink = clamp(a * 1.1, 0.0, 0.7);
  col = mix(col, col / max(a, 0.001) * 0.8 * ink, u_light);
  a = mix(a, ink, u_light);

  // The floor itself: a dark glossy plane.
  float floorShade = (1.0 - above) * mix(0.8, 0.08, u_light) * smoothstep(0.0, 0.02, depth);
  vec3 floorColor = mix(vec3(0.012, 0.028, 0.034), vec3(0.85, 0.93, 0.92), u_light);
  col += floorColor * floorShade * (1.0 - a);
  a = clamp(a + floorShade * (1.0 - a), 0.0, 1.0);
  gl_FragColor = vec4(col, a);
}
`;

function setupSky() {
  const sky = document.querySelector(".hero-sky");
  const canvas = document.createElement("canvas");
  const gl = sky && canvas.getContext("webgl", { premultipliedAlpha: true, alpha: true });
  if (!gl) {
    return;
  }
  const compile = (type, source) => {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    return gl.getShaderParameter(shader, gl.COMPILE_STATUS) ? shader : null;
  };
  const vertex = compile(
    gl.VERTEX_SHADER,
    "attribute vec2 a; void main() { gl_Position = vec4(a, 0.0, 1.0); }",
  );
  const fragment = compile(gl.FRAGMENT_SHADER, SKY_SHADER);
  if (!vertex || !fragment) {
    return;
  }
  const program = gl.createProgram();
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    return;
  }
  gl.useProgram(program);
  // One triangle that covers the canvas.
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, "a");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  const uniform = (name) => gl.getUniformLocation(program, name);
  const uniforms = {
    res: uniform("u_res"),
    time: uniform("u_time"),
    pointer: uniform("u_pointer"),
    light: uniform("u_light"),
    logo: uniform("u_logo"),
    floor: uniform("u_floor"),
    text: uniform("u_text"),
    sat: uniform("u_sat"),
  };
  canvas.className = "hero-sky-gl";
  sky.prepend(canvas);
  sky.classList.add("has-gl");

  const root = document.documentElement;
  const darkScheme = window.matchMedia("(prefers-color-scheme: dark)");
  const still = window.matchMedia("(prefers-reduced-motion: reduce)");
  const isLight = () =>
    root.dataset.theme === "light" || (root.dataset.theme !== "dark" && !darkScheme.matches);
  const logo = document.querySelector(".hero-logo");
  // The logo's centre and the width of its orbit, in canvas pixels; the
  // shader lays the sky out around them.
  let logoAt = [0, 0, 1];
  // The horizon sits just below the hero's text and logo.
  let floorAt = 0;
  const content = [...document.querySelectorAll(".hero-text, .hero-art")];
  // The text's box, left, bottom, right and top, where the veil dims.
  const words = document.querySelector(".hero-text");
  let textAt = [0, 0, 0, 0];
  // How the logo's own units map to canvas pixels, for the satellite.
  let logoUnits = [0, 0, 0];
  const resize = () => {
    const scale = Math.min(window.devicePixelRatio || 1, 1.5);
    const box = sky.getBoundingClientRect();
    canvas.width = Math.max(1, Math.round(box.width * scale));
    canvas.height = Math.max(1, Math.round(box.height * scale));
    gl.viewport(0, 0, canvas.width, canvas.height);
    const below = Math.max(...content.map((part) => part.getBoundingClientRect().bottom), box.top);
    floorAt = (box.bottom - below - 28) * scale;
    if (words) {
      const text = words.getBoundingClientRect();
      textAt = [
        (text.left - box.left) * scale,
        (box.bottom - text.bottom) * scale,
        (text.right - box.left) * scale,
        (box.bottom - text.top) * scale,
      ];
    }
    if (logo) {
      const art = logo.getBoundingClientRect();
      logoAt = [
        (art.left + art.width / 2 - box.left) * scale,
        (box.bottom - art.top - art.height / 2) * scale,
        Math.max(1, art.width * 0.94 * scale),
      ];
      const view = logo.viewBox.baseVal;
      const unit = Math.min(art.width / view.width, art.height / view.height);
      logoUnits = [
        (art.left + (art.width - view.width * unit) / 2 - view.x * unit - box.left) * scale,
        (box.bottom - art.top - (art.height - view.height * unit) / 2 + view.y * unit) * scale,
        unit * scale,
      ];
    }
  };
  // The pointer, from -1 to 1 across the window, eased toward its target.
  const target = [0, 0];
  const pointer = [0, 0];
  window.addEventListener(
    "pointermove",
    (event) => {
      target[0] = (event.clientX / window.innerWidth) * 2 - 1;
      target[1] = 1 - (event.clientY / window.innerHeight) * 2;
    },
    { passive: true },
  );
  window.addEventListener("deviceorientation", (event) => {
    if (event.gamma === null || event.beta === null) {
      return;
    }
    target[0] = Math.max(-1, Math.min(1, event.gamma / 30));
    target[1] = Math.max(-1, Math.min(1, (45 - event.beta) / 30));
  });

  const start = performance.now();
  const draw = (now) => {
    pointer[0] += (target[0] - pointer[0]) * 0.04;
    pointer[1] += (target[1] - pointer[1]) * 0.04;
    gl.uniform2f(uniforms.res, canvas.width, canvas.height);
    // Twenty seconds in, the slow drift has found a pleasant shape.
    gl.uniform1f(uniforms.time, 20 + (still.matches ? 0 : (now - start) / 1000));
    gl.uniform2f(uniforms.pointer, pointer[0], pointer[1]);
    gl.uniform1f(uniforms.light, isLight() ? 1 : 0);
    gl.uniform3f(uniforms.logo, logoAt[0], logoAt[1], logoAt[2]);
    gl.uniform1f(uniforms.floor, floorAt);
    gl.uniform4f(uniforms.text, textAt[0], textAt[1], textAt[2], textAt[3]);
    gl.uniform2f(
      uniforms.sat,
      (logoUnits[0] + satellite.x * logoUnits[2] - logoAt[0]) / logoAt[2],
      (logoUnits[1] - satellite.y * logoUnits[2] - logoAt[1]) / logoAt[2],
    );
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
  let visible = true;
  let frame = 0;
  const loop = (now) => {
    draw(now);
    frame = visible && !still.matches && !document.hidden ? requestAnimationFrame(loop) : 0;
  };
  const wake = () => {
    if (!frame) {
      frame = requestAnimationFrame(loop);
    }
  };
  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    if (visible) {
      wake();
    }
  }).observe(sky);
  document.addEventListener("visibilitychange", wake);
  still.addEventListener("change", wake);
  // A theme switch redraws a still sky too.
  new MutationObserver(wake).observe(root, { attributes: true, attributeFilter: ["data-theme"] });
  darkScheme.addEventListener("change", wake);
  // The sky follows the hero's size, and the text and logo it is laid out
  // around, which move when the fonts arrive.
  const sizes = new ResizeObserver(() => {
    resize();
    wake();
  });
  for (const part of [sky, ...content]) {
    sizes.observe(part);
  }
  resize();
  wake();
}

setupMenu();
setupTheme();
setupCopy();
setupSearch();
setupPlayground();
setupOutline();
setupExplorer();
setupLinks();
setupWiden();
setupOrbit();
setupSky();
