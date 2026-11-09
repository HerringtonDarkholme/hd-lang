type Tree =
  | { tag: "Leaf"; fields: Record<string, never> }
  | { tag: "Node"; fields: { kids: Tree[] } };

// A tree that holds itself: its encoder meets it again on its path.
export default {
  grow(): Tree {
    const kids: Tree[] = [];
    const tree: Tree = { tag: "Node", fields: { kids } };
    kids.push(tree);
    return tree;
  },
};
