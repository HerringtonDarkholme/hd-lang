import type { HirExpression, HirPatternAccessStep } from "../hir.ts";
import { mutableInner, nominalGenericParts, readonlyType } from "../types.ts";

import { IteratorEmitter } from "./iterator.ts";

type DataExpression = Extract<HirExpression, { readonly kind: "data" }>;

/** Data construction and value embedding (08 Data Types and Enums). */
export abstract class DataEmitter extends IteratorEmitter {
  /** The data declaration index of an embedded field's type, `Box` for `Box[T]`. */
  protected embeddedDataIndex(type: string): number {
    const readonly = mutableInner(type) ?? type;
    return this.dataByName.get(nominalGenericParts(readonly)?.name ?? readonly)!.index;
  }

  /**
   * Module text for one copy function per embedded data type (08 Data
   * Embedding), each preceded by a blank line. A copy is a new object whose
   * ordinary fields hold the source's field values, copied shallowly, and
   * whose embedded fields hold copies of the source's parts. Generic fields
   * are copied in their erased representation, so one function serves every
   * instantiation.
   */
  emitEmbeddedCopies(): string {
    const declarations = [...this.dataByIndex.values()];
    const embedded = new Set(
      declarations.flatMap((declaration) =>
        declaration.fields
          .filter((field) => field.embedded)
          .map((field) => this.embeddedDataIndex(field.type)),
      ),
    );
    return declarations
      .filter((declaration) => embedded.has(declaration.index))
      .map((declaration) => {
        const name = `$d${declaration.index}`;
        const fields = declaration.fields.map((field) => {
          const read = `(struct.get ${name} ${name}f${field.index} (local.get $source))`;
          return field.embedded
            ? ` (call $hd.copy_d${this.embeddedDataIndex(field.type)} ${read})`
            : ` ${read}`;
        });
        const body =
          fields.length === 0 ? `(global.get ${name}c)` : `(struct.new ${name}${fields.join("")})`;
        return `\n\n  (func $hd.copy_d${declaration.index} (param $source (ref null ${name})) (result (ref null ${name}))\n    ${body})`;
      })
      .join("");
  }

  protected emitEmbeddedCopy(
    expression: Extract<HirExpression, { readonly kind: "embedded-copy" }>,
  ): string {
    return `(call $hd.copy_d${expression.dataIndex} ${this.emitExpression(expression.value)})`;
  }

  protected emitDataExpression(expression: DataExpression): string {
    const declaration = this.dataByIndex.get(expression.dataIndex)!;
    if (declaration.fields.length === 0) {
      const canonical = `(global.get $d${expression.dataIndex}c)`;
      return expression.spread
        ? `(block (result ${this.watType(expression.type)}) (drop ${this.emitExpression(expression.spread)}) ${canonical})`
        : canonical;
    }
    const spreadTemporary = expression.spread
      ? this.allocateTemporary(expression.spread.type)
      : undefined;
    const temporaries = expression.fields.map((field) => this.allocateTemporary(field.type));
    const sourceByField = new Map(
      expression.fieldIndices.map((fieldIndex, sourceIndex) => [fieldIndex, sourceIndex] as const),
    );
    // Parts supplied by the spread are copied when the spread is evaluated,
    // before any explicit field expression runs (08 Data Embedding).
    const spreadCopies = new Map(
      spreadTemporary
        ? declaration.fields
            .filter((field) => field.embedded && !sourceByField.has(field.index))
            .map(
              (field) => [field.index, this.allocateTemporary(readonlyType(field.type))] as const,
            )
        : [],
    );
    const storedFields = declaration.fields.map((field) => {
      const sourceIndex = sourceByField.get(field.index);
      if (sourceIndex === undefined) {
        if (!spreadTemporary)
          throw new Error(`data field '${field.name}' has no construction source`);
        const copied = spreadCopies.get(field.index);
        if (copied) return `(local.get ${copied})`;
        return `(struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${field.index} (local.get ${spreadTemporary}))`;
      }
      const value = `(local.get ${temporaries[sourceIndex]})`;
      return this.storeErased(
        value,
        expression.erasedFieldTypes?.[field.index],
        expression.fields[sourceIndex]!.type,
        expression.erasedTypeSubstitutions,
      );
    });
    return [
      `(block (result ${this.watType(expression.type)})`,
      ...(expression.spread
        ? [`  (local.set ${spreadTemporary} ${this.emitExpression(expression.spread)})`]
        : []),
      ...[...spreadCopies].map(
        ([fieldIndex, temporary]) =>
          `  (local.set ${temporary} (call $hd.copy_d${this.embeddedDataIndex(declaration.fields[fieldIndex]!.type)} (struct.get $d${expression.dataIndex} $d${expression.dataIndex}f${fieldIndex} (local.get ${spreadTemporary}))))`,
      ),
      ...expression.fields.map(
        (field, index) => `  (local.set ${temporaries[index]} ${this.boxVoid(field)})`,
      ),
      `  (struct.new $d${expression.dataIndex} ${storedFields.join(" ")})`,
      `)`,
    ].join("\n");
  }

  /** The value a match pattern reads: each step selects a field, payload, or tuple element. */
  protected emitPatternAccess(subject: string, path: readonly HirPatternAccessStep[]): string {
    return path.reduce((value, step) => {
      if (step.kind === "tuple")
        return this.unboxValue(
          `(array.get $hd.list (ref.as_non_null ${value}) (i32.const ${step.fieldIndex}))`,
          step.valueType,
        );
      if (step.kind === "erased-variant") {
        return this.unboxValue(
          `(struct.get $hd.variant $hd.variant-payload ${value})`,
          step.valueType,
        );
      }
      const prefix = step.kind === "data" ? `$d${step.typeIndex}` : `$e${step.typeIndex}`;
      const raw = `(struct.get ${prefix} ${prefix}f${step.fieldIndex} ${value})`;
      return this.loadErased(
        raw,
        step.erasedFieldType,
        step.valueType,
        step.erasedTypeSubstitutions,
      );
    }, `(local.get ${subject})`);
  }
}
