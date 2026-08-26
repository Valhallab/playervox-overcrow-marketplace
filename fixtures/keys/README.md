# Development signing fixture

`development-ed25519.key` is an intentionally public deterministic test seed.
It signs only local catalogs selected with `--development-key`; production
builds reject this key, its key ID, and repository-owned signing paths.

The matching public key is `development-ed25519.pub`. Neither file is a
production secret or a trust anchor for the public marketplace.
