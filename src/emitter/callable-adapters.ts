import {
  functionParts,
  nominalGenericParts,
  nominalGenericType,
  substituteTypeParameters,
} from "../types.ts";
import type { ValueType } from "../hir.ts";
import { DataEmitter } from "./data.ts";
import {
  containsGenericValueType,
  functionName,
  isGenericValueType,
  isRowRequirement,
  methodBoundParameters,
  traitSuspensionName,
} from "./shared.ts";

export const CALLABLE_STORAGE_TYPES = `    (type $hd.callable-storage-sig (func
      (param anyref)
      (param (ref null $hd.list))
      (param (ref null $hd.providers))
      (result anyref)))
    (type $hd.box-callable (struct
      (field $hd.box-callable-value anyref)
      (field $hd.box-callable-invoke (ref $hd.callable-storage-sig))))`;

/** Emits the closure thunks that cross concrete and erased callable ABIs. */
export abstract class CallableAdapterEmitter extends DataEmitter {
  private providerUnion(
    requirements: readonly string[],
    keys: readonly string[] = requirements,
  ): string {
    return requirements.reduceRight((parent, requirement, index) => {
      if (isRowRequirement(requirement))
        return `(call $hd.provider_concat (local.get $p${index}) ${parent})`;
      const key = keys[index]!;
      const type = this.traitsByName.has(nominalGenericParts(key)?.name ?? key)
        ? `trait:${key}`
        : `provider:${key}`;
      return `(struct.new $hd.providers (i32.const ${this.providerKey(key)}) ${this.boxProvider(`(local.get $p${index})`, type)} ${parent})`;
    }, `(ref.null $hd.providers)`);
  }

  private emitCallableStorageAdapter(type: ValueType, index: number): string {
    const callable = functionParts(type)!;
    const signature = this.functionSignatures.get(type)!;
    const closure = `(ref.cast (ref $closure${signature}) (local.get $value))`;
    const arguments_ = callable.parameters.map((parameter, parameterIndex) =>
      this.unboxValue(
        `(array.get $hd.list (ref.as_non_null (local.get $args)) (i32.const ${parameterIndex}))`,
        parameter,
      ),
    );
    const providers = callable.requirements.map((requirement) =>
      isRowRequirement(requirement)
        ? `(local.get $providers)`
        : this.unboxProvider(
            `(call $hd.provider_get (local.get $providers) (i32.const ${this.providerKey(requirement)}))`,
            requirement,
          ),
    );
    const call = `(call_ref $sig${signature} (struct.get $closure${signature} $closure${signature}env ${closure})${arguments_.length ? " " + arguments_.join(" ") : ""}${providers.length ? " " + providers.join(" ") : ""} (struct.get $closure${signature} $closure${signature}fn ${closure}))`;
    const storedResult = callable.suspending
      ? call
      : callable.result === "void"
        ? `(block (result anyref) ${call} (ref.null any))`
        : this.boxWatValue(call, callable.result);
    const store = [
      `(func $cstore${index} (type $hd.callable-storage-sig) (param $value anyref) (param $args (ref null $hd.list)) (param $providers (ref null $hd.providers)) (result anyref)`,
      `  ${storedResult}`,
      `)`,
    ].join("\n");

    const parameters = callable.parameters.map(
      (parameter, parameterIndex) =>
        `(param $a${parameterIndex} ${this.parameterWatType(parameter)})`,
    );
    const providerParameters = callable.requirements.map(
      (requirement, providerIndex) =>
        `(param $p${providerIndex} ${this.providerType(requirement)})`,
    );
    const packedArguments =
      callable.parameters.length === 0
        ? `(array.new_default $hd.list (i32.const 0))`
        : `(array.new_fixed $hd.list ${callable.parameters.length} ${callable.parameters
            .map((parameter, parameterIndex) =>
              this.boxWatValue(`(local.get $a${parameterIndex})`, parameter),
            )
            .join(" ")})`;
    const box = `(ref.cast (ref $hd.box-callable) (local.get $env))`;
    const invocation = `(call_ref $hd.callable-storage-sig (struct.get $hd.box-callable $hd.box-callable-value ${box}) ${packedArguments} ${this.providerUnion(callable.requirements)} (struct.get $hd.box-callable $hd.box-callable-invoke ${box}))`;
    const loadedResult = callable.suspending
      ? `(ref.cast (ref null $hd.suspension) ${invocation})`
      : callable.result === "void"
        ? `(drop ${invocation})`
        : this.unboxValue(invocation, callable.result);
    const result = callable.suspending
      ? ` (result (ref null $hd.suspension))`
      : callable.result === "void"
        ? ""
        : ` (result ${this.watType(callable.result)})`;
    const load = [
      `(func $cload${index} (type $sig${signature}) (param $env anyref) ${[...parameters, ...providerParameters].join(" ")}${result}`,
      `  ${loadedResult}`,
      `)`,
    ].join("\n");
    return `${store}\n\n${load}`;
  }

