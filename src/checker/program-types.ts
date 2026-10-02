import { listVararg, type TraitDecl } from "../ast.ts";
import { extendsInspectable, usesStandardInspect } from "./inspectable.ts";
import { INSPECTABLE_MEMBERS } from "./standard-traits.ts";
import type { HirAssociatedBinding, HirData, HirTrait } from "../hir.ts";
import { mutableInner, nominalGenericParts } from "../types.ts";
import { PRELUDE_NAMES } from "./context.ts";
import {
  collectRowParameterReferences,
  firstPrivateSignatureType,
  resolveGenericType,
  typeName,
} from "./shared.ts";

import type { ProgramCheckContext } from "./program-context.ts";

export function declareProgramTypes(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  program.data.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
        span: declaration.span,
      });
      return;
    }
    if (dataTypes.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    // A parameter used after `$` in a field type is a requirement row.
    const rows = new Set<string>();
    for (const field of declaration.fields)
      collectRowParameterReferences(field.type.name, new Set(declaration.genericParameters), rows);
    dataTypes.set(declaration.name, {
      name: declaration.name,
      index,
      genericParameters: declaration.genericParameters,
      ...(rows.size > 0
        ? { rowParameters: declaration.genericParameters.filter((name) => rows.has(name)) }
        : {}),
      fields: [],
      ...(declaration.newtype ? { newtype: true as const } : {}),
      ...(declaration.local ? { local: true as const } : {}),
      ...(declaration.standard ? { standard: true as const } : {}),
      ...(declaration.standardName ? { standardName: declaration.standardName } : {}),
      ...(declaration.variances ? { variances: declaration.variances } : {}),
      span: declaration.span,
    });
  });
  program.enums.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
        span: declaration.span,
      });
      return;
    }
    if (dataTypes.has(declaration.name) || enumTypes.has(declaration.name)) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    enumTypes.set(declaration.name, {
      name: declaration.name,
      index,
      genericParameters: declaration.genericParameters,
      sharedFields: [],
      variants: [],
      fields: [],
      ...(declaration.local ? { local: true as const } : {}),
      ...(declaration.variances ? { variances: declaration.variances } : {}),
      span: declaration.span,
    });
  });
  program.traits.forEach((declaration, index) => {
    if (PRELUDE_NAMES.has(declaration.name) && !declaration.standard) {
      diagnostics.push({
        code: "prelude-name-shadow",
        message: `type '${declaration.name}' is provided by the prelude and cannot be redeclared`,
        span: declaration.span,
      });
      return;
    }
    if (
      dataTypes.has(declaration.name) ||
      enumTypes.has(declaration.name) ||
      traitTypes.has(declaration.name)
    ) {
      diagnostics.push({
        code: "duplicate-type",
        message: `type '${declaration.name}' is already declared`,
        span: declaration.span,
      });
      return;
    }
    traitTypes.set(declaration.name, {
      name: declaration.name,
      ...(declaration.standardName ? { standardName: declaration.standardName } : {}),
      index,
      genericParameters: declaration.genericParameters,
      supertraits: [],
      associatedTypes: [],
      methods: [],
      span: declaration.span,
    });
  });
  // `std.task.Waker`, a prelude name whose module has no `lib/std` file.
  traitTypes.set("Waker", {
    name: "Waker",
    index: program.traits.length,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [
      {
        name: "wake",
        index: 0,
        associated: false,
        genericParameters: [],
        suspending: false,
        receiverMutable: false,
        parameters: [],
        parameterNames: [],
        variadic: false,
        result: "void",
        requirements: [],
        span: program.span,
      },
    ],
    span: program.span,
  });
  // `Any`, the built-in universal empty trait of `std.core`: every value type
  // implements it (04-type-system.md#trait-values-and-any).
  traitTypes.set("Any", {
    name: "Any",
    index: program.traits.length + 1,
    genericParameters: [],
    supertraits: [],
    associatedTypes: [],
    methods: [],
    span: program.span,
  });
}

