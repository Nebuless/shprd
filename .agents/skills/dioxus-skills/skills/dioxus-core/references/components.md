# Components, props, and children

All claims use commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Core contracts

- `packages/core/src/lib.rs`, `Element`: `Result<VNode, RenderError>`. `?` in component render can propagate into nearest error boundary.
- `packages/core/src/lib.rs`, `Component<P>`: function pointer `fn(P) -> Element`.
- `packages/core/src/properties.rs`, `ComponentFunction`: supported functions rebuild from props and return `Element`.
- `packages/core/src/properties.rs`, `Properties`: props are `Clone + Sized + 'static`; `memoize` decides whether old and new props are equal enough to skip work.
- `packages/core/src/nodes.rs`, `VComponent::new`: stores render function pointer, props erased through `VProps`, component name, and body render driver.

## Boundary decision table

| Data shape | Boundary | Reason |
|---|---|---|
| Owned by one child | Prop | Explicit dependency and diff input |
| Needed by many descendants | Context | Avoid unrelated forwarding layers |
| Arbitrary nested UI | `children: Element` | Native RSX child syntax |
| Repeated homogeneous items | Typed collection prop | Parent owns iteration data; child owns item rendering when reusable |
| Deferred rendering with input | Typed callback prop | `Element` children are already built values, not parameterized render functions |
| Root launch dependency | Launch context or zero-arg wrapper | Public launch accepts `fn() -> Element` |

Use `#[component]` for normal function components. It generates props from arguments and checks component signature. Use explicit `#[derive(Props, Clone, PartialEq)]` when struct-level docs, generic control, custom `PartialEq`, or reusable builder type matters. Sources: `packages/core-macro/docs/component.md`, `Component`; `packages/core/src/properties.rs`, `Properties`.

## Prop behavior

| Declaration | Call-site behavior | Source symbol |
|---|---|---|
| Plain field | Required | `FieldInfo::new` generated builder state |
| `Option<T>` | Optional, defaults to `None` | `packages/core-macro/src/props/mod.rs`, `FieldInfo::new` |
| `#[props(!optional)] Option<T>` | Required, caller may pass `None` | same symbol, `strip_option` handling |
| `#[props(default)]` | Optional, uses `Default` | generated props builder |
| `#[props(default = expr)]` | Optional, uses expression | generated props builder |
| `#[props(into)]` | Builder accepts `Into<T>` | generated props builder |
| `String` | Builder accepts display formatting path | `FieldInfo::new`, `from_displayable` |
| Write or Store-like type | Automatic `Into` path | `FieldInfo::new`, `looks_like_write_type`, `looks_like_store_type` |

Do not infer rerender behavior from `Clone`. `Properties::memoize` controls component prop memoization. Derived props normally use generated equality logic; custom props can choose another contract but must keep it correct for visible output.

## Children

Field name `children` is syntax-bearing. In `packages/core-macro/src/props/mod.rs`, `FieldInfo::new`, nonoptional `children` gets default `VNode::empty()`. This makes omitted children legal even when field type is `Element`. Optional `children: Option<Element>` defaults to `None` through option handling.

Choose intentionally:

| Contract | Declaration | Distinguishes omitted from empty? |
|---|---|---|
| Empty content accepted | `children: Element` | No |
| Omission has meaning | `children: Option<Element>` | Yes |
| Caller must spell presence, including `None` | `#[props(!optional)] children: Option<Element>` | Yes, at call site |

Rendering `{children}` propagates its `RenderError` because `Element` is a result. Place boundary outside child subtree when fallback should replace that subtree.

## Component identity edges

- Component function pointer and props form `VComponent`; moving state between component scopes changes lifetime even if rendered RSX looks same.
- Prop equality can skip child rebuild. Never hide visible changing data behind `PartialEq` that reports equality.
- Context is found by concrete `TypeId`. Two semantically different values with same Rust type shadow each other by nearest provider. Use newtypes for separate channels.
- Children defaulting can mask accidentally omitted content. Use optional requiredness when omission is invalid.

## Review checklist

- Component owns one UI responsibility and its local state.
- Props expose data child needs, not parent implementation details.
- Prop types meet `Clone + 'static`; explicit props memoization matches visible behavior.
- `children` omitted behavior is deliberate.
- Shared values use distinct types where multiple contexts could collide.
- Errors from children land in intended boundary.

Official docs: [components](https://dioxuslabs.com/learn/0.7/essentials/ui/components/), [props](https://docs.rs/dioxus-core-macro/0.7/dioxus_core_macro/derive.Props.html). Source above decides generated behavior.
