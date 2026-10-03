interface Atom {
  readonly text: string;
  readonly quoted: boolean;
}

interface Form {
  readonly start: number;
  readonly end: number;
  readonly items: readonly (Atom | Form)[];
}

function isForm(item: Atom | Form): item is Form {
  return "items" in item;
}

function atom(form: Form, index: number): string | undefined {
  const item = form.items[index];
  return item && !isForm(item) && !item.quoted ? item.text : undefined;
}

/** Parse generated WAT structurally; strings and comments never create edges. */
function parseModule(source: string): Form {
  let offset = 0;
  const space = (): void => {
    while (offset < source.length) {
      if (/\s/.test(source[offset]!)) {
        offset++;
      } else if (source.startsWith(";;", offset)) {
        const end = source.indexOf("\n", offset);
        offset = end === -1 ? source.length : end + 1;
      } else if (source.startsWith("(;", offset)) {
        offset += 2;
        let depth = 1;
        while (depth > 0 && offset < source.length) {
          if (source.startsWith("(;", offset)) {
            depth++;
            offset += 2;
          } else if (source.startsWith(";)", offset)) {
            depth--;
            offset += 2;
          } else offset++;
        }
        if (depth) throw new Error("unterminated generated WAT comment");
      } else break;
    }
  };
  const form = (): Form => {
    space();
    const start = offset;
    if (source[offset++] !== "(") throw new Error("expected generated WAT form");
    const items: Array<Atom | Form> = [];
    while (true) {
      space();
      if (offset >= source.length) throw new Error("unterminated generated WAT form");
      if (source[offset] === ")") {
        offset++;
        return { start, end: offset, items };
      }
      if (source[offset] === "(") items.push(form());
      else if (source[offset] === '"') {
        const begin = offset++;
        while (offset < source.length && source[offset] !== '"') {
          if (source[offset] === "\\") offset++;
          offset++;
        }
        if (offset >= source.length) throw new Error("unterminated generated WAT string");
        offset++;
        items.push({ text: source.slice(begin, offset), quoted: true });
      } else {
        const begin = offset;
        while (offset < source.length && !/[\s()]/.test(source[offset]!)) offset++;
        items.push({ text: source.slice(begin, offset), quoted: false });
      }
    }
  };
  const module = form();
  space();
  if (atom(module, 0) !== "module" || offset !== source.length)
    throw new Error("expected one generated WAT module");
  return module;
}

/**
 * Link backend declarations, including runtime fragments and recursive types.
 * All backend bindings use unique symbolic IDs, not positional indices. Roots
 * are exports, start, and active initialization segments. `elem declare` is
 * validation metadata, not a root: retain only its live ref.func targets.
 * Keep original source slices so function code and stable IDs do not change.
 */
export function linkWat(source: string): string {
  const module = parseModule(source);
  const forms = module.items.filter(isForm);
  const definitions = new Map<string, Form>();
  const bindings = new Map<Form, string>();
  const live = new Set<string>();
  const pending: Form[] = [];
  const binding = (form: Form): string | undefined => {
    if (atom(form, 0) === "import") {
      const declaration = form.items.find(isForm);
      return declaration && atom(declaration, 1);
    }
    if (!["func", "type", "global", "memory", "table", "tag"].includes(atom(form, 0) ?? ""))
      return undefined;
    return atom(form, 1);
  };
  const declarations = forms.flatMap((form) =>
    atom(form, 0) === "rec" ? form.items.filter(isForm) : [form],
  );
  for (const declaration of declarations) {
    const name = binding(declaration);
    if (!name?.startsWith("$")) continue;
    if (definitions.has(name)) throw new Error(`duplicate generated WAT binding '${name}'`);
    definitions.set(name, declaration);
    bindings.set(declaration, name);
  }
  const hasExport = (form: Form): boolean =>
    form.items.some((item) => isForm(item) && atom(item, 0) === "export");
  for (const declaration of declarations) {
    const kind = atom(declaration, 0);
    if (hasExport(declaration) || (!bindings.has(declaration) && kind !== "elem"))
      pending.push(declaration);
    if (kind === "elem" && atom(declaration, 1) !== "declare") pending.push(declaration);
  }
  const visit = (form: Form): void => {
    for (const item of form.items) {
      if (isForm(item)) visit(item);
      else if (!item.quoted && definitions.has(item.text) && !live.has(item.text)) {
        live.add(item.text);
        pending.push(definitions.get(item.text)!);
      }
    }
  };
  for (let index = 0; index < pending.length; index++) visit(pending[index]!);
  const text = (form: Form): string => source.slice(form.start, form.end);
  const retained = (form: Form): boolean => {
    const name = bindings.get(form);
    return name === undefined || live.has(name);
  };
  const output = forms.flatMap((form) => {
    if (atom(form, 0) === "rec") {
      const types = form.items.filter(isForm).filter(retained);
      return types.length ? [`(rec\n${types.map(text).join("\n")}\n)`] : [];
    }
    if (atom(form, 0) === "elem" && atom(form, 1) === "declare") {
      const targets = form.items
        .filter((item): item is Atom => !isForm(item) && !item.quoted && live.has(item.text))
        .map((item) => item.text);
      return targets.length ? [`(elem declare func ${targets.join(" ")})`] : [];
    }
    return retained(form) ? [text(form)] : [];
  });
  return `(module\n${output.join("\n")}\n)`;
}
