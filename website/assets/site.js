// Client behavior for the hd-lang site: the mobile navigation toggle, the
// theme toggle, heading search, copy buttons on code blocks, passing
// `#code=` from the playground page to the playground, and the home page's
// claims explorers, linked lines, and widening cards.

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

setupMenu();
setupTheme();
setupCopy();
setupSearch();
setupPlayground();
setupOutline();
setupExplorer();
setupLinks();
setupWiden();