export function defineProgramData(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.data) {
    const data = dataTypes.get(declaration.name);
    if (!data) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const names = new Set<string>();
    const fields = declaration.fields.map((field, index) => {
      if (names.has(field.name))
        diagnostics.push({
          code: field.embedded ? "duplicate-embedded-field" : "duplicate-data-field",
          message: `field '${field.name}' is declared more than once`,
          span: field.span,
        });
      names.add(field.name);
      const rowParameters = new Set(data.rowParameters ?? []);
      // A parameter bounded by `Eq` and `Hash` may key a map (trait.hash.map-key).
      const hashable = new Set(
        declaration.genericParameters.filter((name) =>
          ["Eq", "Hash"].every((trait) =>
            (declaration.genericBounds ?? []).some(
              (bound) => bound.parameter === name && bound.traits.includes(trait),
            ),
          ),
        ),
      );
      const type =
        typeName(
          field.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          new Set(declaration.genericParameters.filter((name) => !rowParameters.has(name))),
          rowParameters,
          hashable,
        ) ?? "void";
      if (type === "void")
        diagnostics.push({
          code: "void-data-field",
          message: "a data field cannot have type void",
          span: field.span,
        });
      // An embedded field names a data type (08-data-and-enums.md#r-data.embed.data-only).
      const embeddedData = field.embedded
        ? dataTypes.get(nominalGenericParts(type)?.name ?? type)
        : undefined;
      if (field.embedded && type !== "void" && (!embeddedData || embeddedData.newtype))
        diagnostics.push({
          code: "embedded-non-data",
          message: `an embedded field must name a data type, not '${field.type.name}'`,
          span: field.span,
        });
      // An embedded field is always public (08-data-and-enums.md#data-declarations).
      const leaked =
        declaration.public && field.embedded ? firstPrivateSignatureType(type, program) : undefined;
      if (leaked)
        diagnostics.push({
          code: "private-type-leak",
          message: `public data '${declaration.name}' embeds private type '${leaked}'; embedded fields are always public`,
          span: field.span,
        });
      return {
        ...(field.public || field.embedded ? { public: true } : {}),
        name: field.name,
        type,
        index,
        embedded: field.embedded,
        defaultFunctionName: field.default
          ? `$default.${declaration.name}.${field.name}`
          : undefined,
        span: field.span,
      };
    });
    const defaults = Object.entries(declaration.genericDefaults ?? {}).map(
      ([name, type]) =>
        [
          name,
          typeName(
            type,
            dataTypes,
            enumTypes,
            traitTypes,
            diagnostics,
            new Set(declaration.genericParameters),
          ) ?? "void",
        ] as const,
    );
    dataTypes.set(declaration.name, {
      ...data,
      fields,
      ...(defaults.length > 0 ? { genericDefaults: new Map(defaults) } : {}),
    });
  }
}

export function defineProgramEnums(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.enums) {
    const enumType = enumTypes.get(declaration.name);
    if (!enumType) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const variantNames = new Set<string>();
    const allFields: HirData["fields"][number][] = [];
    const sharedNames = new Set<string>();
    // A named payload member must not repeat a named shared field; unnamed
    // ones never clash (08-data-and-enums.md#r-data.shared.payload-names).
    const sharedNamed = new Set<string>();
    const sharedFields = declaration.sharedFields.map((field) => {
      if (!field.positional) sharedNamed.add(field.name);
      if (sharedNames.has(field.name))
        diagnostics.push({
          code: "duplicate-data-field",
          message: `shared enum field '${field.name}' is declared more than once`,
          span: field.span,
        });
      sharedNames.add(field.name);
      const type =
        typeName(
          field.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          new Set(declaration.genericParameters),
        ) ?? "void";
      if (type === "void")
        diagnostics.push({
          code: "void-data-field",
          message: "a shared enum field cannot have type void",
          span: field.span,
        });
      const checked = {
        name: field.name,
        type,
        index: allFields.length,
        defaultFunctionName: field.default
          ? `$enum-default.${declaration.name}.${field.name}`
          : undefined,
        span: field.span,
      };
      allFields.push(checked);
      return checked;
    });
    const variants = declaration.variants.map((variant, tag) => {
      if (variantNames.has(variant.name))
        diagnostics.push({
          code: "duplicate-variant",
          message: `variant '${variant.name}' is declared more than once`,
          span: variant.span,
        });
      variantNames.add(variant.name);
      const fieldNames = new Set<string>();
      const fields = variant.fields.map((field) => {
        if (!field.positional && sharedNamed.has(field.name))
          diagnostics.push({
            code: "duplicate-data-field",
            message: `payload field '${field.name}' duplicates a shared enum field`,
            span: field.span,
          });
        if (fieldNames.has(field.name))
          diagnostics.push({
            code: "duplicate-data-field",
            message: `payload field '${field.name}' is declared more than once`,
            span: field.span,
          });
        fieldNames.add(field.name);
        const type =
          typeName(
            field.type,
            dataTypes,
            enumTypes,
            traitTypes,
            diagnostics,
            new Set(declaration.genericParameters),
          ) ?? "void";
        if (type === "void")
          diagnostics.push({
            code: "void-data-field",
            message: "an enum payload cannot have type void",
            span: field.span,
          });
        const checked = {
          name: field.name,
          type,
          index: allFields.length,
          span: field.span,
        };
        allFields.push(checked);
        return checked;
      });
      return {
        name: variant.name,
        tag,
        fields,
        factoryFunctionName:
          sharedFields.length > 0 ? `$enum-variant.${declaration.name}.${variant.name}` : undefined,
        span: variant.span,
      };
    });
    enumTypes.set(declaration.name, {
      ...enumType,
      sharedFields,
      variants,
      fields: allFields,
    });
  }
}

