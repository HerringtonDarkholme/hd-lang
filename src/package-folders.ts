// The folder graph of a package (spec/lang/10-modules.md#dependency-cycles):
// each module's folder, and one shortest loop per tangle of folders that
// use each other, for the linker's `folder-cycle` error (src/package.ts).

/** A package module, as the folder graph needs it. */
export interface FolderModule {
  readonly path: string;
  readonly identity: string;
}

/** A use of `target` in a module of a package. */
export interface FolderUse<M extends FolderModule> {
  readonly target: M;
}

/** An edge of the folder graph, with the first use that makes it. */
export interface FolderEdge<M extends FolderModule, U extends FolderUse<M>> {
  readonly from: string;
  readonly to: string;
  readonly module: M;
  readonly use: U;
}

/** The directory that holds a package file. */
export function directoryOf(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

/**
 * Each module's folder (spec/lang/10-modules.md#folders): the directory that
 * holds it (module.folder.holder), except that a file `x.hd` whose directory
 * `x/` beside it holds its child modules is in folder `x/`, as `x/mod.hd`
 * would be (module.folder.parent-file).
 */
export function folders<M extends FolderModule>(modules: Iterable<M>): (module: M) => string {
  // Each parent file that has a child module, as `src/shop.hd` for
  // `src/shop/item.hd` or `src/shop/item/mod.hd`.
  const parents = new Set<string>();
  for (const { path, identity } of modules) {
    if (!identity.includes(".")) continue;
    const directory = directoryOf(path.endsWith("/mod.hd") ? directoryOf(path) : path);
    parents.add(`${directory}.hd`);
  }
  return ({ path }) => (parents.has(path) ? path.slice(0, -".hd".length) : directoryOf(path));
}

// One shortest loop per strongly connected component of the folder graph
// (spec/lang/10-modules.md#cycle-diagnostic). Each edge is carried by the first use
// that makes it; `tangle` is the component's size.
export function folderLoops<M extends FolderModule, U extends FolderUse<M>>(
  uses: readonly { module: M; use: U }[],
  folderOf: (module: M) => string,
): (FolderEdge<M, U>[] & { tangle: number })[] {
  const out = new Map<string, Map<string, FolderEdge<M, U>>>();
  for (const { module, use } of uses) {
    const from = folderOf(module);
    const to = folderOf(use.target);
    if (from === to) continue;
    const targets = out.get(from) ?? new Map<string, FolderEdge<M, U>>();
    out.set(from, targets);
    if (!targets.has(to)) targets.set(to, { from, to, module, use });
  }
  const successors = (folder: string): FolderEdge<M, U>[] =>
    [...(out.get(folder)?.values() ?? [])].sort((left, right) => (left.to < right.to ? -1 : 1));
  const folders = [
    ...new Set([...out.keys(), ...[...out.values()].flatMap((targets) => [...targets.keys()])]),
  ].sort();
  const loops: (FolderEdge<M, U>[] & { tangle: number })[] = [];
  for (const component of stronglyConnected(folders, (folder) =>
    successors(folder).map(({ to }) => to),
  )) {
    if (component.length < 2) continue;
    const inside = new Set(component);
    let best: FolderEdge<M, U>[] | undefined;
    for (const start of [...component].sort()) {
      // Breadth-first search back to `start`, staying inside the component.
      const previous = new Map<string, FolderEdge<M, U>>();
      const queue = [start];
      let closing: FolderEdge<M, U> | undefined;
      for (let index = 0; index < queue.length && !closing; index++) {
        for (const edge of successors(queue[index]!)) {
          if (!inside.has(edge.to)) continue;
          if (edge.to === start) {
            closing = edge;
            break;
          }
          if (previous.has(edge.to)) continue;
          previous.set(edge.to, edge);
          queue.push(edge.to);
        }
      }
      if (!closing) continue;
      const path = [closing];
      for (let edge = previous.get(closing.from); edge; edge = previous.get(edge.from))
        path.unshift(edge);
      if (!best || path.length < best.length) best = path;
    }
    if (best) loops.push(Object.assign(best, { tangle: component.length }));
  }
  return loops;
}

// Tarjan's algorithm: the strongly connected components of a graph.
export function stronglyConnected<T>(nodes: readonly T[], next: (node: T) => readonly T[]): T[][] {
  const index = new Map<T, number>();
  const low = new Map<T, number>();
  const stack: T[] = [];
  const onStack = new Set<T>();
  const components: T[][] = [];
  const connect = (node: T): void => {
    index.set(node, index.size);
    low.set(node, index.get(node)!);
    stack.push(node);
    onStack.add(node);
    for (const target of next(node)) {
      if (!index.has(target)) {
        connect(target);
        low.set(node, Math.min(low.get(node)!, low.get(target)!));
      } else if (onStack.has(target)) low.set(node, Math.min(low.get(node)!, index.get(target)!));
    }
    if (low.get(node) !== index.get(node)) return;
    const component: T[] = [];
    for (;;) {
      const member = stack.pop()!;
      onStack.delete(member);
      component.push(member);
      if (member === node) break;
    }
    components.push(component);
  };
  for (const node of nodes) if (!index.has(node)) connect(node);
  return components;
}
