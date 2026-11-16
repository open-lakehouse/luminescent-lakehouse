# chatcn: third-party code, not under this repository's license

The files in this directory (and its subdirectories) are from
[chatcn](https://chatcn.dev) by Aaditya Chowdhury, installed with the shadcn CLI
(`apps/desktop/components.json`, registry `@chatcn`) and in places adapted (marked
`luminescent:`). They are covered by the **chatcn License**
(<https://chatcn.dev/docs/license>), **not** by the Apache License 2.0 that covers
the rest of this repository. chatcn is free to use but not open source.

The license, as published (2026-11-16):

> chatcn is free to use, but it is not open source. The code you install with the
> shadcn CLI is yours to use and change inside your projects. What you cannot do is
> turn chatcn itself into a product.
>
> **You can**
> - Use chatcn in personal and commercial projects, including products you sell and
>   client work.
> - Copy components into your codebase and modify them however you need.
> - Ship chatcn inside an end product, even one whose source code is public.
>
> **You cannot**
> - Redistribute or sell chatcn, or a large part of it, as a component library, UI
>   kit, template, theme, block collection, icon set or design system, free or paid.
> - Publish chatcn components to a public registry, package manager or marketplace,
>   except as part of an end product.
> - Build a product that competes with chatcn using its code.
> - Present chatcn as your own work or remove the license notice.
>
> **Icons** The icon drawings chatcn shares with NeonKit are also available under
> NeonKit's MIT license. Icons added by chatcn, such as the agent set, are covered
> by the chatcn License.

What that means here: this code ships only inside the desktop app (an end
product). Never move or copy it into a published package: `@open-lakehouse/ui-kit`,
`unity-catalog-ui`, any npm package, registry or release tarball. Keep this notice
and the per-file headers.