export function defineProgramTraits(context: ProgramCheckContext): void {
  const { program, diagnostics, dataTypes, enumTypes, traitTypes } = context;
  for (const declaration of program.traits) {
    const trait = traitTypes.get(declaration.name);
    if (!trait) continue;
    for (const parameter of declaration.genericParameters) {
      if (PRELUDE_NAMES.has(parameter))
        diagnostics.push({
          code: "prelude-name-shadow",
          message: `generic parameter '${parameter}' shadows a prelude name`,
          span: declaration.span,
        });
    }
    const supertraitNames = new Set<string>();
    // A supertrait's arguments and bindings may name `Self`
    // (09-traits.md#supertrait-bindings).
    const supertraitGenerics = new Set([...declaration.genericParameters, "Self"]);
    const supertraits = declaration.supertraits.flatMap((reference) => {
      const resolved = resolveGenericType(reference.name, supertraitGenerics);
      const application = nominalGenericParts(resolved);
      const name = application?.name ?? resolved;
      // `AnyVal` and `AnyRef` are value categories, not dispatched traits.
      if (name === "AnyVal" || name === "AnyRef") return [];
      const supertrait = traitTypes.get(name);
      if (!supertrait) {
        diagnostics.push({
          code: "unknown-trait",
          message: `unknown supertrait '${reference.name}'`,
          span: reference.span,
        });
        return [];
      }
      if (supertraitNames.has(resolved)) {
        diagnostics.push({
          code: "duplicate-supertrait",
          message: `supertrait '${reference.name}' is listed more than once`,
          span: reference.span,
        });
        return [];
      }
      supertraitNames.add(resolved);
      const traitArguments = application?.arguments ?? [];
      if (traitArguments.length !== supertrait.genericParameters.length) {
        diagnostics.push({
          code: "generic-arity",
          message: `trait '${supertrait.name}' expects ${supertrait.genericParameters.length} type arguments`,
          span: reference.span,
        });
        return [];
      }
      const associatedBindings: HirAssociatedBinding[] = [];
      const declared = program.traits[supertrait.index];
      for (const binding of declaration.supertraitBindings ?? []) {
        if (binding.trait !== reference.name) continue;
        // The supertrait's own supertraits may not be defined yet, so the
        // names it reaches come from the declarations (trait.binding.super.names).
        const declaring =
          declared?.name === supertrait.name
            ? declaringTraits(program.traits, declared, binding.name)
            : [];
        if (declaring.length !== 1) {
          diagnostics.push({
            code: declaring.length === 0 ? "unknown-associated-type" : "ambiguous-associated-type",
            message:
              declaring.length === 0
                ? `trait '${supertrait.name}' declares or reaches no associated type '${binding.name}'`
                : `'${binding.name}' of '${supertrait.name}' is ambiguous: ${declaring.join(" and ")} each declare it`,
            span: binding.span,
          });
          continue;
        }
        if (associatedBindings.some((existing) => existing.name === binding.name)) {
          diagnostics.push({
            code: "duplicate-associated-binding",
            message: `associated type '${binding.name}' of '${supertrait.name}' is bound more than once`,
            span: binding.span,
          });
          continue;
        }
        associatedBindings.push({
          name: binding.name,
          type: resolveGenericType(binding.type.name, supertraitGenerics),
        });
      }
      return [
        {
          traitIndex: supertrait.index,
          traitName: supertrait.name,
          traitArguments,
          ...(associatedBindings.length > 0 ? { associatedBindings } : {}),
        },
      ];
    });
    const names = new Set<string>();
    const associatedTypes = declaration.associatedTypes.map((associated, index) => {
      if (names.has(associated.name))
        diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait member '${associated.name}' is declared more than once`,
          span: associated.span,
        });
      names.add(associated.name);
      return { name: associated.name, index, span: associated.span };
    });
    const methods = declaration.methods.map((method, index) => {
      if (names.has(method.name))
        diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait member '${method.name}' is declared more than once`,
          span: method.span,
        });
      names.add(method.name);
      const associated = method.parameters[0]?.name !== "self";
      const memberGenerics = new Set([
        ...declaration.genericParameters,
        ...method.genericParameters,
        "Self",
        ...associatedTypes.map((associated) => `Self::${associated.name}`),
      ]);
      const sourceParameters = associated ? method.parameters : method.parameters.slice(1);
      sourceParameters.forEach((parameter, parameterIndex) => {
        if (parameter.variadic && parameterIndex !== sourceParameters.length - 1) {
          diagnostics.push({
            code: "nonfinal-vararg",
            message: `variadic parameter '${parameter.name}' must be the final parameter`,
            span: parameter.span,
          });
        }
      });
      const parameters = sourceParameters.map((parameter) => {
        const type = typeName(
          parameter.type,
          dataTypes,
          enumTypes,
          traitTypes,
          diagnostics,
          memberGenerics,
        );
        return type;
      });
      const result =
        typeName(method.result, dataTypes, enumTypes, traitTypes, diagnostics, memberGenerics) ??
        "void";
      const referenceParameters: string[] = [];
      const valueParameters: string[] = [];
      const genericBounds = method.genericBounds.flatMap((bound) =>
        bound.traits.flatMap((sourceTraitName) => {
          const mutable = mutableInner(sourceTraitName) !== undefined;
          const traitKey = mutableInner(sourceTraitName) ?? sourceTraitName;
          const application = nominalGenericParts(traitKey);
          const traitName = application?.name ?? traitKey;
          if (traitName === "AnyRef") {
            referenceParameters.push(bound.parameter);
            return [];
          }
          if (traitName === "AnyVal") {
            valueParameters.push(bound.parameter);
            return [];
          }
          const boundTrait = traitName === "Any" ? undefined : traitTypes.get(traitName);
          if (!boundTrait) return [];
          const traitArguments = (application?.arguments ?? []).map((argument) =>
            resolveGenericType(argument, memberGenerics, new Set()),
          );
          return [
            {
              parameter: bound.parameter,
              traitName: boundTrait.name,
              traitIndex: boundTrait.index,
              traitArguments,
              mutable,
            },
          ];
        }),
      );
      const defaults = Object.entries(method.genericDefaults ?? {}).map(
        ([name, type]) =>
          [
            name,
            typeName(type, dataTypes, enumTypes, traitTypes, diagnostics, memberGenerics) ?? "void",
          ] as const,
      );
      return {
        name: method.name,
        index,
        associated,
        genericParameters: method.genericParameters,
        genericBounds,
        ...(defaults.length > 0 ? { genericDefaults: new Map(defaults) } : {}),
        referenceParameters,
        valueParameters,
        suspending: method.suspending,
        receiverMutable: method.parameters[0]?.type.name === "mut:Self",
        parameters: parameters.map((parameter) => parameter ?? "void"),
        parameterNames: sourceParameters.map((parameter) => parameter.name),
        variadic: listVararg(sourceParameters.at(-1)),
        result,
        requirements: method.requirements,
        span: method.span,
      };
    });
    traitTypes.set(declaration.name, {
      ...trait,
      supertraits,
      associatedTypes,
      methods,
    });
  }
  diagnoseSupertraitCycles(context);
  diagnoseSupertraitMemberNames(context);
}

