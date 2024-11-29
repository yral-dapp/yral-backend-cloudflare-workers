# Local testing
```bash
ENV=local npx wrangler dev
```

# Prod deployment
`prod` feat is the fallback when `ENV` is not passed so no need to pass it when deploying to prod...
```bash
npx wrangler publish
```
