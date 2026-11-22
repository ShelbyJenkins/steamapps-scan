# Documentation style

Do not overwrite existing documentation without an explicit request; documentation is expensive to write. Mark outdated content with a deprecation warning so lint or review catches it. Do not create new Markdown files without explicit consent.

It is acceptable not to document. Obvious implementation details need no comments, and when a comment maps a named behavior to code, rename the code to match the behavior instead.

Use plain text only: no emojis or characters a developer would not type. Do not key lists or comments by number or letter; explicit ordering keys make changes painful.

Keep README files small and concrete: no endless generic sections, no diagrams, no aspirational architecture the code does not support, and no project status. Documentation states what is or what will be; status, when required, is requested and recorded separately.

A design document explains why the design works this way; it is not a how-to guide. Avoid code snippets, variable names, and function names in the main design document unless a design rule cannot be explained without them, so the design is not locked to details that change during implementation.

Describe the ideal state as if it already exists. A design document is the design, not a todo list, and should not change every time part of the design is implemented.

Write as a specification: short, direct paragraphs, each stating one rule, consequence, constraint, exception, or clarifying example. Keep rationale that explains why a rule exists; remove debate, history, and repeated setup. Each sentence should do new work; delete or merge a sentence that only restates an idea in different words.

Use examples only to clarify a subtle rule or prevent a likely misunderstanding, never to restate the rule.

Status updates, future planning, and outstanding questions may appear only at the end of a design document. Split outstanding questions into design decisions to make and research questions requiring investigation, measurement, dependency review, or external confirmation. Code snippets are acceptable in this closing planning section.

Begin each module with `//!` documentation: the crate and module name on one line, an underline of equals signs, then a brief description of purpose, key concepts, main types, and usage patterns.

Do not write trivial comments that restate code. Document what a struct represents, its role, and its primary use cases; each field's purpose, constraints, and relationships; and each function's behavior, parameters, return value, and error conditions.
