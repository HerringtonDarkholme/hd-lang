import type { Expression } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirDataField } from "../hir.ts";
import { displayType, nominalGenericParts, readonlyType } from "../types.ts";
import type { InherentMethod } from "./context.ts";
import { GadtChecker } from "./gadt-checker.ts";
import { registeredPackageOwnership } from "./package-ownership.ts";

/**
 * Module visibility at a use: which members a module sees and which traits
 * are available there (spec/lang/10-modules.md#public-uses-and-visibility,
 * spec/lang/09-traits.md#trait-availability). Each check takes the use's
 * span, which names its module, since a joined program's top level is one
 * declaration.
 */
export abstract class MemberVisibilityChecker extends GadtChecker {
  /**
   * Spec 09 Trait Availability: a trait is available at `here`, the call,
   * when it is declared in or imported into the calling module, or supplied
   * by the prelude (09-traits.md#r-trait.avail.module). Another module's
   * trait, of this package or another, is available only where a use
   * imports it (checker/package-ownership.ts). The prototype still treats
   * every std trait as available.
   */
  protected traitAvailable(name: string, here: SourceSpan): boolean {
    const ownership = registeredPackageOwnership(this.traitTypes);
    if (!ownership || this.declaration.standard === true) return true;
    const trait = this.traitTypes.get(name);
    if (!trait || trait.standardName !== undefined) return true;
    return ownership.sameModule(trait.span, here) || ownership.imports(here, trait.name);
  }

  /**
   * Spec 03 Member Resolution: an own field or inherent method is visible at
   * `here` when it is declared in the calling module or marked `pub`
   * (10-modules.md#r-module.vis.members). A `lib/std` type's private member
   * is hidden from code outside std (08-data-and-enums.md#field-visibility),
   * and another module's from code outside that module, in the same package
   * or not (checker/package-ownership.ts).
   */
  protected memberVisible(
    member: HirDataField | InherentMethod,
    here: SourceSpan,
    owner?: HirData,
  ): boolean {
    if (
      member.public === true ||
      this.declaration.standard === true ||
      this.declaration.privateAccess === true ||
      this.compilerPrivateMember
    )
      return true;
    if (owner?.standard === true) return false;
    const ownership = registeredPackageOwnership(this.traitTypes);
    if (!ownership) return true;
    let declared = owner?.span;
    if (!owner) {
      // A std type's methods keep their own rule; another type's private
      // method is visible in the module whose `impl` declares it.
      const target = nominalGenericParts(readonlyType((member as InherentMethod).targetType));
      const name = target?.name ?? readonlyType((member as InherentMethod).targetType);
      const data = this.dataTypes.get(name);
      const enumType = this.enumTypes.get(name);
      if (data?.standard === true || (!data && enumType?.standardName !== undefined)) return true;
      if (!data && !enumType) return true;
      declared = (member as InherentMethod).span;
    }
    return ownership.sameModule(declared!, here);
  }

  /**
   * Whether a data literal or a data pattern at `here` may name `field` of
   * `owner` (08-data-and-enums.md#r-data.vis.private-fields). A newtype's
   * single field belongs to its constructor syntax, never a source name. A
   * `lib/std` type keeps its old rule here: the checker's derivations build
   * std structure values in the deriving module.
   */
  protected literalFieldVisible(field: HirDataField, here: SourceSpan, owner: HirData): boolean {
    return (
      owner.standard === true || owner.newtype === true || this.memberVisible(field, here, owner)
    );
  }

  /**
   * In another module, a data literal or copy-update names no private field
   * and builds only a type whose fields are all public
   * (08-data-and-enums.md#r-data.vis.literal,
   * 08-data-and-enums.md#r-data.vis.private-fields).
   */
  protected requireLiteralFieldsVisible(
    declaration: HirData,
    expression: Extract<Expression, { kind: "data" }>,
  ): void {
    for (const entry of expression.fields) {
      const field = declaration.fields.find((candidate) => candidate.name === entry.name);
      if (field && !this.literalFieldVisible(field, entry.span, declaration))
        this.fail(
          "private-member",
          `field '${entry.name}' is not visible from this module`,
          entry.span,
        );
    }
    const hidden = declaration.fields.find(
      (field) => !this.literalFieldVisible(field, expression.span, declaration),
    );
    if (hidden)
      this.fail(
        "private-member",
        `data type '${displayType(declaration.name)}' has the private field '${hidden.name}', so only its own module can build it with a data literal; call a function of that module instead`,
        expression.span,
      );
  }

  /** An inherent member named at `span` through its type, as `Cart::open`. */
  protected requireInherentVisible(member: InherentMethod, span: SourceSpan): void {
    if (!this.memberVisible(member, span))
      this.fail(
        "private-member",
        `${member.associated ? "associated function" : "method"} '${member.name}' is not visible from this module`,
        span,
      );
  }
}
