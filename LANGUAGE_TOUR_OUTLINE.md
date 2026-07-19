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

## 3. Type System

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

## 4. Modules, Packages, and Imports

- Path-inferred modules
- Directory submodules
- `mod.hd` index files
- Importing names and modules
- Aliases
- Re-exports
- Visibility with `pub`

## 5. Control Flow and Expressions

- Expressions versus statements
- `:` after block headers and same-line bodies
- Blocks evaluate to their last expression
- `if` as an expression
- `match` as an expression
- `for` and `while` loops
- Loop `else` blocks and `break value`
- `return`, `break`, and `continue`

## 6. Structs

- Defining structs
- Struct literals
- No classes
- Struct embedding

## 7. Enums

- Algebraic sum types
- Variants with no data
- Variants with named payload fields
- Enum literals
- Exhaustive pattern matching
- `Option` and `Result` as standard enum-like types

## 8. Functions

- Function syntax
- Return types
- Closure expressions
- Lexical captures
- Named parameters, if supported
- Varargs and spread calls
- Error and effect signatures
- Function-first design

## 9. Traits and Methods

- Defining traits
- Implementing traits for structs
- Trait-based methods
- Generic constraints

## 10. Effects

- Basic effect signatures
- Dependency injection as effects
- User-defined effects
- Handlers
- Effect polymorphism
- Removing handled effects from an effect variable

## 11. Testing

- Unit tests
- Property tests
- Generated test data
- Mocking via effects

## 12. Tool Calls for AI Agents

- Function-first tool definition
- Tool descriptions
- Context, auth, and access control
- Error effects
- Examples for tools
- References between tools

## 13. Capabilities and Sandbox

- Capability-based execution
- Default sandbox model
- Explicit access grants
- Safe agent scripting

## 14. Persistence and Resumption

- Interactive notebook-style resumption
- Durable workflow-style resumption
- Serializable closures
- Deterministic replay concerns

## 15. Observability

- Logging
- Tracing
- Metrics
- Observability as effects or standard library capabilities

## 16. Incremental Computation

- Cached computations
- Dependency tracking
- Partial recomputation
- Use in AI workflows and infra code

## 17. Declarative Annotations

- Common typed representation for structs, enums, and functions
- Structural generic derivation from declaration shapes
- Local override annotations, reusable override profiles, and full derivation rewrites
- Explicit runtime export of generated metadata/artifacts
- Database schema, UI, tool, retention, observability, and workflow facets

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
