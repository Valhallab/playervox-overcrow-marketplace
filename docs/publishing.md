# Publishing

Publishing copies an already admitted `.ocpkg` from the private accepted store
and signs catalog metadata. It does not compile, retest, or mutate widget
bytes. `marketplace-tool verify-admission` must succeed for the selected review
tree before a production publisher consumes it.

Keep signed catalogs, monotonic sequence, expiry, exact archive digests,
provenance, licenses, human approval, and offline signing.

`marketplace-tool stage-development-catalog` exercises this contract only with
the public development fixture key and fixed loopback origin. It is not a
publication tool. `published/` is the last production snapshot. This reset does
not rewrite it, and no Web API v1 production signer currently exists. Follow
[production operations](production-operations.md) for any later authorized
publication. This document does not authorize a push or deployment.
