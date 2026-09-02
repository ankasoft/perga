# Authentication

This service uses Bearer tokens. See [setup](../guides/setup.md) for the full
walkthrough, and [[Token Rotation]] for the rotation policy.

## Obtaining a token

```sh
curl -X POST /auth/token \
  -d '{"key":"..."}'
```

Back to [the index](../../README.md#fixture-vault).