// 09 Trait Declarations: a child trait must not declare a member name of any
// transitive supertrait. Reported on the child's member.
function diagnoseSupertraitMemberNames(context: ProgramCheckContext): void {
  const traitsByIndex = new Map<number, HirTrait>();
  for (const trait of context.traitTypes.values()) traitsByIndex.set(trait.index, trait);
  const inheritedNames = (trait: HirTrait): Map<string, string> => {
    const names = new Map<string, string>();
    const seen = new Set<number>([trait.index]);
    const pending = trait.supertraits.map((supertrait) => supertrait.traitIndex);
    while (pending.length > 0) {
      const index = pending.pop()!;
      if (seen.has(index)) continue;
      seen.add(index);
      const supertrait = traitsByIndex.get(index);
      if (!supertrait) continue;
      for (const member of [...supertrait.associatedTypes, ...supertrait.methods])
        if (!names.has(member.name)) names.set(member.name, supertrait.name);
      pending.push(...supertrait.supertraits.map((parent) => parent.traitIndex));
    }
    return names;
  };
  const sealed = usesStandardInspect(context.imports);
  for (const declaration of context.program.traits) {
    const trait = context.traitTypes.get(declaration.name);
    if (!trait || trait.supertraits.length === 0) continue;
    const inherited = inheritedNames(trait);
    const inspectable = sealed && extendsInspectable(context.traitTypes, trait.name);
    for (const member of [...declaration.associatedTypes, ...declaration.methods]) {
      // 09 Sealed Traits: redeclaring a member of the sealed Inspectable.
      if (inspectable && INSPECTABLE_MEMBERS.has(member.name)) {
        context.diagnostics.push({
          code: "sealed-trait-implementation",
          message: `trait '${trait.name}' redeclares '${member.name}', a member of the sealed Inspectable`,
          span: member.span,
        });
        continue;
      }
      const owner = inherited.get(member.name);
      if (owner)
        context.diagnostics.push({
          code: "duplicate-trait-member",
          message: `trait '${trait.name}' declares '${member.name}', which its supertrait '${owner}' already declares`,
          span: member.span,
        });
    }
  }
}

