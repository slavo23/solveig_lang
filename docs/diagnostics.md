# Solveig Diagnostics

Solveig diagnostics are grouped by compiler phase:

- `PAR` parser diagnostics
- `RES` name resolution, imports, operators, and module graph diagnostics
- `KIND` kind inference diagnostics
- `TC` type checking, typeclass, exhaustiveness, and warning diagnostics

Runtime errors are currently rendered as `runtime error: ...` without a
diagnostic code. Project loading errors, such as a missing `src/main.sv`, are
also plain text today. [Errors, Defects, and Interruption](errors.md) explains
which program failures are typed values, which unwind as defects, and which
runtime failures are not catchable by program code.

## Parser

| Code | Meaning |
| --- | --- |
| `PAR0001` | Parse error. The parser could not understand the source at this location and recovered where possible. |

## Resolver

| Code | Meaning |
| --- | --- |
| `RES0001` | Ambiguous import. More than one imported module provides the same name. |
| `RES0002` | Unknown module. An import names a module that cannot be found. |
| `RES0003` | Unknown name. A term, constructor, type, class, or operator is not in scope. |
| `RES0004` | Duplicate definition. A module defines the same name twice in the same namespace. |
| `RES0005` | Class arity error. Classes and instances must have exactly one parameter. |
| `RES0006` | Signature arity error. A definition has more value parameters than its signature accepts. |
| `RES0007` | Cyclic imports. The module graph contains an import cycle. |
| `RES0008` | Fixity conflict. Same-precedence operators with incompatible associativity were mixed without parentheses. |
| `RES0009` | Illegal unary minus. Prefix `-` was used in a position that needs parentheses. |
| `RES0010` | Malformed operator expression. A flat operator sequence could not be resolved into an expression tree. |
| `RES0014` | Unknown selected import name. The source module does not expose that name to the importing package. |
| `RES0019` | Invalid module API directive, such as a missing entity, duplicate visibility declaration, invalid constructor exposure, or re-export collision. |
| `RES0020` | Ambiguous module qualifier. Two explicit imports claim the same short qualifier; give one an `as` alias. |
| `RES0021` | Orphan instance. The defining package owns neither the class nor the outer type constructor after aliases are expanded. |
| `RES0022` | Overlapping instances. Two instance heads in the visible package graph can match the same constraint. |

## Kinds

| Code | Meaning |
| --- | --- |
| `KIND0001` | Type variable not in scope. |
| `KIND0002` | Type constructor not in scope. |
| `KIND0003` | Kind occurs check failed; a recursive kind would be required. |
| `KIND0004` | Kind mismatch. A type was used at one kind where another kind was required. |
| `KIND0005` | Signature kind error. A value signature must have kind `*`. |

## Type Checking

| Code | Severity | Meaning |
| --- | --- | --- |
| `TC0001` | Error | Variable not in scope. |
| `TC0002` | Error | Unknown constructor in an expression. |
| `TC0003` | Error | Unknown constructor in a pattern. |
| `TC0004` | Error | Empty `case` expression. |
| `TC0005` | Error | Type mismatch. |
| `TC0006` | Error | Failed left-side existential instantiation during subtyping. |
| `TC0007` | Error | Failed right-side existential instantiation during subtyping. |
| `TC0008` | Error | Constructor was used as if it accepted more arguments than it can. |
| `TC0009` | Error | Application of a non-function. |
| `TC0010` | Error | Type application to a non-polymorphic expression. |
| `TC0011` | Error | Ill-formed type annotation. |
| `TC0012` | Error | Case-arm result typing state became inconsistent. |
| `TC0013` | Error | Unsatisfied typeclass constraint. For example, `"a" + "b"` needs `Num String`, and no such instance exists. |
| `TC0015` | Error | Instance method implementation does not match the class method type. |
| `TC0016` | Error | Non-exhaustive pattern match. |
| `TC0017` | Warning | Unreachable pattern. Earlier arms already cover this arm. |
| `TC0019` | Error | Missing instance method and no usable class default. |
| `TC0020` | Error | Float literal pattern. Float patterns are not supported. |

## Examples

Unknown operator:

```solveig
main = 1 <+> 2
```

produces `RES0003`, because `<+>` has not been imported or defined.

Fixity conflict:

```solveig
infixl 5 <+>
infixr 5 <*>

main = 1 <+> 2 <*> 3
```

produces `RES0008`. Add parentheses to choose an order.

No `Num String` instance:

```solveig
main : String
main = "left" + "right"
```

produces `TC0013`, because `+` is the `Num` method and `String` has no `Num`
instance. Use `appendString` or `prelude.prim.stringAppend` for string
concatenation.
