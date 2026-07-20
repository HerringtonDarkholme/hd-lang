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
- Error and effect signatures
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

## 10. Effects

- Basic effect signatures
- Dependency injection as effects
- User-defined effects
- Handlers
- Effect polymorphism
- Removing handled effects from an effect variable

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

## 13. Testing

- Dedicated named `test` blocks
- Unit tests discovered by the test runner
- `std.testing` assertions with mandatory reasons
- Property testing through `std.testing`, without special syntax
- Generated test data
- Mocking via effects

## 14. Capabilities and Sandbox

- Capability-based execution
- Default sandbox model
- Explicit access grants
- Safe agent scripting

## 15. Persistence and Resumption

- Interactive notebook-style resumption
- Durable workflow-style resumption
- Serializable closures
- Deterministic replay concerns

## 16. Observability

- Logging
- Tracing
- Metrics
- Observability as effects or standard library capabilities

## 17. Incremental Computation

- Cached computations
- Dependency tracking
- Partial recomputation
- Use in AI workflows and infra code

## 18. Data Retention

- Expressing ownership
- Retention policies such as `deleteWhen`
- Declarative lifecycle rules
- Keeping retention out of the core syntax

## 19. Compilation Targets

- JavaScript target
- WASM target
- Runtime assumptions
- Interop direction

## 20. Putting It Together

- Small AI tool example
- Effects, tools, and tests together
- Generated test data and mock behavior
- What the compiler can verify
