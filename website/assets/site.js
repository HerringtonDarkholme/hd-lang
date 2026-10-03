// Client behavior for the hd-lang site: the mobile navigation toggle, the
// theme toggle, heading search, copy buttons on code blocks, and passing
// `#code=` from the playground page to the playground.

const base = document.body.dataset.base ?? "/";

function setupMenu() {
  const button = document.querySelector(".menu-button");
  if (!button) return;
  const close = () => {
    document.body.classList.remove("nav-open");
    button.setAttribute("aria-expanded", "false");
  };
  button.addEventListener("click", () => {
    const open = document.body.classList.toggle("nav-open");
    button.setAttribute("aria-expanded", String(open));
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") close();
  });
  document.querySelector(".content")?.addEventListener("click", close);
  document.getElementById("sidebar")?.addEventListener("click", (event) => {
    if (event.target instanceof Element && event.target.closest("a")) close();
  });
}

/** The localStorage key layout.ts's head script reads before the first paint. */
const THEME_KEY = "hd-theme";

function setupTheme() {
  const button = document.querySelector(".theme-toggle");
  if (!button) return;
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
  if (!navigator.clipboard) return;
  for (const pre of document.querySelectorAll("main pre.code")) {
    if (pre.closest(".code-window, .claim")) continue;
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
    if (tryLink) actions.append(tryLink);
    actions.append(copy);
    block.append(actions);
  }
}

function setupSearch() {
  const input = document.getElementById("search-input");
  const results = document.getElementById("search-results");
  if (!input || !results) return;
  let entries = null;
  let active = -1;
  const load = async () => {
    if (entries) return entries;
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
      if (target) window.location.href = target.href;
    } else if (event.key === "Escape") {
      input.value = "";
      results.hidden = true;
      input.blur();
    }
  });
  document.addEventListener("click", (event) => {
    if (!(event.target instanceof Node) || !results.parentElement?.contains(event.target))
      results.hidden = true;
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
  if (!match) return null;
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
  const open = document.getElementById("playground-open");
  const sync = () => {
    const hash = window.location.hash.startsWith("#code=") ? window.location.hash : "";
    if (frame) {
      const target = frame.dataset.src + hash;
      if (frame.getAttribute("src") !== target) frame.setAttribute("src", target);
      if (open) open.href = target;
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
  if (!frame && !document.getElementById("playground-code")) return;
  sync();
  window.addEventListener("hashchange", sync);
}

function setupOutline() {
  const links = [...document.querySelectorAll(".toc a")];
  if (links.length === 0 || !("IntersectionObserver" in window)) return;
  const byId = new Map(links.map((link) => [decodeURIComponent(link.hash.slice(1)), link]));
  const visible = new Set();
  const observer = new IntersectionObserver(
    (records) => {
      for (const record of records) {
        if (record.isIntersecting) visible.add(record.target.id);
        else visible.delete(record.target.id);
      }
      const current = links.find((link) => visible.has(decodeURIComponent(link.hash.slice(1))));
      if (current) links.forEach((link) => link.classList.toggle("active", link === current));
    },
    { rootMargin: "0px 0px -70% 0px" },
  );
  for (const id of byId.keys()) {
    const heading = document.getElementById(id);
    if (heading) observer.observe(heading);
  }
}

setupMenu();
setupTheme();
setupCopy();
setupSearch();
setupPlayground();
setupOutline();
