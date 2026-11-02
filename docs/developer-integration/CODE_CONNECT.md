# Code Connect

How FigCraft components map to code components. This page describes what exists today. Phase 4 of
the integration adds to it; see [ARCHITECTURE.md](ARCHITECTURE.md).

## The model

A link belongs to a component or component set, one per language, and holds an import path, the code
component's name and a template. Links are stored in the design file (saved, synced, undoable) and
travel through `figcraft.connect.json` in the code repository.

```json
{
  "version": 1,
  "project": {"name": "shop", "assetDir": "public/figcraft"},
  "links": [
    {
      "component": "Button / Primary",
      "language": "react",
      "importPath": "@/components/ui/button",
      "name": "Button",
      "template": "<Button variant={variant} size={size}>{label}</Button>"
    }
  ]
}
```

Languages: `react`, `leptos`, `vue`, `swiftui`, `compose`, `arkui`, `flutter`.

Templates are text and are never executed:

| Placeholder | Becomes |
|---|---|
| `{property}` | The value, as an expression in the language. A property's name or its parameter name |
| `{property ? "a" : "b"}` | One of two strings |
| Any other `{…}` | Stays as written |

## Working with links

| Task | How |
|---|---|
| Create or edit | Inspect tab ▸ Connect to code, or `codeConnect.set` |
| Remove | `codeConnect.remove` |
| Write the file | `codeConnect.export {path}`. Keys it doesn't own, such as `project`, stay |
| Read the file | `codeConnect.import {path}`: links each entry to every component of that name |
| Update a design file in CI | `figcraft-cli connect publish design.figcraft` |

## How agents use them

`inspect.code` and `inspect.componentCode` show a connected component's snippet with the instance's
values and its import lines, and `get_node` reports `codeLinks`. Through MCP these commands run with
`run_command`. A connected instance should be implemented with that snippet instead of generating
a new component.

## Limits today

- Links are matched by component name, so renaming a component breaks its entry in the file.
- The file names an import path, not a source file, and nothing checks that the code exists.
- Variant and default mappings are expressed in the template.
- Importing is explicit: connecting a project doesn't apply its mappings to a design.