function diagnoseSupertraitCycles(context: ProgramCheckContext): void {
  // Each cycle is reported once, on its member that appears first. A trait is
  // that member when it reaches itself through traits declared after it.
  const order = new Map<number, number>();
  const traitsByIndex = new Map<number, HirTrait>();
  context.program.traits.forEach((declaration, position) => {
    const trait = context.traitTypes.get(declaration.name);
    if (!trait) return;
    order.set(trait.index, position);
    traitsByIndex.set(trait.index, trait);
  });
  const reaches = (current: number, target: number, floor: number, seen: Set<number>): boolean => {
    if (current === target) return true;
    if ((order.get(current) ?? -1) <= floor || seen.has(current)) return false;
    seen.add(current);
    return (
      traitsByIndex
        .get(current)
        ?.supertraits.some((supertrait) => reaches(supertrait.traitIndex, target, floor, seen)) ??
      false
    );
  };
  context.program.traits.forEach((declaration, position) => {
    const trait = context.traitTypes.get(declaration.name);
    if (
      trait?.supertraits.some((supertrait) =>
        reaches(supertrait.traitIndex, trait.index, position, new Set()),
      )
    )
      context.diagnostics.push({
        code: "supertrait-cycle",
        message: `trait '${trait.name}' participates in a supertrait cycle`,
        span: declaration.span,
      });
  });
}

/** The traits among `trait` and its declared supertraits that declare the associated type `name`. */
function declaringTraits(traits: readonly TraitDecl[], trait: TraitDecl, name: string): string[] {
  const seen = new Set<string>();
  const declaring: string[] = [];
  const visit = (current: TraitDecl): void => {
    if (seen.has(current.name)) return;
    seen.add(current.name);
    if (current.associatedTypes.some((associated) => associated.name === name))
      declaring.push(current.name);
    for (const supertrait of current.supertraits) {
      const head = nominalGenericParts(supertrait.name)?.name ?? supertrait.name;
      const parent = traits.find((candidate) => candidate.name === head);
      if (parent) visit(parent);
    }
  };
  visit(trait);
  return declaring;
}
