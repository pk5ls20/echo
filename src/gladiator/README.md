## [Gladiator](https://youtu.be/watch?v=QsrDdI31UIM)

### Concept Overview

In **Gladiator**, during the **output (render) phase** of HTML, we apply **fine‑grained control** over page elements based
on each user’s granted “permissions,” ensuring that the HTML finally delivered to the front end strictly adheres to the
user’s authorization scope.

### Permissions

- **Atomic and orthogonal (non-hierarchical)**: each permission is defined independently; permissions do **not** imply, override, or stack with one another.
- **Per-element single check**: each protected element declares **exactly one** `echo-pm` permission ID; visibility is a boolean check: the element is visible iff the user holds that ID.
- **Users may hold multiple permissions**: a user can have many permission IDs, but for any given element **only the element’s ID** is evaluated—no composition (AND/OR), inheritance, or priority is applied.

> Formal: for an element with permission `p` and a user set `U`, the element is visible iff `p ∈ U`. No union/intersection/priority is used.

### Input Phase

- Unfiltered HTML fragments or data objects coming from the front end, API, or other input sources
- Must follow established input conventions to guarantee predictable downstream processing

### Render (Output) Phase

- Reads the set of permissions held by the current user, retains (or flags) only those element attributes, text nodes,
  and structures that comply with permission rules, and generates the safe, compliant HTML that is sent to the front end

### Element Types

Gladiator currently supports two categories of elements:

1. **Standard echo‑pm Elements** (referred to as “standard elements”)
2. **Extended echo‑pm Elements** (referred to as “extended elements”)

Otherwise, any other tag with echo-pm is considered unsupported.

##### Input Constraints

1. Nesting is currently prohibited for both standard and extended elements, and for nested elements, an error is expected to be thrown during the check parsing phase

---

#### 1. Standard echo‑pm Elements

The standard element controls whether it is shown or hidden in the output with the help of `span`, using only permission tags.

```html
<span echo-pm="x"> {{innerHTML}} </span>
```

##### Input

| Attribute | Nullable? | Type   | Comment                                             |
| --------- | --------- | ------ | --------------------------------------------------- |
| `echo-pm` | No        | usize  | See also “Permission ID”                            |
| innerHTML | Yes       | string | Passed through to the parsing backend exactly as‑is |

##### Render (Output)

```html
<span echo-pm="x" echo-s="x"> {{innerHTML}} </span>
```

| Attribute | Nullable?                                      | Type   | Comment                                                                                                                                                                                                                                                                                 |
| --------- | ---------------------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `echo-pm` | Inherited from input                           | usize  | Inherited from input                                                                                                                                                                                                                                                                    |
| `echo-s`  | Null if permission granted; otherwise not null | usize  | echo-s: when permission denied, this attribute MUST be present and equal to the total number of Unicode grapheme clusters contained in the element’s textual subtree (implementation note: unicode-segmentation with graphemes(true)); when permission granted, echo-s MUST be omitted. |
| innerHTML | Null if permission denied; otherwise inherited | string | Passed through as‑is when permission is granted                                                                                                                                                                                                                                         |

---

#### 2. Extended echo‑pm Elements

The extension element is based on `div`, by introducing the same concept of `echo-pm` as the standard element, with an extension unique ID `echo-ext-id` and extension meta information `echo-ext-meta-{key}`.

```html
<div echo-pm="x"
    echo-ext-id="x"
    echo-ext-meta-{key}="{val}">
   {{innerHTML}}
</div>
```

##### Input

| Attribute             | Nullable?                             | Type   | Comment                                   |
| --------------------- | ------------------------------------- | ------ | ----------------------------------------- |
| `echo-pm`             | Inherited from standard element input | usize  | Inherited from standard element input     |
| `echo-ext-id`         | No                                    | usize  | Extension template ID                     |
| `echo-ext-meta-{key}` | No (if that metadata key is required) | string | Template‑defined metadata key–value pairs |
| innerHTML             | Always null                           | —      |                                           |

##### Render (Output)

| Attribute             | Nullable?                                      | Type   | Comment |
| --------------------- | ---------------------------------------------- | ------ | ------- |
| `echo-pm`             | Inherited from standard element input          | usize  |         |
| `echo-ext-id`         | Inherited from extended element input          | usize  |         |
| `echo-ext-meta-{key}` | Null if permission denied; otherwise inherited | string |         |
| innerHTML             | Null if permission denied; otherwise not null  | —      |         |

---

### Security Constraints

- All input and output HTML must pass through an XSS‑injection filter to prevent potential XSS attacks.
