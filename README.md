# Local testing
```bash
ENV=local npm run dev:sample-worker
```

# Prod deployment
`prod` feat is the fallback when `ENV` is not passed so no need to pass it when deploying to prod...
```bash
npm run deploy:sample-worker
```

# Create a new worker
- Check sample-worker for a sample
- Create a new worker in the workers directory with Cargo.toml, package.json, src/lib.rs
- Add the worker cmd to the package.json
- Add the worker to the deploy-workers.yml
