import type { ValueType } from "./hir.ts";

export interface ResultParts {
  readonly ok: ValueType;
  readonly error: ValueType;
}

export interface FunctionParts {
  readonly parameters: readonly ValueType[];
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
}

export interface NominalGenericParts {
  readonly name: string;
  readonly arguments: readonly ValueType[];
}

export function mutableInner(type: ValueType): ValueType | undefined {
  return type.startsWith("mut:") ? type.slice("mut:".length) : undefined;
}

export function mutableType(type: ValueType): ValueType {
  return `mut:${type}`;
}

export function readonlyType(type: ValueType): ValueType {
  return mutableInner(type) ?? type;
}

export function tupleParts(type: ValueType): readonly ValueType[] | undefined {
  if (!type.startsWith("(") || !type.endsWith(")")) return undefined;
  const contents = type.slice(1, -1);
  if (contents === "") return [];
  const values: string[] = [];
  let depth = 0;
  let start = 0;
  let sawComma = false;
  for (let index = 0; index < contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if (character === "," && depth === 0) {
      sawComma = true;
      const value = contents.slice(start, index);
      if (value) values.push(value);
      start = index + 1;
    }
  }
  const final = contents.slice(start);
  if (final) values.push(final);
  return sawComma ? values : undefined;
}

export function tupleType(elements: readonly ValueType[]): ValueType {
  return `(${elements.join(",")}${elements.length === 1 ? "," : ""})`;
}

export function nominalGenericParts(type: ValueType): NominalGenericParts | undefined {
  const open = type.indexOf("[");
  if (open <= 0 || !type.endsWith("]")) return undefined;
  const name = type.slice(0, open);
  const contents = type.slice(open + 1, -1);
  const arguments_: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index <= contents.length; index += 1) {
    const character = contents[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "," || index === contents.length) && depth === 0) {
      const argument = contents.slice(start, index);
      if (argument) arguments_.push(argument);
      start = index + 1;
    }
  }
  return { name, arguments: arguments_ };
}

export function nominalGenericType(name: string, arguments_: readonly ValueType[]): ValueType {
  return `${name}[${arguments_.join(",")}]`;
}

export function optionalInner(type: ValueType): ValueType | undefined {
  return type.endsWith("?") ? type.slice(0, -1) : undefined;
}

export function resultParts(type: ValueType): ResultParts | undefined {
  if (!type.startsWith("Result[") || !type.endsWith("]")) return undefined;
  const contents = type.slice("Result[".length, -1);
  let depth = 0;
  for (let index = 0; index < contents.length; index += 1) {
    const character = contents[index];
    if (character === "[") depth += 1;
    else if (character === "]") depth -= 1;
    else if (character === "," && depth === 0) {
      return { ok: contents.slice(0, index), error: contents.slice(index + 1) };
    }
  }
  return undefined;
}

export function resultType(ok: ValueType, error: ValueType): ValueType {
  return `Result[${ok},${error}]`;
}

export function isErasedVariant(type: ValueType): boolean {
  return optionalInner(type) !== undefined || resultParts(type) !== undefined;
}

export function functionParts(type: ValueType): FunctionParts | undefined {
  if (!type.startsWith("fn(") || !type.includes(")->")) return undefined;
  let depth = 0;
  let close = -1;
  for (let index = 3; index < type.length; index += 1) {
    const character = type[index];
    if (character === "(" || character === "[") depth += 1;
    else if (character === "[") depth += 1;
    else if (character === "]") depth -= 1;
    else if (character === ")" && depth === 0) {
      close = index;
      break;
    } else if (character === ")") depth -= 1;
  }
  if (close < 0 || type.slice(close, close + 3) !== ")->") return undefined;
  const parameterText = type.slice(3, close);
  const renderedParameters: string[] = [];
  let start = 0;
  depth = 0;
  for (let index = 0; index <= parameterText.length; index += 1) {
    const character = parameterText[index];
    if (character === "[" || character === "(") depth += 1;
    else if (character === "]" || character === ")") depth -= 1;
    else if ((character === "," || index === parameterText.length) && depth === 0) {
      const parameter = parameterText.slice(start, index);
      if (parameter) renderedParameters.push(parameter);
      start = index + 1;
    }
  }
  const tail = type.slice(close + 3);
  let resultDepth = 0;
  let requirementStart = -1;
  for (let index = 0; index < tail.length; index += 1) {
    const character = tail[index];
    if (character === "[" || character === "(") resultDepth += 1;
    else if (character === "]" || character === ")") resultDepth -= 1;
    else if (character === "$" && resultDepth === 0) {
      requirementStart = index;
      break;
    }
  }
  const result = requirementStart < 0 ? tail : tail.slice(0, requirementStart);
  const requirements = requirementStart < 0
    ? []
    : tail.slice(requirementStart + 1).split("+").filter(Boolean);
  const variadic = renderedParameters.at(-1)?.endsWith("...") === true;
  const parameters = renderedParameters.map((parameter, index) => {
    if (!variadic || index !== renderedParameters.length - 1) return parameter;
    return nominalGenericType("list", [parameter.slice(0, -3)]);
  });
  return { parameters, variadic, result, requirements };
}

export function functionType(parameters: readonly ValueType[], result: ValueType, requirements: readonly string[] = [], variadic = false): ValueType {
  const row = [...new Set(requirements)].sort();
  const rendered = parameters.map((parameter, index) => {
    if (!variadic || index !== parameters.length - 1) return parameter;
    const nominal = nominalGenericParts(parameter);
    return `${nominal?.name === "list" && nominal.arguments.length === 1 ? nominal.arguments[0] : parameter}...`;
  });
  return `fn(${rendered.join(",")})->${result}${row.length ? `$${row.join("+")}` : ""}`;
}

export function contextKeys(type: ValueType): readonly string[] | undefined {
  return type.startsWith("context:") ? type.slice("context:".length).split("+").filter(Boolean) : undefined;
}

export function contextType(keys: readonly string[]): ValueType {
  return `context:${[...new Set(keys)].sort().join("+")}`;
}

export function suspensionType(functionIndex: number, result: ValueType): ValueType {
  return `suspend(${functionIndex}):${result}`;
}

export function suspensionParts(type: ValueType): { functionIndex: number; result: ValueType } | undefined {
  const match = /^suspend\((\d+)\):(.*)$/s.exec(type);
  return match ? { functionIndex: Number(match[1]), result: match[2]! } : undefined;
}

export function traitSuspensionType(traitIndex: number, methodIndex: number, result: ValueType): ValueType {
  return `trait-suspend(${traitIndex},${methodIndex}):${result}`;
}

export function traitSuspensionParts(type: ValueType): { traitIndex: number; methodIndex: number; result: ValueType } | undefined {
  const match = /^trait-suspend\((\d+),(\d+)\):(.*)$/s.exec(type);
  return match ? { traitIndex: Number(match[1]), methodIndex: Number(match[2]), result: match[3]! } : undefined;
}