  private emitCallableStorageAdapters(): string {
    const emitted: string[] = [];
    for (let index = 0; index < this.storageAdapters.length; index += 1) {
      const adapter = this.storageAdapters[index]!;
      emitted.push(this.emitCallableStorageAdapter(adapter.type, adapter.index));
    }
    return emitted.join("\n\n");
  }

  emitCallableAdapters(): string {
    const callableAdapters = this.adapters
      .map((adapter) => {
        const formal = functionParts(adapter.formalType)!;
        const actual = functionParts(adapter.actualType)!;
        const formalSignature = this.functionSignatures.get(adapter.formalType);
        const actualSignature = this.functionSignatures.get(adapter.actualType);
        const parameters = formal.parameters.map(
          (parameter, index) => `(param $a${index} ${this.parameterWatType(parameter)})`,
        );
        const providers = formal.requirements.map(
          (requirement, index) => `(param $p${index} ${this.providerType(requirement)})`,
        );
        const result = formal.suspending
          ? ` (result (ref null $hd.suspension))`
          : formal.result === "void"
            ? ""
            : ` (result ${this.watType(formal.result)})`;
        const closure = `(ref.cast (ref $closure${actualSignature}) (local.get $env))`;
        const arguments_ = this.adaptedArguments(formal, actual);
        const substitutions = new Map(
          adapter.typeSubstitutions.map(({ parameter, type }) => [parameter, type] as const),
        );
        const instantiatedFormal = formal.requirements.map((requirement) =>
          substituteTypeParameters(requirement, substitutions),
        );
        const instantiatedActual = actual.requirements.map((requirement) =>
          substituteTypeParameters(requirement, substitutions),
        );
        const concreteFormal = new Map<string, number>();
        formal.requirements.forEach((requirement, index) => {
          const key = instantiatedFormal[index]!;
          if (!isRowRequirement(requirement) && !concreteFormal.has(key))
            concreteFormal.set(key, index);
        });
        const formalUnion = this.providerUnion(formal.requirements, instantiatedFormal);
        const actualProviders = actual.requirements.map((requirement, index) => {
          if (isRowRequirement(requirement)) return formalUnion;
          const key = instantiatedActual[index]!;
          const direct = concreteFormal.get(key);
          if (direct !== undefined) return `(local.get $p${direct})`;
          if (formal.requirements.length === 0)
            throw new Error(
              `cannot adapt requirement '${key}' from ${adapter.formalType} to ${adapter.actualType}`,
            );
          return this.unboxProvider(
            `(call $hd.provider_get ${formalUnion} (i32.const ${this.providerKey(key)}))`,
            key,
          );
        });
        const call = `(call_ref $sig${actualSignature} (struct.get $closure${actualSignature} $closure${actualSignature}env ${closure})${arguments_.length ? " " : ""}${arguments_.join(" ")}${actualProviders.length ? " " : ""}${actualProviders.join(" ")} (struct.get $closure${actualSignature} $closure${actualSignature}fn ${closure}))`;
        const body = formal.suspending
          ? call
          : isGenericValueType(formal.result) && !isGenericValueType(actual.result)
            ? this.boxWatValue(call, actual.result)
            : isGenericValueType(actual.result) && !isGenericValueType(formal.result)
              ? this.unboxValue(call, formal.result)
              : call;
        return `(func $adapt${adapter.index} (type $sig${formalSignature}) (param $env anyref) ${[...parameters, ...providers].join(" ")}${result}\n  ${body}\n)`;
      })
      .join("\n\n");
    const suspensionResultAdapters = this.resultAdapters
      .map(
        (adapter) =>
          `(func $sresultadapt${adapter.index} (type $hd.suspension-result-adapt-sig) (param $value anyref) (result anyref)\n  ${adapter.body}\n)`,
      )
      .join("\n\n");
    const storageAdapters = this.emitCallableStorageAdapters();
    return [callableAdapters, suspensionResultAdapters, storageAdapters]
      .filter(Boolean)
      .join("\n\n");
  }

