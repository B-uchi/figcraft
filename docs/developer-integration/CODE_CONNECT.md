# Code Connect

How FigCraft components map to code components, so an agent reuses them instead of writing new ones.

## The model

A link belongs to a component or component set, one per language, and holds an import path, the code
component's name and a template, and optionally its source file, what variant values are called in
code, and notes. Links are stored in the design file (saved, synced, undoable) and
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
      "template": "<Button variant={Variant} size={Size}>{label}</Button>",
      "source": "src/components/ui/button.tsx",
      "values": {"Variant": {"Primary": "primary"}, "Size": {"Medium": "md"}},
      "notes": "Use the loading prop while a request runs."
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

`values` translates a design value to the code's: with the mapping above, an instance whose `Variant`
is `Primary` renders `variant="primary"`. A value with no entry keeps the design's name. Defaults come
from the component: an instance that leaves a property alone shows its default.

`source` is relative to the code repository and must stay inside it.

## Map a component

1. Select a component or instance and open the **Inspect** tab.
2. Under **Connect to code**, choose the language in the Code section and click **+**.
3. Fill in the import path, name, source file and template, and what each variant value is called in
   code. The snippet above updates as you type.
4. In **Developer Connect**, click **Export** beside `figcraft.connect.json` to save the mappings to
   the project. Commit the file.

In another copy of the project, or in a new design with the same component names, click **Import**.

## Working with links

| Task | How |
|---|---|
| Create or edit | Inspect tab ▸ Connect to code, or `codeConnect.set` |
| Remove | `codeConnect.remove` |
| List | `codeConnect.list {ids?, unmapped?, root?}` |
| Write the file | `codeConnect.export {path}`. Keys it doesn't own, such as `project`, stay, and so do the links of components this design doesn't have |
| Read the file | `codeConnect.import {path}`: links each entry to every component of that name |
| Update a design file in CI | `figcraft-cli connect publish design.figcraft` |

## How agents use them

| Question | Tool |
|---|---|
| Which code component is this? What do I import? | `component_mappings {ids}`: `import`, `name`, `source`, `snippet` |
| Which properties and variants are there? | `component_mappings`: `properties`, with `values` for what needs translating |
| Does the file still exist? | `sourceFound`, for a connected project |
| What has no mapping yet? | `component_mappings {unmapped: true}` |
| Is the project's file newer than this design? | `sync_mappings {direction: "import"}` |

`design_context` gives a connected instance's `code` with its own values, and stops there. The
`implement_design` prompt tells an agent to use these rather than build a replacement.

## Limits

- Links are matched by component name, so renaming a component breaks its entry in the file.
- Importing is explicit, because it changes the design: connecting a project doesn't apply its mappings.
- `sourceFound` says the file exists, not that it exports the named component.
