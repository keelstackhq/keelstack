# @keelstackhq/docspack

The [keelstack docs](https://keelstack.dev/docs/) as a [docspack](https://www.npmjs.com/package/docspack)
documentation package, so an agent in your repo can answer from them offline.

```bash
pnpm add -D docspack@1.5.0 @keelstackhq/docspack
pnpm exec docspack sync
pnpm exec docspack ask "how do I approve a pull request from chat" --json
```

docspack needs Node 22.5 or later. Details: https://keelstack.dev/docs/getting-started/
