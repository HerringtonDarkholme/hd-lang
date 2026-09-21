# hd-lang in Y Minutes: Tour Outline

This tour should introduce hd-lang through small examples first, then explain the design intention behind each feature briefly.

## 1. Hello hd-lang

- Minimal program
- Python-style indentation
- Comments
- Running a script

## 2. Values and Types

- Bindings
- Primitive types
- Arithmetic and bitwise operators
- Swift-style optionals, such as `string?`
- Collections
- List and map comprehensions

## 3. Control Flow and Expressions

- Expressions versus statements
- `:` after block headers and same-line bodies
- Blocks evaluate to their last expression
- `if` as an expression
- `match` as an expression
- `for` and `while` loops
- Loop `else` blocks and `break value`
- `return`, `break`, and `continue`

## 4. Structs

- Defining structs
- Struct literals
- No classes
- Struct embedding

## 5. Enums

- Algebraic sum types
- Variants with no data
- Variants with named payload fields
- Enum literals
- Exhaustive pattern matching
- `Option` and `Result` as standard enum-like types

## 6. Functions

- Function syntax
- Return types
- Closure expressions
- Lexical captures
- Named parameters, if supported
- Varargs and spread calls
- Error, requirement, and suspension signatures
- Function-first design

## 7. Traits and Methods

- Defining traits
- Implementing traits for structs
- Trait-based methods
- Generic constraints

## 8. Type System

- Static typing with local inference
- Explicit public boundaries
- Nominal structs and enums
- Structural tuples
- Transparent aliases and nominal newtypes
- Numeric widening and explicit narrowing casts
- Generic types and functions
- Erased function generics and explicit `reified` parameters
- Variadic generics
- Explicit trait implementations
- Trait values versus generic static dispatch
- No implicit nullability

## 9. Modules, Packages, and Imports

- Path-inferred modules
- Directory submodules
- `mod.hd` index files
- Importing names and modules
- Aliases
- Re-exports
- Visibility with `pub`

## 10. Requirements and Suspension

- Static requirement rows
- Dependency injection through provider contexts
- One-shot suspension with `fn!` and `Suspend[T]`
- Construction-time provider capture
- Provisional requirement polymorphism
- Removing locally provided requirements from a requirement variable

## 11. Using Annotations

- `annotate Target` member metadata blocks
- Homogeneous dynamic metadata trait lists
- `annotate Annotation for Target` derivation blocks
- `pass` for derivation without overrides
- Reusable metadata lists
- Bottom-up metadata generation
- Explicit runtime metadata retrieval
- Structural result overrides
- Manual registration of generated metadata

## 12. Implementing Annotators

- Runtime declaration shapes
- `Annotation.Info` and `Annotate[A]`
- `FieldMetadata[T]`, `VariantMetadata`, and `ParamMetadata[T]`
- Struct, enum, and function annotators
- Exact type annotation cases
- `AnnotationRef` for recursive metadata
- Tool metadata as an example of `FuncAnnotator`, not a separate language feature

## Companion Runtime and Library Document

The following topics are outside the core-language tour and belong in [Runtime and Library Design](RUNTIME_AND_LIBRARY.md):

- Testing and property testing
- Capabilities and sandbox enforcement
- WebAssembly, Wasm GC, and WASI runtime integration
- Persistence, resumption, and deterministic replay
- Observability
- Serializable closures
- Incremental computation
- Data-retention libraries
- Tool, RPC, and deployment adapters