  emitTraitAdapters(): string {
    return [...this.implementationsByIndex.values()]
      .filter((implementation) => !implementation.intrinsic)
      .flatMap((implementation) => {
        const trait = this.traitsByIndex.get(implementation.traitIndex)!;
        const traitSubstitutions = new Map([
          ...trait.genericParameters.map(
            (parameter, index) => [parameter, implementation.traitArguments[index]!] as const,
          ),
          ...trait.associatedTypes.map(
            (associated, index) =>
              [`Self::${associated.name}`, implementation.associatedTypes[index]!] as const,
          ),
          ["Self", implementation.targetType] as const,
        ]);
        const substitutions = [...traitSubstitutions].map(([parameter, type]) => ({
          parameter,
          type,
        }));
        return implementation.methodFunctions.flatMap((mapping) => {
          if (!this.methodIsLive(implementation.traitIndex, mapping.methodIndex)) return [];
          const method = trait.methods[mapping.methodIndex]!;
          const parameters = method.parameters.map(
            (parameter, index) => `(param $a${index} ${this.parameterWatType(parameter)})`,
          );
          const methodBounds = methodBoundParameters(method, "b");
          const providers = method.requirements.map(
            (requirement, index) => `(param $p${index} ${this.providerType(requirement)})`,
          );
          const result = method.result === "void" ? "" : ` (result ${this.watType(method.result)})`;
          const dictionary = `(ref.cast (ref $trait${trait.index}) (local.get $dictionary))`;
          const boundPack = `(ref.as_non_null (struct.get $trait${trait.index} $trait${trait.index}bounds ${dictionary}))`;
          const arguments_ = [
            ...(method.associated
              ? []
              : [this.unboxValue(`(local.get $self)`, implementation.targetType)]),
            ...method.parameters.map((parameter, index) =>
              parameter === "generic:Self"
                ? this.unboxValue(`(local.get $a${index})`, implementation.targetType)
                : containsGenericValueType(parameter)
                  ? this.loadErased(
                      `(local.get $a${index})`,
                      parameter,
                      substituteTypeParameters(parameter, traitSubstitutions),
                      substitutions,
                    )
                  : `(local.get $a${index})`,
            ),
            ...implementation.genericBounds.map((bound, index) =>
              this.unboxValue(
                `(array.get $hd.list ${boundPack} (i32.const ${index}))`,
                `trait:${
                  bound.traitArguments.length > 0
                    ? nominalGenericType(bound.traitName, bound.traitArguments)
                    : bound.traitName
                }`,
              ),
            ),
            ...methodBounds.map((_, index) => `(local.get $b${index})`),
            ...method.requirements.map((_, index) => `(local.get $p${index})`),
          ];
          if (mapping.strengthened)
            return [
              `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...methodBounds, ...providers].join(" ")}${result}\n  (unreachable)\n)`,
            ];
          if (!method.suspending) {
            const call = `(call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})`;
            const body = containsGenericValueType(method.result)
              ? this.storeErased(
                  call,
                  method.result,
                  substituteTypeParameters(method.result, traitSubstitutions),
                  substitutions,
                )
              : call;
            return [
              `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...methodBounds, ...providers].join(" ")}${result}\n  ${body}\n)`,
            ];
          }
          const wrapper = traitSuspensionName(trait.index, method.index);
          const frame = `$s${mapping.functionIndex}`;
          const constructor = `(func $tadapt${implementation.index}_${method.index} (type $tsig${trait.index}_${method.index}) (param $self anyref) (param $dictionary anyref) ${[...parameters, ...methodBounds, ...providers].join(" ")} (result (ref null ${wrapper}))\n  (struct.new ${wrapper}\n    (call ${functionName(mapping.functionIndex)} ${arguments_.join(" ")})\n    (ref.func $tspolladapt${implementation.index}_${method.index})\n    (ref.func $tscanceladapt${implementation.index}_${method.index})\n    (ref.func $tsresultadapt${implementation.index}_${method.index})\n    (ref.null $hd.suspension-result-adapt-sig))\n)`;
          const poll = `(func $tspolladapt${implementation.index}_${method.index} (type $tspollsig${trait.index}_${method.index}) (param $inner anyref) (result i32)\n  (call $poll${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
          const cancel = `(func $tscanceladapt${implementation.index}_${method.index} (type $tscancelsig${trait.index}_${method.index}) (param $inner anyref)\n  (call $cancel${mapping.functionIndex} (ref.cast (ref null ${frame}) (local.get $inner)))\n)`;
          const resultBody =
            method.result === "void"
              ? ""
              : `\n  ${
                  containsGenericValueType(method.result)
                    ? this.storeErased(
                        `(struct.get ${frame} ${frame}result (ref.cast (ref null ${frame}) (local.get $inner)))`,
                        method.result,
                        substituteTypeParameters(method.result, traitSubstitutions),
                        substitutions,
                      )
                    : `(struct.get ${frame} ${frame}result (ref.cast (ref null ${frame}) (local.get $inner)))`
                }`;
          const resultAdapter = `(func $tsresultadapt${implementation.index}_${method.index} (type $tsresultsig${trait.index}_${method.index}) (param $inner anyref)${result}${resultBody}\n)`;
          return [constructor, poll, cancel, resultAdapter];
        });
      })
      .join("\n\n");
  }
}
